//! Astro file-based pages and exported endpoint handlers.
use super::{extractors::regex, nuxt};
use anyhow::Result;
use prometeu_extract::{ExtractionResult, supplemental::Context};

pub(super) fn extract(path: &str, source: &str) -> Result<ExtractionResult> {
    let normalized = path.replace('\\', "/");
    let is_page = normalized.ends_with(".astro");
    let ctx = Context::new(path, source, if is_page { "astro" } else { "typescript" });
    let mut out = ctx.result.clone();
    out.nodes.clear();
    let Some(pages) = regex(r"(?:^|/)src/pages/")?.find(&normalized)? else {
        return Ok(out);
    };
    if !regex(r"\.(astro|ts|js|mjs)$")?.is_match(&normalized)? {
        return Ok(out);
    }
    let after = &normalized[pages.end()..];
    if after.split('/').any(|segment| segment.starts_with('_'))
        || regex(r"\.config\.[a-z]+$")?.is_match(after.rsplit('/').next().unwrap())?
    {
        return Ok(out);
    }
    let without_extension = regex(r"\.(astro|ts|js|mjs)$")?.replace(after, "");
    let without_index = regex(r"(^|/)index$")?.replace(&without_extension, "$1");
    let without_index = without_index.strip_suffix('/').unwrap_or(&without_index);
    let route = format!("/{}", nuxt::parameters(without_index, false)?);
    let route = if route == "/" {
        &route
    } else {
        route.strip_suffix('/').unwrap_or(&route)
    };
    let node = nuxt::page(&ctx, route);
    if is_page {
        out.unresolved_references
            .push(nuxt::component_reference(&node, ".astro"));
    } else {
        for found in regex(r"(?m)^\s*export\s+(?:(?:async\s+)?function\s+(GET|POST|PUT|PATCH|DELETE|OPTIONS|HEAD|ALL)\b|const\s+(GET|POST|PUT|PATCH|DELETE|OPTIONS|HEAD|ALL)\b)")?.captures_iter(source) {
            let found = found?;
            let name = found.get(1).or_else(|| found.get(2)).unwrap().as_str();
            let mut reference = super::routes::reference(&node, name, "references");
            reference.line = ctx.position(found.get(0).unwrap().start()).0;
            reference.candidates = Some(vec![name.into()]);
            out.unresolved_references.push(reference);
        }
    }
    out.nodes.push(node);
    Ok(out)
}
