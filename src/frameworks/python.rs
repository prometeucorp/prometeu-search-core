//! Python route declarations from source, including imperative Flask registration.
use super::{
    extractors::regex,
    routes::{reference, route},
};
use anyhow::Result;
use prometeu_extract::{ExtractionResult, supplemental::Context};
use std::collections::BTreeMap;
use std::collections::HashSet;

pub(super) fn extract(path: &str, source: &str, framework: &str) -> Result<ExtractionResult> {
    let safe = crate::synthesis::comments::strip_for_regex(source, "python");
    let ctx = Context::new(path, source, "python");
    let mut out = ctx.result.clone();
    out.nodes.clear();
    if !path.ends_with(".py") {
        return Ok(out);
    }
    match framework {
        "django" => django(&ctx, &safe, &mut out)?,
        "flask" => {
            decorators(&ctx, &safe, false, &mut out)?;
            restful(&ctx, &safe, &mut out)?;
            url_rules(&ctx, &safe, &mut out)?;
        }
        "fastapi" => decorators(&ctx, &safe, true, &mut out)?,
        _ => {}
    }
    Ok(out)
}

fn django(ctx: &Context<'_>, source: &str, out: &mut ExtractionResult) -> Result<()> {
    let include = regex(r#"^include\s*\(\s*['"]([^'"]+)['"]"#)?;
    let trailing = regex(r"\.(?:as_view|\w+)\s*\([^)]*\)\s*$")?;
    let identifier = regex(r"^[A-Za-z_][A-Za-z0-9_]*$")?;
    for matched in
        regex(r#"\b(path|re_path|url)\s*\(\s*r?['"]([^'"]+)['"]\s*,\s*([\w.]+(?:\s*\([^)]*\))?)"#)?
            .captures_iter(source)
    {
        let matched = matched?;
        let text = matched.get(0).unwrap();
        let line = ctx.position(text.start()).0;
        let path = &matched[2];
        let mut node = route(
            ctx,
            line,
            "",
            path,
            ctx.source[text.start()..text.end()].encode_utf16().count() as u32,
        );
        node.id = format!("route:{}:{line}:{path}", node.file_path);
        node.qualified_name = format!("{}::route:{path}", node.file_path);
        let expression = matched[3].trim();
        if let Some(module) = include.captures(expression)? {
            out.unresolved_references
                .push(reference(&node, &module[1], "imports"));
        } else {
            let head = trailing.replace(expression, "");
            if let Some(name) = head.split('.').rfind(|part| !part.is_empty())
                && identifier.is_match(name)?
            {
                out.unresolved_references
                    .push(reference(&node, name, "references"));
            }
        }
        out.nodes.push(node);
    }
    for matched in
        regex(r#"\.register\s*\(\s*r?['"]([^'"]+)['"]\s*,\s*([\w.]+)"#)?.captures_iter(source)
    {
        let matched = matched?;
        let name = matched[2].split('.').next_back().unwrap();
        if !regex(r"View(Set)?$")?.is_match(name)? {
            continue;
        }
        let prefix = matched[1].strip_prefix('^').unwrap_or(&matched[1]);
        let prefix = prefix
            .strip_suffix('$')
            .map_or(prefix, |p| p.strip_suffix('/').unwrap_or(p));
        let text = matched.get(0).unwrap();
        let line = ctx.position(text.start()).0;
        let mut node = route(
            ctx,
            line,
            "VIEWSET",
            prefix,
            ctx.source[text.start()..text.end()].encode_utf16().count() as u32,
        );
        node.name = format!("VIEWSET /{prefix}");
        node.qualified_name = format!("{}::route:{prefix}", node.file_path);
        out.unresolved_references
            .push(reference(&node, name, "references"));
        out.nodes.push(node);
    }
    Ok(())
}

fn decorators(
    ctx: &Context<'_>,
    source: &str,
    fastapi: bool,
    out: &mut ExtractionResult,
) -> Result<()> {
    let pattern = if fastapi {
        r#"@(\w+)\.(get|post|put|patch|delete|options|head)\s*\(\s*['"]([^'"]*)['"]"#
    } else {
        r#"@(\w+)\.route\s*\(\s*['"]([^'"]*)['"](?:\s*,\s*methods\s*=\s*[[(]([^\])]+)[\])])?\s*\)"#
    };
    let listed = regex(r#"(?i)['"]([A-Z]+)['"]"#)?;
    let handler = regex(r"\n\s*(?:async\s+)?def\s+(\w+)")?;
    for matched in regex(pattern)?.captures_iter(source) {
        let matched = matched?;
        let path = &matched[if fastapi { 3 } else { 2 }];
        let method = if fastapi {
            matched[2].to_uppercase()
        } else if let Some(methods) = matched.get(3) {
            listed
                .captures(methods.as_str())?
                .map_or("GET".into(), |m| m[1].to_uppercase())
        } else {
            "GET".into()
        };
        let text = matched.get(0).unwrap();
        let node = route(
            ctx,
            ctx.position(text.start()).0,
            &method,
            path,
            ctx.source[text.start()..text.end()].encode_utf16().count() as u32,
        );
        if let Some(handler) = handler.captures(&source[text.end()..])? {
            out.unresolved_references
                .push(reference(&node, &handler[1], "references"));
        }
        out.nodes.push(node);
    }
    Ok(())
}

fn restful(ctx: &Context<'_>, source: &str, out: &mut ExtractionResult) -> Result<()> {
    let strings = regex(r#"['"]([^'"]+)['"]"#)?;
    for matched in regex(r#"\.add\w*[Rr]esource\s*\(\s*(\w+)\s*,\s*((?:['"][^'"]+['"]\s*,?\s*)+)"#)?
        .captures_iter(source)
    {
        let matched = matched?;
        let line = ctx.position(matched.get(0).unwrap().start()).0;
        for path in strings.captures_iter(&matched[2]) {
            let path = path?;
            let node = route(ctx, line, "ANY", &path[1], 0);
            out.unresolved_references
                .push(reference(&node, &matched[1], "references"));
            out.nodes.push(node);
        }
    }
    Ok(())
}

/// Read one balanced call, retaining the reference's 4,000 UTF-16-unit ceiling.
fn balanced_args(text: &str, open: usize) -> Option<&str> {
    let mut depth = 1;
    let mut quote = None;
    let mut escaped = false;
    let mut units = 0;
    for (offset, ch) in text[open..].char_indices() {
        if units >= 4000 {
            break;
        }
        units += ch.len_utf16();
        if escaped {
            escaped = false;
            continue;
        }
        if let Some(q) = quote {
            if ch == '\\' {
                escaped = true;
            } else if ch == q {
                quote = None;
            }
        } else if matches!(ch, '\'' | '"') {
            quote = Some(ch);
        } else if matches!(ch, '(' | '[' | '{') {
            depth += 1;
        } else if matches!(ch, ')' | ']' | '}') {
            depth -= 1;
            if depth == 0 {
                return Some(&text[open..open + offset]);
            }
        }
    }
    None
}

fn url_rules(ctx: &Context<'_>, source: &str, out: &mut ExtractionResult) -> Result<()> {
    let opener = regex(r"(\.add_url_rule|\b[A-Za-z_]\w*)\s*\(")?;
    let first = regex(r#"^\s*(?:rule\s*=\s*)?(['"])([^'"]*)\1"#)?;
    let list = regex(
        r#"(?:^|,)\s*(?:routes|rules|urls|paths)?\s*=?\s*\[((?:\s*(['"])[^'"]*\2\s*,?)+)\]"#,
    )?;
    let quoted = regex(r#"(['"])([^'"]*)\1"#)?;
    let keyword = regex(r"\bview_func\s*=\s*([A-Za-z_][\w.]*)")?;
    let positional =
        regex(r#"^\s*(['"])[^'"]*\1\s*,\s*(?:(['"])[^'"]*\2|None)\s*,\s*([A-Za-z_][\w.]*)"#)?;
    let methods = regex(r"\bmethods\s*=\s*[[(]([^\])]*)[\])]")?;
    let method_name = regex(r#"['"](\w+)['"]"#)?;
    let mut seen = HashSet::new();
    let mut cursor = 0;
    while let Some(matched) = opener.captures_from_pos(source, cursor)? {
        let text = matched.get(0).unwrap();
        let open = text.end();
        cursor = open;
        let Some(args) = balanced_args(source, open) else {
            continue;
        };
        let rule = &matched[1] == ".add_url_rule";
        if !rule && !regex(r"\bview_func\s*=")?.is_match(args)? {
            continue;
        }
        let paths = if rule {
            first
                .captures(args)?
                .map_or(Vec::new(), |m| vec![m[2].to_owned()])
        } else if let Some(list) = list.captures(args)? {
            quoted
                .captures_iter(&list[1])
                .map(|m| Ok(m?[2].to_owned()))
                .collect::<Result<Vec<_>>>()?
        } else {
            Vec::new()
        };
        let paths: Vec<_> = paths
            .into_iter()
            .filter(|path| path.starts_with('/'))
            .collect();
        if paths.is_empty() {
            continue;
        }
        let expression = keyword.captures(args)?.map(|m| m[1].to_owned());
        let expression = if expression.is_none() && rule {
            positional.captures(args)?.map(|m| m[3].to_owned())
        } else {
            expression
        };
        let Some(expression) = expression else {
            continue;
        };
        let class_view = expression.ends_with(".as_view");
        let target = expression.strip_suffix(".as_view").unwrap_or(&expression);
        let name = target.split('.').next_back().unwrap();
        let listed = methods.captures(args)?;
        let methods = if let Some(listed) = listed.filter(|m| !m[1].is_empty()) {
            method_name
                .captures_iter(&listed[1])
                .map(|m| Ok(m?[1].to_uppercase()))
                .collect::<Result<Vec<_>>>()?
        } else {
            vec![if class_view { "ANY" } else { "GET" }.into()]
        };
        let line = ctx.position(text.start()).0;
        for path in paths {
            for method in &methods {
                let node = route(ctx, line, method, &path, 0);
                if seen.insert(node.id.clone()) {
                    out.unresolved_references
                        .push(reference(&node, name, "references"));
                    out.nodes.push(node);
                }
            }
        }
        cursor = open + args.len();
    }
    Ok(())
}

fn join_paths(prefix: &str, path: &str) -> String {
    let prefix = prefix.trim_end_matches('/');
    let path = path.trim_start_matches('/');
    match (prefix.is_empty(), path.is_empty()) {
        (true, true) => String::new(),
        (true, false) => format!("/{path}"),
        (false, true) => prefix.into(),
        (false, false) => format!("{prefix}/{path}"),
    }
}

/// Compose literal router prefixes over committed imports and source files.
pub(super) fn post_extract(
    nodes: &[prometeu_extract::Node],
    files: &BTreeMap<String, String>,
    imports: &BTreeMap<String, Vec<crate::parser::Import>>,
    modules: &crate::graph::PythonModules,
) -> Result<Vec<prometeu_extract::Node>> {
    struct Mount {
        from: String,
        prefix: String,
        file: String,
        variable: String,
    }
    let declaration = regex(r"\b([A-Za-z_]\w*)\s*=\s*APIRouter\s*\(([^)]*)\)")?;
    let literal = regex(r#"\bprefix\s*=\s*['"]([^'"]*)['"]"#)?;
    let computed = regex(r"\bprefix\s*=")?;
    let decorator = regex(r"@([A-Za-z_]\w*)\.(?:get|post|put|patch|delete|options|head)\s*\(")?;
    let include =
        regex(r"\b([A-Za-z_]\w*)\.include_router\s*\(\s*([A-Za-z_][\w.]*)\s*((?:,[^)]*)?)\)")?;
    let mut own: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    let mut mounts: BTreeMap<String, Vec<Mount>> = BTreeMap::new();
    let mut receivers = BTreeMap::new();
    for (path, source) in files {
        if !path.ends_with(".py")
            || (!source.contains("APIRouter") && !source.contains("include_router"))
        {
            continue;
        }
        let safe = crate::synthesis::comments::strip_for_regex(source, "python");
        let positions = Context::new(path, source, "python");
        let mut variables = BTreeMap::new();
        for matched in declaration.captures_iter(&safe) {
            let matched = matched?;
            let prefix = literal
                .captures(&matched[2])?
                .map_or(String::new(), |m| m[1].into());
            variables.insert(matched[1].to_owned(), prefix);
        }
        let mut lines = BTreeMap::new();
        for matched in decorator.captures_iter(&safe) {
            let matched = matched?;
            lines.insert(
                positions.position(matched.get(0).unwrap().start()).0,
                matched[1].to_owned(),
            );
        }
        receivers.insert(path.clone(), lines);
        for matched in include.captures_iter(&safe) {
            let matched = matched?;
            let rest = matched.get(3).map_or("", |m| m.as_str());
            let prefix = literal.captures(rest)?;
            if prefix.is_none() && computed.is_match(rest)? {
                continue;
            }
            let parts: Vec<_> = matched[2].split('.').collect();
            let head = parts[0];
            let target = if parts.len() == 1 && variables.contains_key(head) {
                Some((path.clone(), head.into()))
            } else if let Some(mapping) = imports
                .get(path)
                .into_iter()
                .flatten()
                .find(|mapping| mapping.local == head)
            {
                if parts.len() > 1 {
                    modules
                        .resolve(&format!("{}.{}", mapping.module, mapping.imported), path)
                        .or_else(|| modules.resolve(&mapping.module, path))
                        .map(|file| (file, parts.last().unwrap().to_string()))
                } else {
                    modules
                        .resolve(&mapping.module, path)
                        .map(|file| (file, mapping.imported.clone()))
                }
            } else {
                None
            };
            let Some((file, variable)) = target else {
                continue;
            };
            mounts.entry(path.clone()).or_default().push(Mount {
                from: matched[1].into(),
                prefix: prefix.map_or(String::new(), |m| m[1].into()),
                file,
                variable,
            });
        }
        own.insert(path.clone(), variables);
    }
    let mut base: BTreeMap<(String, String), String> = BTreeMap::new();
    for _ in 0..8 {
        let mut next: BTreeMap<(String, String), Option<String>> = BTreeMap::new();
        for (file, mounts) in &mounts {
            for mount in mounts {
                let prefix = base
                    .get(&(file.clone(), mount.from.clone()))
                    .map_or("", String::as_str);
                let own = own
                    .get(file)
                    .and_then(|vars| vars.get(&mount.from))
                    .map_or("", String::as_str);
                let full = join_paths(&join_paths(prefix, own), &mount.prefix);
                let key = (mount.file.clone(), mount.variable.clone());
                match next.entry(key) {
                    std::collections::btree_map::Entry::Vacant(entry) => {
                        entry.insert(Some(full));
                    }
                    std::collections::btree_map::Entry::Occupied(mut entry) => {
                        if entry.get().as_ref() != Some(&full) {
                            entry.insert(None);
                        }
                    }
                }
            }
        }
        let next: BTreeMap<_, _> = next
            .into_iter()
            .filter_map(|(key, value)| value.map(|value| (key, value)))
            .collect();
        let changed = next != base;
        base = next;
        if !changed {
            break;
        }
    }
    let mut updates = Vec::new();
    for node in nodes.iter().filter(|node| node.kind == "route") {
        let Some(variable) = receivers
            .get(&node.file_path)
            .and_then(|lines| lines.get(&node.start_line))
        else {
            continue;
        };
        let prefix = join_paths(
            base.get(&(node.file_path.clone(), variable.clone()))
                .map_or("", String::as_str),
            own.get(&node.file_path)
                .and_then(|vars| vars.get(variable))
                .map_or("", String::as_str),
        );
        let Some((_, qualified)) = node.qualified_name.split_once("::") else {
            continue;
        };
        let Some((method, original)) = qualified.split_once(':') else {
            continue;
        };
        // Derive every name from its original qualified declaration, so removing
        // a mount also removes the old prefix from an unchanged route file.
        let path = if prefix.is_empty() || prefix == "/" {
            if original.is_empty() {
                "/".into()
            } else {
                original.into()
            }
        } else {
            join_paths(&prefix, original)
        };
        let name = format!("{method} {path}")
            .trim_matches(crate::search::query_whitespace)
            .to_owned();
        if name != node.name {
            let mut updated = node.clone();
            updated.name = name;
            updates.push(updated);
        }
    }
    Ok(updates)
}
