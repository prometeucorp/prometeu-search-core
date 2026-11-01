//! Vapor route builders, grouped paths, and inline closure references.
use super::{
    extractors::regex,
    routes::{reference, route},
};
use anyhow::Result;
use prometeu_extract::{ExtractionResult, supplemental::Context};
use std::collections::{HashMap, HashSet};

fn segments(source: &str) -> Result<String> {
    let mut path = String::new();
    for matched in regex(r#""([^"]*)""#)?.captures_iter(source) {
        path.push('/');
        path.push_str(&matched?[1]);
    }
    Ok(path)
}
fn handler(expression: &str, types: &HashMap<String, String>) -> Option<String> {
    let mut parts: Vec<_> = expression
        .split('.')
        .filter(|part| !part.is_empty())
        .collect();
    let method = parts.pop()?;
    if parts
        .first()
        .is_some_and(|part| matches!(*part, "self" | "Self"))
    {
        parts.remove(0);
    }
    if parts.is_empty() {
        return Some(format!("@{method}"));
    }
    if parts[0].starts_with(|ch: char| ch.is_ascii_uppercase()) {
        return Some(format!("{}@{method}", parts.join(".")));
    }
    Some(if parts.len() == 1 {
        types
            .get(parts[0])
            .map_or(format!("@{method}"), |ty| format!("{ty}@{method}"))
    } else {
        format!("@{method}")
    })
}
fn brace(source: &str, open: usize) -> Option<usize> {
    let mut depth = 0;
    let mut quoted = false;
    let mut escaped = false;
    for (at, byte) in source.as_bytes()[open..].iter().enumerate() {
        if escaped {
            escaped = false;
            continue;
        }
        if quoted {
            if *byte == b'\\' {
                escaped = true;
            } else if *byte == b'"' {
                quoted = false;
            }
            continue;
        }
        if *byte == b'"' {
            quoted = true;
        } else if *byte == b'{' {
            depth += 1;
        } else if *byte == b'}' {
            depth -= 1;
            if depth == 0 {
                return Some(open + at);
            }
        }
    }
    None
}
fn closure_calls(source: &str) -> Result<Vec<String>> {
    const NOT_CALLS: &[&str] = &[
        "if", "guard", "switch", "while", "for", "return", "catch", "case", "in", "try", "await",
        "throw", "some", "any",
    ];
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let clean = regex(r"[\s?!]")?;
    let expression = regex(r"[.)\]]\s*$")?;
    for matched in
        regex(r"((?:[A-Za-z_]\w*\s*[?!]?\.\s*)*)([A-Za-z_]\w*)\s*\(")?.captures_iter(source)
    {
        let matched = matched?;
        let callee = &matched[2];
        let receiver = clean.replace_all(&matched[1], "");
        let receiver = receiver.strip_suffix('.').unwrap_or(&receiver);
        if receiver.is_empty()
            && (NOT_CALLS.contains(&callee)
                || expression.is_match(&source[..matched.get(0).unwrap().start()])?)
        {
            continue;
        }
        let name = if receiver.is_empty() {
            callee.into()
        } else {
            format!("{receiver}.{callee}")
        };
        if seen.insert(name.clone()) {
            out.push(name);
        }
    }
    Ok(out)
}

