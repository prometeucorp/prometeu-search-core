//! File-based page and endpoint declarations used by frontend route matching.
// Ported from the pinned MIT-licensed reference; see THIRD_PARTY_NOTICES.md.
use super::extractors::regex;
use anyhow::{Result, bail};
use prometeu_extract::{ExtractionResult, Node, supplemental::Context};
use std::collections::HashSet;
pub(crate) mod angular;
pub(crate) type ModuleResolver<'a> = dyn Fn(&str, &str, &str) -> Result<Option<String>> + 'a;
pub(crate) mod href;
pub(crate) mod jsx_code;
pub(crate) mod navigation;
pub(crate) mod object;
pub(crate) mod paths;
pub(crate) mod react;
pub(crate) mod react_exports;
pub(crate) mod react_loader;
pub(crate) mod react_router;
pub(crate) mod react_routes;
pub(crate) mod react_tables;
pub(crate) mod tables;
pub(crate) mod tanstack;
pub(crate) mod vue_router;

pub(crate) fn extract(path: &str, source: &str, framework: &str) -> Result<ExtractionResult> {
    if framework == "react" {
        return react::extract(path, source);
    }
    if framework == "angular-router" {
        return angular::extract(path, source);
    }
    if framework == "nuxt" {
        return super::nuxt::extract(path, source);
    }
    if framework == "vue-router" {
        return vue_router::extract(path, source);
    }
    if framework == "tanstack-router" {
        return tanstack::extract(path, source);
    }
    let language = language(path, framework == "svelte");
    let ctx = Context::new(path, source, language);
    let mut output = ctx.result.clone();
    output.nodes.clear();
    if framework == "svelte" {
        if let Some(route) = paths::svelte(path)? {
            let mut node = page_node(&ctx, &route);
            node.id.push_str(":1");
            node.is_exported = None;
            output.nodes.push(node);
        }
        return Ok(output);
    }
    let (kind, route) = match framework {
        "nextjs" => {
            let Some(route) = paths::next(path)? else {
                return Ok(output);
            };
            (route.kind, route.path)
        }
        "expo-router" => {
            if let Some(route) = paths::expo_api(path)? {
                (paths::Kind::Handler, route)
            } else if let Some(route) = paths::expo(path)? {
                (paths::Kind::Page, route)
            } else {
                return Ok(output);
            }
        }
        _ => bail!("unsupported frontend framework: {framework}"),
    };
    let safe = crate::synthesis::comments::strip_for_regex(source, "typescript");
    if kind == paths::Kind::Handler {
        let declaration = regex(
            r"\bexport\s+(?:async\s+)?function\s+(GET|POST|PUT|PATCH|DELETE|HEAD|OPTIONS)\b|\bexport\s+(?:const|let)\s+(GET|POST|PUT|PATCH|DELETE|HEAD|OPTIONS)\s*=",
        )?;
        let mut seen = HashSet::new();
        for found in declaration.captures_iter(&safe) {
            let found = found?;
            let method = found.get(1).or_else(|| found.get(2)).unwrap().as_str();
            if !seen.insert(method.to_owned()) {
                continue;
            }
            let text = found.get(0).unwrap();
            let mut node = super::routes::route(
                &ctx,
                ctx.position(text.start()).0,
                method,
                &route,
                source[text.start()..text.end()].encode_utf16().count() as u32,
            );
            node.is_exported = Some(true);
            let mut reference = super::routes::reference(&node, method, "references");
            reference.candidates = Some(vec![method.to_owned()]);
            output.unresolved_references.push(reference);
            output.nodes.push(node);
        }
    } else {
        let mut node = page_node(&ctx, &route);
        if kind == paths::Kind::Api {
            node.id = format!("route:{path}:1:ANY:{route}");
            node.name = format!("ANY {route}");
            node.qualified_name = format!("{path}::ANY:{route}");
        }
        if let Some((name, at)) = default_export(&safe)? {
            let mut reference = super::routes::reference(
                &node,
                &name,
                if kind == paths::Kind::Api {
                    "references"
                } else {
                    "calls"
                },
            );
            reference.line = ctx.position(at).0;
            reference.candidates = Some(vec![name]);
            output.unresolved_references.push(reference);
        }
        output.nodes.push(node);
    }
    Ok(output)
}

