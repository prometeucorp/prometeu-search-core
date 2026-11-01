//! File-based Nuxt pages, API routes, and middleware.
use super::extractors::regex;
use anyhow::Result;
use prometeu_extract::{ExtractionResult, Node, UnresolvedReference, supplemental::Context};

pub(super) fn page(ctx: &Context<'_>, route: &str) -> Node {
    let mut node = ctx.result.nodes[0].clone();
    let path = &node.file_path;
    node.id = format!("route:{path}:{route}:1");
    node.kind = "route".into();
    node.name = route.into();
    node.qualified_name = format!("{path}::route:{route}");
    node.start_line = 1;
    node.end_line = 1;
    node.start_column = 0;
    node.end_column = 0;
    node.is_exported = None;
    node
}

pub(super) fn component_reference(node: &Node, extension: &str) -> UnresolvedReference {
    let basename = node.file_path.rsplit(['/', '\\']).next().unwrap();
    let name = basename.strip_suffix(extension).unwrap_or(basename);
    let mut reference = super::routes::reference(node, name, "calls");
    reference.candidates = Some(vec![name.into()]);
    reference
}

pub(super) fn parameters(path: &str, optional: bool) -> Result<String> {
    let route = regex(r"\[\.\.\.([^\]]+)\]")?.replace_all(path, "*$1");
    let route = if optional {
        regex(r"\[{2}([^\]]+)\]{2}")?
            .replace_all(&route, ":$1?")
            .into_owned()
    } else {
        route.into_owned()
    };
    Ok(regex(r"\[([^\]]+)\]")?
        .replace_all(&route, ":$1")
        .into_owned())
}

pub(super) fn extract(path: &str, source: &str) -> Result<ExtractionResult> {
    let normalized = format!("/{}", path.replace('\\', "/"));
    let language = if normalized.ends_with(".vue") {
        "vue"
    } else {
        "typescript"
    };
    let ctx = Context::new(path, source, language);
    let mut out = ctx.result.clone();
    out.nodes.clear();
    if let Some((_, after)) = normalized.split_once("/pages/")
        && let Some(without_extension) = after.strip_suffix(".vue")
    {
        let without_index = regex(r"(?:^|/)index$")?.replace(without_extension, "");
        let route = format!("/{}", parameters(&without_index, true)?);
        let route = if route == "/" {
            &route
        } else {
            route.strip_suffix('/').unwrap_or(&route)
        };
        let node = page(&ctx, route);
        out.unresolved_references
            .push(component_reference(&node, ".vue"));
        out.nodes.push(node);
    }
    if let Some((_, after)) = normalized.split_once("/server/api/") {
        let without_extension = regex(r"\.[^/.]+$")?.replace(after, "");
        let without_index = regex(r"(?:^|/)index$")?.replace(&without_extension, "");
        let route = parameters(&without_index, false)?;
        out.nodes.push(page(
            &ctx,
            &if route.is_empty() {
                "/api".into()
            } else {
                format!("/api/{route}")
            },
        ));
    }
    if let Some((_, after)) = normalized.split_once("/middleware/") {
        let name = regex(r"\.[^/.]+$")?.replace(after, "");
        let mut node = page(&ctx, &name);
        node.id = format!("middleware:{path}:{name}:1");
        node.kind = "function".into();
        node.qualified_name = format!("{path}::middleware:{name}");
        out.nodes.push(node);
    }
    Ok(out)
}