pub(super) fn extract(path: &str, source: &str) -> Result<ExtractionResult> {
    let safe = crate::synthesis::comments::strip_for_regex(source, "swift");
    let ctx = Context::new(path, source, "swift");
    let mut out = ctx.result.clone();
    out.nodes.clear();
    if !path.ends_with(".swift") {
        return Ok(out);
    }
    let mut groups: HashMap<String, String> = HashMap::new();
    let mut types = HashMap::new();
    for matched in regex(r"\blet\s+(\w+)\s*=\s*(\w+)\.grouped\s*\(([^)]*)\)")?.captures_iter(&safe)
    {
        let matched = matched?;
        groups.insert(
            matched[1].into(),
            format!(
                "{}{}",
                groups.get(&matched[2]).map_or("", String::as_str),
                segments(&matched[3])?
            ),
        );
    }
    for matched in regex(r"\b(\w+)\.group\s*\(([^)]*)\)\s*\{\s*(\w+)\s+in")?.captures_iter(&safe) {
        let matched = matched?;
        groups.insert(
            matched[3].into(),
            format!(
                "{}{}",
                groups.get(&matched[1]).map_or("", String::as_str),
                segments(&matched[2])?
            ),
        );
    }
    for matched in
        regex(r"\b(?:let|var)\s+([a-z_]\w*)\s*(?::\s*([A-Z]\w*)\s*)?=\s*([A-Z][\w.]*)\s*\(")?
            .captures_iter(&safe)
    {
        let matched = matched?;
        types.insert(
            matched[1].into(),
            matched
                .get(2)
                .unwrap_or(matched.get(3).unwrap())
                .as_str()
                .into(),
        );
    }
    let path_arg = regex(r#"^\s*"[^"]*"\s*$"#)?;
    for (on, pattern) in [
        (
            false,
            r"\b(\w+)\.(get|post|put|patch|delete|head|options)\s*\(\s*((?:[^,()]+,)*\s*)use:\s*([A-Za-z_][\w.]*)",
        ),
        (
            true,
            r"\b(\w+)\.on\s*\(\s*\.([A-Z]+)\s*,\s*((?:(?:[^,()]|\([^()]*\))+,)*\s*)use:\s*([A-Za-z_][\w.]*)",
        ),
    ] {
        for matched in regex(pattern)?.captures_iter(&safe) {
            let matched = matched?;
            let text = matched.get(0).unwrap();
            let args = if on {
                let mut args = Vec::new();
                for arg in matched[3].split(',') {
                    if path_arg.is_match(arg)? {
                        args.push(arg);
                    }
                }
                args.join(",")
            } else {
                matched[3].into()
            };
            let path = format!(
                "{}{}",
                groups.get(&matched[1]).map_or("", String::as_str),
                segments(&args)?
            );
            let path = if path.is_empty() { "/" } else { &path };
            let mut node = route(
                &ctx,
                ctx.position(text.start()).0,
                &matched[2].to_uppercase(),
                path,
                source[text.start()..text.end()].encode_utf16().count() as u32,
            );
            node.qualified_name = format!("{}::route:{path}", ctx.path);
            if let Some(name) = handler(&matched[4], &types) {
                out.unresolved_references
                    .push(reference(&node, &name, "references"));
            }
            out.nodes.push(node);
        }
    }
    let use_arg = regex(r"\buse:")?;
    let url = regex(r#"^\s*"https?:"#)?;
    let statement = regex(r"^\s*(?:(?:try|await)\s+)*$")?;
    let on = regex(r"(?s)^\s*\.([A-Z]+)\s*,?(.*)$")?;
    for matched in regex(
        r"\b(\w+)\.(get|post|put|patch|delete|head|options|webSocket|on)\s*\(([^()]*)\)\s*\{",
    )?
    .captures_iter(&safe)
    {
        let matched = matched?;
        let text = matched.get(0).unwrap();
        let args = &matched[3];
        if use_arg.is_match(args)? || &matched[1] == "client" || url.is_match(args)? {
            continue;
        }
        let line_start = safe[..text.start()].rfind('\n').map_or(0, |at| at + 1);
        if !statement.is_match(&safe[line_start..text.start()])? {
            continue;
        }
        let (method, args) = if &matched[2] == "on" {
            let Some(on) = on.captures(args)? else {
                continue;
            };
            (on[1].to_owned(), on[2].to_owned())
        } else {
            (
                if &matched[2] == "webSocket" {
                    "WS".into()
                } else {
                    matched[2].to_uppercase()
                },
                args.into(),
            )
        };
        let path = format!(
            "{}{}",
            groups.get(&matched[1]).map_or("", String::as_str),
            segments(&args)?
        );
        let path = if path.is_empty() { "/" } else { &path };
        let mut node = route(
            &ctx,
            ctx.position(text.start()).0,
            &method,
            path,
            source[text.start()..text.end()].encode_utf16().count() as u32,
        );
        node.qualified_name = format!("{}::route:{path}", ctx.path);
        let open = text.end() - 1;
        if let Some(close) = brace(&safe, open) {
            for name in closure_calls(&safe[open + 1..close])? {
                out.unresolved_references
                    .push(reference(&node, &name, "calls"));
            }
        }
        out.nodes.push(node);
    }
    Ok(out)
}
