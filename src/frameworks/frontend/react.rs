//! React Router declarations supplied by the reference framework extractor.
use super::react_routes;
use anyhow::Result;
use prometeu_extract::{ExtractionResult, UnresolvedReference, supplemental::Context};
pub(crate) fn extract(path: &str, source: &str) -> Result<ExtractionResult> {
    let language = if path.ends_with(".tsx") { "tsx" } else { "jsx" };
    let ctx = Context::new(path, source, language);
    let mut out = ctx.result.clone();
    out.nodes.clear();
    for route in react_routes::declarations(
        source,
        ![".ts", ".mts", ".cts"]
            .iter()
            .any(|suffix| path.ends_with(suffix)),
    )? {
        let line = ctx.position(route.at).0;
        let mut node = ctx.result.nodes[0].clone();
        node.id = format!("route:{path}:{line}:{}", route.path);
        node.kind = "route".into();
        node.is_exported = None;
        node.name = route.path.clone();
        node.qualified_name = format!("{path}::route:{}", route.path);
        node.start_line = line;
        node.end_line = line;
        node.start_column = 0;
        node.end_column = 0;
        if route
            .parts
            .iter()
            .any(|part| part.expr.as_ref().is_some_and(|expr| !expr.is_empty()))
        {
            node.signature = Some(format!(
                "route-parts:{}",
                serde_json::to_string(&route.parts)?
            ));
        }
        let target = route.component.or(route.lazy);
        for name in target.into_iter().chain(
            route
                .layouts
                .into_iter()
                .map(|name| format!("layout:{name}")),
        ) {
            out.unresolved_references.push(UnresolvedReference {
                from_node_id: node.id.clone(),
                reference_name: name,
                reference_kind: "references".into(),
                line,
                column: 0,
                file_path: Some(path.into()),
                language: Some(language.into()),
                candidates: None,
            });
        }
        out.nodes.push(node);
    }
    Ok(out)
}
// Recompute additions independently of previously generated table records.
pub(crate) fn finalize(
    nodes: &[prometeu_extract::Node],
    sources: &std::collections::BTreeMap<&str, &str>,
    resolve_module: &super::ModuleResolver<'_>,
) -> Result<super::react_tables::Finalized> {
    super::react_tables::Context::new(nodes, sources, resolve_module)?.finalize()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    #[test]
    fn route_extraction_and_scanning_match_reference() {
        let corpus: Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/upstream/synthesis/frontend/react-routes.json"
        ))
        .unwrap();
        for case in corpus["extraction"].as_array().unwrap() {
            let result = extract(
                case["path"].as_str().unwrap(),
                case["source"].as_str().unwrap(),
            )
            .unwrap();
            let mut actual =
                json!({"nodes":result.nodes,"references":result.unresolved_references});
            for node in actual["nodes"].as_array_mut().unwrap() {
                node.as_object_mut().unwrap().remove("updatedAt");
            }
            assert_eq!(
                actual, case["expected"],
                "{} {}",
                case["path"], case["source"]
            );
        }
        for case in corpus["scans"].as_array().unwrap() {
            let source = case["source"].as_str().unwrap();
            let result = react_routes::scan(
                source,
                true,
                react_routes::Options {
                    objects: true,
                    uses: true,
                    table: None,
                },
            )
            .unwrap();
            assert_eq!(json!(result), case["expected"], "{source}");
        }
    }
}
