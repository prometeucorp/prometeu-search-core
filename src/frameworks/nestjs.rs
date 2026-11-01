//! Decorator routes and application prefixes for NestJS transports.
use super::extractors::regex;
use anyhow::Result;
use prometeu_extract::{ExtractionResult, Node, supplemental::Context};
use std::collections::BTreeMap;
mod bootstrap;
mod scan;
pub(super) use bootstrap::post_extract;
use scan::{Hit, Scan, Scope, trim};
const HTTP: &str = "Get|Post|Put|Patch|Delete|Head|Options|All";
fn language(path: &str) -> &'static str {
    if path.ends_with(".ts") || path.ends_with(".tsx") {
        "typescript"
    } else {
        "javascript"
    }
}
fn string(pattern: &str, source: &str) -> Result<String> {
    Ok(regex(pattern)?
        .captures(source)?
        .map(|hit| hit[1].into())
        .unwrap_or_default())
}
fn string_arg(args: &str) -> Result<String> {
    string(r#"['"`]([^'"`]*)['"`]"#, args)
}
fn join(prefix: &str, path: &str) -> String {
    format!(
        "/{}",
        [prefix, path]
            .iter()
            .map(|part| trim(part).trim_matches('/'))
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("/")
    )
}

pub(super) fn extract(path: &str, source: &str) -> Result<ExtractionResult> {
    let ctx = Context::new(path, source, language(path));
    let mut out = ctx.result.clone();
    out.nodes.clear();
    if !regex(r"\.(m?js|tsx?|cjs)$")?.is_match(path)? {
        return Ok(out);
    }
    let safe = crate::synthesis::comments::strip_for_regex(source, language(path));
    let scan = Scan::default();
    let scopes = scan.scopes(&safe)?;
    for (names, kind) in [
        (HTTP, "http"),
        ("Query|Mutation|Subscription", "graphql"),
        ("MessagePattern|EventPattern", "micro"),
        ("SubscribeMessage", "websocket"),
    ] {
        for hit in scan.decorators(&safe, names)? {
            let scope = scopes
                .iter()
                .find(|scope| scope.start <= hit.start && hit.start < scope.end);
            if kind == "graphql" && !scope.is_some_and(|scope| scope.kind == "resolver") {
                continue;
            }
            let handler = scan.name_after(&safe, hit.end, false)?;
            let (method, path) = match kind {
                "http" => (
                    hit.name.to_uppercase(),
                    join(
                        scope
                            .filter(|scope| scope.kind == "controller")
                            .map_or("", |scope| scope.prefix.as_str()),
                        &string_arg(hit.args)?,
                    ),
                ),
                "graphql" => {
                    let name = regex(r#"name\s*:\s*['"`]([^'"`]*)['"`]|^\s*['"`]([^'"`]*)['"`]"#)?
                        .captures(hit.args)?
                        .map(|found| {
                            found
                                .get(1)
                                .or_else(|| found.get(2))
                                .unwrap()
                                .as_str()
                                .to_owned()
                        })
                        .unwrap_or_else(|| handler.clone().unwrap_or_default());
                    (hit.name.to_uppercase(), name)
                }
                "micro" => {
                    let name = string_arg(hit.args)?;
                    (
                        if hit.name == "EventPattern" {
                            "EVENT"
                        } else {
                            "MESSAGE"
                        }
                        .into(),
                        if name.is_empty() {
                            handler.clone().unwrap_or_default()
                        } else {
                            name
                        },
                    )
                }
                _ => {
                    let name = string_arg(hit.args)?;
                    let name = if name.is_empty() {
                        handler.clone().unwrap_or_default()
                    } else {
                        name
                    };
                    let namespace = scope
                        .filter(|scope| scope.kind == "gateway")
                        .map_or("", |scope| scope.prefix.as_str());
                    (
                        "WS".into(),
                        if namespace.is_empty() {
                            name
                        } else {
                            format!("{namespace}:{name}")
                        },
                    )
                }
            };
            let mut node = super::routes::route(
                &ctx,
                ctx.position(hit.start).0,
                &method,
                &path,
                source[hit.start..hit.end].encode_utf16().count() as u32,
            );
            node.name = format!("{method} {path}");
            if let Some(handler) = handler {
                out.unresolved_references.push(super::routes::reference(
                    &node,
                    &handler,
                    "references",
                ));
            }
            out.nodes.push(node);
        }
    }
    Ok(out)
}

pub(crate) fn resolve<'a>(
    path: &str,
    name: &str,
    named: impl Fn(&str) -> Vec<&'a Node>,
) -> Result<Option<(&'a Node, f64)>> {
    for (suffix, convention) in [
        ("Service", ".service."),
        ("Controller", ".controller."),
        ("Resolver", ".resolver."),
        ("Gateway", ".gateway."),
        ("Repository", ".repository."),
        ("Guard", ".guard."),
        ("Interceptor", ".interceptor."),
        ("Pipe", ".pipe."),
        ("Module", ".module."),
    ] {
        if !name.ends_with(suffix) {
            continue;
        }
        let nodes = named(name)
            .into_iter()
            .filter(|node| node.kind == "class")
            .collect::<Vec<_>>();
        if let Some(node) = nodes.iter().find(|node| node.file_path == path) {
            return Ok(Some((*node, 0.85)));
        }
        if regex(r"\.(?:m?[jt]sx?|cjs|cts|mts)$")?.is_match(path)? {
            let directory = path.rfind('/').map_or("", |at| &path[..at + 1]);
            return Ok(nodes
                .into_iter()
                .find(|node| {
                    node.file_path.contains(convention)
                        && node
                            .file_path
                            .strip_prefix(directory)
                            .is_some_and(|tail| !tail.contains('/'))
                })
                .map(|node| (node, 0.85)));
        }
        return Ok(nodes
            .iter()
            .find(|node| node.file_path.contains(convention))
            .map(|node| (*node, 0.85))
            .or_else(|| nodes.first().map(|node| (*node, 0.7))));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    fn stripped(nodes: Vec<Node>) -> Value {
        let mut value = json!(nodes);
        for node in value.as_array_mut().unwrap() {
            node.as_object_mut().unwrap().remove("updatedAt");
        }
        value
    }
    #[test]
    fn records_finalization_and_bindings_match_independent_reference() {
        let corpus: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/upstream/nestjs/cases.json"
        ))
        .unwrap();
        for case in corpus["cases"].as_array().unwrap() {
            let actual = extract(
                case["path"].as_str().unwrap(),
                case["source"].as_str().unwrap(),
            )
            .unwrap();
            assert_eq!(
                json!({"nodes":stripped(actual.nodes),"references":actual.unresolved_references}),
                case["expected"],
                "{} {}",
                case["path"],
                case["source"]
            );
        }
        for case in corpus["post"].as_array().unwrap() {
            let mut nodes = case["nodes"].clone();
            for node in nodes.as_array_mut().unwrap() {
                node["updatedAt"] = json!(0);
            }
            let nodes: Vec<Node> = serde_json::from_value(nodes).unwrap();
            let files = serde_json::from_value(case["files"].clone()).unwrap();
            let actual = post_extract(&nodes, &files).unwrap();
            let mut actual = stripped(actual).as_array().unwrap().clone();
            let mut expected = case["expected"].as_array().unwrap().clone();
            actual.sort_by_key(|node| node["id"].as_str().unwrap().to_owned());
            expected.sort_by_key(|node| node["id"].as_str().unwrap().to_owned());
            assert_eq!(actual, expected, "{}", case["files"]);
        }
        let nodes: Vec<Node> = serde_json::from_value(corpus["resolver_nodes"].clone()).unwrap();
        for case in corpus["resolutions"].as_array().unwrap() {
            let reference = &case["reference"];
            let result = resolve(
                reference["filePath"].as_str().unwrap(),
                reference["referenceName"].as_str().unwrap(),
                |name| nodes.iter().filter(|node| node.name == name).collect(),
            )
            .unwrap();
            let actual = result
                .map(|(node, confidence)| json!({"id":node.id,"confidence":confidence}))
                .unwrap_or(Value::Null);
            assert_eq!(actual, case["expected"], "{reference}");
        }
    }
}
