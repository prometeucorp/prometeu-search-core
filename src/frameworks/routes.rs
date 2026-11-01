//! Shared canonical route records and Go HTTP registration extraction.
use super::extractors::regex;
use anyhow::Result;
use prometeu_extract::{ExtractionResult, Node, UnresolvedReference, supplemental::Context};
pub(super) fn route(ctx: &Context<'_>, line: u32, method: &str, path: &str, width: u32) -> Node {
    let mut node = ctx.result.nodes[0].clone();
    let file = &node.file_path;
    node.id = format!("route:{file}:{line}:{method}:{path}");
    node.kind = "route".into();
    node.is_exported = None;
    node.name = format!("{method} {}", if path.is_empty() { "/" } else { path })
        .trim_start()
        .into();
    node.qualified_name = format!("{file}::{method}:{path}");
    node.start_line = line;
    node.end_line = line;
    node.start_column = 0;
    node.end_column = width;
    node
}

pub(super) fn reference(node: &Node, name: &str, kind: &str) -> UnresolvedReference {
    UnresolvedReference {
        from_node_id: node.id.clone(),
        reference_name: name.into(),
        reference_kind: kind.into(),
        line: node.start_line,
        column: 0,
        file_path: Some(node.file_path.clone()),
        language: Some(node.language.clone()),
        candidates: None,
    }
}

pub(super) fn go(path: &str, source: &str) -> Result<ExtractionResult> {
    let safe = crate::synthesis::comments::strip_for_regex(source, "go");
    let ctx = Context::new(path, source, "go");
    let mut out = ctx.result.clone();
    out.nodes.clear();
    if !path.ends_with(".go") {
        return Ok(out);
    }
    let prefix = regex(r"^(GET|POST|PUT|PATCH|DELETE|OPTIONS|HEAD|CONNECT|TRACE)\s+\S")?;
    let whitespace = regex(r"\s+")?;
    let tail = regex(r"(?:\.|^)([A-Za-z_][A-Za-z0-9_]*)$")?;
    for matched in regex(r#"\b\w+\.(GET|POST|PUT|PATCH|DELETE|OPTIONS|HEAD|Get|Post|Put|Patch|Delete|Handle|HandleFunc)\s*\(\s*"([^"]+)"\s*,\s*([^)]+)\)"#)?.captures_iter(&safe) {
        let matched=matched?;let verb=&matched[1];let written=&matched[2];
        let prefix=if matches!(verb,"Handle"|"HandleFunc"){prefix.captures(written)?}else{None};
        if !written.starts_with('/')&&prefix.is_none(){continue;}
        let (method,path)=if let Some(prefix)=prefix {(prefix[1].to_owned(),written[prefix[1].len()..].trim_start())}
            else {(if matches!(verb,"Handle"|"HandleFunc"){"ANY".into()}else{verb.to_uppercase()},written)};
        let text=matched.get(0).unwrap();
        let mut node=route(&ctx,ctx.position(text.start()).0,&method,path,ctx.source[text.start()..text.end()].encode_utf16().count() as u32);
        node.qualified_name=format!("{}::route:{path}",node.file_path);
        let cleaned=whitespace.replace_all(matched[3].trim(),"");
        let cleaned=cleaned.strip_suffix("()").unwrap_or(&cleaned);
        if let Some(handler)=tail.captures(cleaned)? {out.unresolved_references.push(reference(&node,&handler[1],"references"));}
        out.nodes.push(node);
    }
    Ok(out)
}

pub(super) fn goframe(path: &str, source: &str) -> Result<ExtractionResult> {
    let safe = crate::synthesis::comments::strip_for_regex(source, "go");
    let ctx = Context::new(path, source, "go");
    let mut out = ctx.result.clone();
    out.nodes.clear();
    if !path.ends_with(".go") || !source.contains("g.Meta") {
        return Ok(out);
    }
    let package = regex(r"(?m)^\s*package\s+(\w+)")?;
    let package = package.captures(&safe)?;
    let path_pattern = regex(r#"\bpath:"([^"]+)""#)?;
    let method_pattern = regex(r#"\bmethod:"([^"]+)""#)?;
    for matched in
        regex(r"\btype\s+([A-Z]\w*)\s+struct\s*\{\s*g\.Meta\s+`([^`]*)`")?.captures_iter(&safe)
    {
        let matched = matched?;
        let Some(path) = path_pattern.captures(&matched[2])? else {
            continue;
        };
        let method = method_pattern
            .captures(&matched[2])?
            .map_or("ANY".into(), |m| m[1].to_uppercase());
        let text = matched.get(0).unwrap();
        let join = package.as_ref().map_or(matched[1].to_owned(), |p| {
            format!("{}.{}", &p[1], &matched[1])
        });
        let mut node = route(
            &ctx,
            ctx.position(text.start()).0,
            &method,
            &path[1],
            ctx.source[text.start()..text.end()].encode_utf16().count() as u32,
        );
        node.qualified_name = format!("{}::goframe-route:{join}", node.file_path);
        out.nodes.push(node);
    }
    Ok(out)
}
