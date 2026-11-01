use prometeu_extract::{Edge, Node};
use prometeu_search::graph_query::{Direction, GraphSnapshot, TraversalOptions};
use serde_json::{Value, json};
use std::collections::HashMap;

fn edge_key(edge: &Value) -> String {
    json!([
        edge["source"],
        edge["target"],
        edge["kind"],
        edge["line"],
        edge["column"]
    ])
    .to_string()
}

fn compact(value: &mut Value, edges: &HashMap<String, usize>) {
    if value.get("filePath").is_some() && value.get("kind").is_some() {
        *value = json!({"$node":value["id"]});
    } else if value.get("source").is_some() && value.get("target").is_some() {
        *value = json!({"$edge":edges[&edge_key(value)]});
    } else {
        match value {
            Value::Array(values) => values.iter_mut().for_each(|value| compact(value, edges)),
            Value::Object(values) => values.values_mut().for_each(|value| compact(value, edges)),
            _ => {}
        }
    }
}

#[test]
fn graph_operations_match_independent_sqlite_queries_and_traversals() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/upstream/graph-queries/cases.json")).unwrap();
    let mut count = 0;
    let mut filter_deviations = 0;
    for case in fixture["graphs"].as_array().unwrap() {
        let nodes: Vec<Node> = serde_json::from_value(case["nodes"].clone()).unwrap();
        let edges: Vec<Edge> = serde_json::from_value(case["edges"].clone()).unwrap();
        let edge_ids = case["edges"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .map(|(i, e)| (edge_key(e), i))
            .collect();
        let snapshot = GraphSnapshot::new(&nodes, &edges).unwrap();
        for query in case["cases"].as_array().unwrap() {
            let operation = query["operation"].as_str().unwrap();
            let id = query["id"].as_str().unwrap();
            let opts = &query["options"];
            let depth = opts["depth"].as_u64().unwrap_or(2) as usize;
            let kinds: Vec<String> =
                serde_json::from_value(opts.get("edgeKinds").cloned().unwrap_or(json!([])))
                    .unwrap();
            let traversal = TraversalOptions {
                max_depth: opts["maxDepth"].as_u64().unwrap_or(1024) as usize,
                direction: match opts["direction"].as_str() {
                    Some("incoming") => Direction::Incoming,
                    Some("both") => Direction::Both,
                    _ => Direction::Outgoing,
                },
                include_start: opts["includeStart"].as_bool().unwrap_or(true),
                limit: opts["limit"].as_u64().unwrap_or(1000) as usize,
                edge_kinds: kinds.clone(),
                node_kinds: serde_json::from_value(
                    opts.get("nodeKinds").cloned().unwrap_or(json!([])),
                )
                .unwrap(),
            };
            let mut actual = match operation {
                "bfs" => json!(snapshot.breadth_first(id, &traversal).unwrap()),
                "dfs" => json!(snapshot.depth_first(id, &traversal).unwrap()),
                "callers" => json!(snapshot.calls(id, true, depth).unwrap()),
                "callees" => json!(snapshot.calls(id, false, depth).unwrap()),
                "call_sites" => json!(snapshot.call_sites(id, opts["direction"] == "callers")),
                "call_graph" => json!(snapshot.call_graph(id, depth).unwrap()),
                "usages" => json!(snapshot.usages(id)),
                "ancestors" => json!(snapshot.ancestors(id)),
                "children" => json!(snapshot.children(id)),
                "type_graph" => json!(snapshot.type_graph(id)),
                "path" => json!(snapshot.find_path(id, opts["target"].as_str().unwrap(), &kinds)),
                "impact" => json!(
                    snapshot
                        .impact(
                            id,
                            depth,
                            opts["maxNodes"].as_u64().unwrap_or(10000) as usize,
                            opts["maxEdges"].as_u64().unwrap_or(50000) as usize
                        )
                        .unwrap()
                ),
                "symbol_context" => json!(snapshot.symbol_context(id)),
                "dependencies" | "dependents" => {
                    let mut paths = snapshot.file_dependencies(id, operation == "dependents");
                    paths.sort();
                    json!(paths)
                }
                "exports" => json!(snapshot.exports(id)),
                "qualified" => json!(snapshot.qualified_names(id).unwrap()),
                "filtered" => json!(snapshot.filtered(
                    |n| n.kind != "file",
                    opts["includeEdges"].as_bool().unwrap()
                )),
                _ => panic!("unexpected operation {operation}"),
            };
            compact(&mut actual, &edge_ids);
            // Public bounds are explicit even where the reference omits this flag.
            if query["expected"].get("truncated").is_none()
                && let Some(object) = actual.as_object_mut()
            {
                object.remove("truncated");
            }
            let mut expected = query["expected"].clone();
            if operation == "filtered" {
                // The pinned reader omits newer extraction kinds. Preserve its recorded
                // ordering, then include those supplied nodes and their incident edges.
                let known_kinds = [
                    "file",
                    "module",
                    "class",
                    "struct",
                    "union",
                    "interface",
                    "trait",
                    "function",
                    "method",
                    "variable",
                    "constant",
                    "enum",
                    "type_alias",
                ];
                let additional = nodes
                    .iter()
                    .filter(|node| !known_kinds.contains(&node.kind.as_str()))
                    .collect::<Vec<_>>();
                if !additional.is_empty() {
                    filter_deviations += 1;
                    expected["nodes"]
                        .as_array_mut()
                        .unwrap()
                        .extend(additional.iter().map(|node| json!({"$node":node.id})));
                    if opts["includeEdges"] == true {
                        let ids = expected["nodes"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .map(|node| node["$node"].as_str().unwrap())
                            .collect::<Vec<_>>();
                        let mut retained_edges = Vec::new();
                        for id in &ids {
                            let mut adjacent = edges
                                .iter()
                                .filter(|edge| {
                                    edge.source == *id && ids.contains(&edge.target.as_str())
                                })
                                .collect::<Vec<_>>();
                            adjacent.sort_by_key(|edge| {
                                (&edge.kind, &edge.target, edge.line, edge.column)
                            });
                            retained_edges.extend(
                                adjacent
                                    .into_iter()
                                    .map(|edge| json!({"$edge":edge_ids[&edge_key(&json!(edge))]})),
                            );
                        }
                        expected["edges"] = json!(retained_edges);
                    }
                }
            }
            assert_eq!(actual, expected, "{} {operation} {id} {opts}", case["id"]);
            count += 1;
        }
    }
    assert_eq!(filter_deviations, 32);
    assert!(
        count > 20000,
        "oracle coverage unexpectedly shrank: {count}"
    );
}

#[test]
fn depth_first_marks_a_limit_reached_on_the_last_pending_branch() {
    let nodes: Vec<Node> = ["a", "b", "c"].into_iter().map(|id| serde_json::from_value(json!({
        "id":id,"name":id,"qualifiedName":id,"kind":"function","filePath":"main.rs","language":"rust",
        "startLine":1,"endLine":1,"startColumn":0,"endColumn":1,"updatedAt":0
    })).unwrap()).collect();
    let edges: Vec<Edge> = [("a", "b"), ("b", "c")]
        .into_iter()
        .map(|(source, target)| {
            serde_json::from_value(json!({"source":source,"target":target,"kind":"calls"})).unwrap()
        })
        .collect();
    let graph = GraphSnapshot::new(&nodes, &edges).unwrap();
    let result = graph
        .depth_first(
            "a",
            &TraversalOptions {
                limit: 2,
                max_depth: 3,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(
        result
            .nodes
            .iter()
            .map(|node| node.id.as_str())
            .collect::<Vec<_>>(),
        ["a", "b"]
    );
    assert!(result.truncated);
}

#[test]
fn large_snapshots_and_explicit_traversal_budgets_are_not_artificially_capped() {
    let template: Node = serde_json::from_value(json!({
        "id":"n0","name":"run","qualifiedName":"run","kind":"function",
        "filePath":"main.rs","language":"rust","startLine":1,"endLine":1,
        "startColumn":0,"endColumn":1,"updatedAt":0
    }))
    .unwrap();
    let nodes = (0..100_001)
        .map(|index| Node {
            id: format!("n{index}"),
            ..template.clone()
        })
        .collect::<Vec<_>>();
    let template: Edge = serde_json::from_value(json!({
        "source":"n0","target":"n0","kind":"calls"
    }))
    .unwrap();
    let mut edges = Vec::with_capacity(200_001);
    for index in 1..nodes.len() {
        for line in [1, 2] {
            edges.push(Edge {
                source: nodes[index - 1].id.clone(),
                target: nodes[index].id.clone(),
                line: Some(line),
                ..template.clone()
            });
        }
    }
    edges.push(template);
    let graph = GraphSnapshot::new(&nodes, &edges).unwrap();
    let defaults = graph
        .breadth_first("n0", &TraversalOptions::default())
        .unwrap();
    assert_eq!(defaults.nodes.len(), 1000);
    let expanded = graph
        .breadth_first(
            "n0",
            &TraversalOptions {
                limit: 11_001,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(expanded.nodes.last().unwrap().id, "n11000");
    assert_eq!(graph.calls("n0", false, 11_000).unwrap().len(), 11_000);
    let impact = graph
        .impact("n30000", usize::MAX, 30_001, usize::MAX)
        .unwrap();
    assert_eq!(impact.nodes.len(), 30_001);
    assert!(impact.edges.len() > 50_000);
    assert!(!impact.truncated);
}