fn page_node(ctx: &Context<'_>, route: &str) -> Node {
    let mut node = ctx.result.nodes[0].clone();
    let path = &node.file_path;
    node.id = format!("route:{path}:{route}");
    node.kind = "route".into();
    node.name = route.to_owned();
    node.qualified_name = format!("{path}::route:{route}");
    node.start_line = 1;
    node.end_line = 1;
    node.start_column = 0;
    node.end_column = 0;
    node.is_exported = Some(true);
    node
}
fn language(path: &str, svelte: bool) -> &'static str {
    if svelte {
        return if path.ends_with(".svelte") {
            "svelte"
        } else {
            "typescript"
        };
    }
    match path.rsplit('.').next() {
        Some("tsx") => "tsx",
        Some("jsx") => "jsx",
        Some("ts" | "mts" | "cts") => "typescript",
        _ => "javascript",
    }
}
pub(crate) fn default_export(source: &str) -> Result<Option<(String, usize)>> {
    for pattern in [
        r"export\s+default\s+(?:async\s+)?function\s*\*?\s*([A-Za-z_$][\w$]*)",
        r"export\s+default\s+class\s+([A-Za-z_$][\w$]*)",
        r"export\s+default\s+(?:[A-Za-z_$][\w$]*\.)?[A-Za-z_$][\w$]*\s*\(\s*([A-Za-z_$][\w$]*)\s*[,)]",
        r"(?m)export\s+default\s+([A-Za-z_$][\w$]*)\s*;?\s*$",
        r"export\s*\{\s*([A-Za-z_$][\w$]*)\s+as\s+default\s*\}",
    ] {
        if let Some(found) = regex(pattern)?.captures(source)?
            && &found[1] != "default"
        {
            return Ok(Some((found[1].to_owned(), found.get(0).unwrap().start())));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    #[test]
    fn declarations_match_independent_reference() {
        let corpus: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/upstream/synthesis/frontend/cases.json"
        ))
        .unwrap();
        for case in corpus["cases"].as_array().unwrap() {
            let actual = extract(
                case["path"].as_str().unwrap(),
                case["source"].as_str().unwrap(),
                case["framework"].as_str().unwrap(),
            )
            .unwrap();
            let mut actual =
                json!({"nodes":actual.nodes,"references":actual.unresolved_references});
            for node in actual["nodes"].as_array_mut().unwrap() {
                node.as_object_mut().unwrap().remove("updatedAt");
            }
            assert_eq!(actual, case["expected"], "{}", case["id"]);
        }
    }
    #[test]
    fn navigation_matches_independent_reference_resolvers() {
        let corpus: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/upstream/synthesis/cases.json"
        ))
        .unwrap();
        for case in corpus["cases"].as_array().unwrap() {
            let mut nodes = case["nodes"].clone();
            for node in nodes.as_array_mut().unwrap() {
                node["updatedAt"] = json!(0);
            }
            let nodes: Vec<Node> = serde_json::from_value(nodes).unwrap();
            let active: Vec<String> =
                serde_json::from_value(case["activeNavigation"].clone()).unwrap();
            let sources = case["files"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(path, source)| (path.as_str(), source.as_str().unwrap()))
                .collect();
            let manifests = case
                .get("assets")
                .and_then(Value::as_object)
                .into_iter()
                .flatten()
                .map(|(path, source)| (path.as_str(), source.as_str().unwrap()))
                .collect();
            let module = |specifier: &str, from: &str, language: &str| {
                Ok(case
                    .get("modules")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .find(|row| {
                        row["specifier"] == specifier
                            && row["from"] == from
                            && row["language"] == language
                    })
                    .and_then(|row| row["result"].as_str().map(str::to_owned)))
            };
            let resolver = navigation::Navigation::with_context(
                &nodes, &active, &sources, &module, &manifests,
            )
            .unwrap();
            for call in case["navigation"].as_array().unwrap() {
                let reference: prometeu_extract::UnresolvedReference =
                    serde_json::from_value(call["reference"].clone()).unwrap();
                let source = case["files"][reference.file_path.as_ref().unwrap()]
                    .as_str()
                    .unwrap();
                let start = nodes
                    .iter()
                    .find(|n| {
                        n.id == reference.from_node_id
                            && Some(&n.file_path) == reference.file_path.as_ref()
                    })
                    .map(|n| n.start_line);
                let actual=resolver.resolve(&reference,&href::lines(source),start).unwrap().into_iter().map(|destination|json!({"target":destination.node.id,"metadata":destination.metadata})).collect::<Vec<_>>();
                assert_eq!(
                    json!(actual),
                    call["expected"],
                    "{} {}:{} {}",
                    case["id"],
                    reference.file_path.as_deref().unwrap(),
                    reference.line,
                    reference.reference_name
                );
            }
        }
    }
}
