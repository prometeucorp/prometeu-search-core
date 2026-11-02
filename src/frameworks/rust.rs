//! Actix, Axum and Rocket route declarations and handler sites.
use super::{
    extractors::regex,
    routes::{reference, route},
};
use anyhow::Result;
use prometeu_extract::{ExtractionResult, supplemental::Context};

fn push(
    ctx: &Context<'_>,
    out: &mut ExtractionResult,
    path: &str,
    method: &str,
    expression: &str,
    line: u32,
    end: usize,
) {
    let Some(handler) = expression.rsplit("::").find(|part| !part.is_empty()) else {
        return;
    };
    let at = end - expression.len() + expression.rfind(handler).unwrap();
    let (site_line, column) = ctx.position(at);
    let mut node = route(ctx, line, &method.to_uppercase(), path, 0);
    node.qualified_name = format!("{}::route:{path}", ctx.path);
    let mut target = reference(&node, handler, "references");
    target.line = site_line;
    target.column = column;
    out.nodes.push(node);
    out.unresolved_references.push(target);
}

pub(super) fn unit_end(source: &str, start: usize, count: usize) -> usize {
    let mut units = 0;
    for (at, ch) in source[start..].char_indices() {
        units += ch.len_utf16();
        if units > count {
            return start + at;
        }
    }
    source.len()
}

pub(super) fn extract(path: &str, source: &str) -> Result<ExtractionResult> {
    let safe = crate::synthesis::comments::strip_for_regex(source, "rust");
    let ctx = Context::new(path, source, "rust");
    let mut out = ctx.result.clone();
    out.nodes.clear();
    if !path.ends_with(".rs") {
        return Ok(out);
    }
    let handler = regex(r"\n\s*(?:pub\s+)?(?:async\s+)?fn\s+(\w+)")?;
    for matched in
        regex(r#"#\[(get|post|put|patch|delete|head|options)\s*\(\s*["']([^"']+)["'][^\]]*\)\]"#)?
            .captures_iter(&safe)
    {
        let matched = matched?;
        let text = matched.get(0).unwrap();
        let mut node = route(
            &ctx,
            ctx.position(text.start()).0,
            &matched[1].to_uppercase(),
            &matched[2],
            source[text.start()..text.end()].encode_utf16().count() as u32,
        );
        node.qualified_name = format!("{path}::route:{}", &matched[2]);
        if let Some(handler) = handler.captures(&safe[text.end()..])? {
            out.unresolved_references
                .push(reference(&node, &handler[1], "references"));
        }
        out.nodes.push(node);
    }
    let path_pattern = regex(r#"^\s*"([^"]+)"\s*,"#)?;
    let methods =
        regex(r"\b(get|post|put|patch|delete|head|options|trace)\s*\(\s*([A-Za-z_][\w:]*)")?;
    let mut work = 0usize;
    for opener in regex(r"\.route\s*\(")?.find_iter(&safe) {
        let opener = opener?;
        let open = opener.end() - 1;
        let mut depth = 0;
        let mut close = None;
        for (at, byte) in safe.as_bytes()[open..].iter().enumerate() {
            work += 1;
            anyhow::ensure!(work <= 10_000_000, "route extraction work budget exceeded");
            if *byte == b'(' {
                depth += 1;
            } else if *byte == b')' {
                depth -= 1;
                if depth == 0 {
                    close = Some(open + at);
                    break;
                }
            }
        }
        let Some(close) = close else {
            continue;
        };
        let args = &safe[open + 1..close];
        let Some(path_match) = path_pattern.captures(args)? else {
            continue;
        };
        let route_path = &path_match[1];
        let line = ctx.position(opener.start()).0;
        let body_at = open + 1 + path_match.get(0).unwrap().end();
        let body = &safe[body_at..close];
        let mut depth = 0i32;
        let mut scanned = 0;
        for matched in methods.captures_iter(body) {
            let matched = matched?;
            let text = matched.get(0).unwrap();
            for byte in &body.as_bytes()[scanned..text.start()] {
                if *byte == b'(' {
                    depth += 1;
                } else if *byte == b')' {
                    depth -= 1;
                }
            }
            scanned = text.start();
            if depth > 0 {
                continue;
            }
            push(
                &ctx,
                &mut out,
                route_path,
                &matched[1],
                &matched[2],
                line,
                body_at + text.end(),
            );
        }
    }
    let method_to =
        regex(r"web::(get|post|put|patch|delete|head)\s*\(\s*\)\s*\.to\s*\(\s*([A-Za-z_][\w:]*)")?;
    let direct = regex(r"^\s*\.to\s*\(\s*([A-Za-z_][\w:]*)")?;
    for matched in regex(r#"web::resource\s*\(\s*"([^"]+)"\s*\)"#)?.captures_iter(&safe) {
        let matched = matched?;
        let text = matched.get(0).unwrap();
        let after = text.end();
        let line = ctx.position(text.start()).0;
        let mut end = unit_end(source, after, 500).min(
            safe[after..]
                .find("web::resource")
                .map_or(safe.len(), |at| after + at),
        );
        let mut depth = 0i32;
        for (at, byte) in safe.as_bytes()[after..end].iter().enumerate() {
            if *byte == b'(' {
                depth += 1;
            } else if *byte == b')' {
                depth -= 1;
                if depth < 0 {
                    end = after + at;
                    break;
                }
            }
        }
        let chain = &safe[after..end];
        let mut found = false;
        for method in method_to.captures_iter(chain) {
            let method = method?;
            let text = method.get(0).unwrap();
            let site = line
                + chain[..text.start()]
                    .bytes()
                    .filter(|byte| *byte == b'\n')
                    .count() as u32;
            push(
                &ctx,
                &mut out,
                &matched[1],
                &method[1],
                &method[2],
                site,
                after + text.end(),
            );
            found = true;
        }
        if !found && let Some(direct) = direct.captures(chain)? {
            push(
                &ctx,
                &mut out,
                &matched[1],
                "ANY",
                &direct[1],
                line,
                after + direct.get(0).unwrap().end(),
            );
        }
    }
    for matched in regex(r#"\.route\s*\(\s*"([^"]+)"\s*,\s*web::(get|post|put|patch|delete|head)\s*\(\s*\)\s*\.to\s*\(\s*([A-Za-z_][\w:]*)"#)?.captures_iter(&safe) {
        let matched=matched?;let text=matched.get(0).unwrap();push(&ctx,&mut out,&matched[1],&matched[2],&matched[3],ctx.position(text.start()).0,text.end());
    }
    Ok(out)
}
