use prometeu_extract::{Edge, Node};
use prometeu_search::graph_query::{Direction, GraphSnapshot, TraversalOptions};
use prometeu_search::{Engine, GraphOperation, GraphQueryOptions};
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
            assert_eq!(
                actual, query["expected"],
                "{} {operation} {id} {opts}",
                case["id"]
            );
            count += 1;
        }
    }
    assert!(
        count > 20000,
        "oracle coverage unexpectedly shrank: {count}"
    );
}

#[test]
fn graph_queries_keep_committed_sources_and_follow_dependency_lifecycle() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("go.mod"), "module fixture\n\ngo 1.22\n").unwrap();
    std::fs::write(
        root.join("worker.go"),
        "package fixture\nfunc Worker() {}\n",
    )
    .unwrap();
    std::fs::write(
        root.join("worker_test.go"),
        "package fixture\nfunc TestWorker() { Worker(); Worker() }\n",
    )
    .unwrap();
    let database = temp.path().join("cache.sqlite");
    let mut engine = Engine::open(&root, Some(&database)).unwrap();
    engine.sync().unwrap();
    let query = GraphQueryOptions {
        operation: GraphOperation::Dependents,
        path: Some("worker.go".into()),
        ..Default::default()
    };
    let before = engine.graph_query(&query).unwrap();
    assert_eq!(before.data, json!(["worker_test.go"]));
    let affected = engine
        .analyze(
            "worker.go",
            prometeu_search::GraphMode::Affected,
            &Default::default(),
        )
        .unwrap();
    assert_eq!(affected.affected_tests, ["worker_test.go"]);
    let id = engine
        .search("Worker", 50, 65536)
        .unwrap()
        .results
        .into_iter()
        .find(|n| n.name == "Worker")
        .unwrap()
        .id;
    let sites = engine
        .graph_query(&GraphQueryOptions {
            operation: GraphOperation::CallSites,
            id: Some(id),
            traversal: TraversalOptions {
                direction: Direction::Incoming,
                ..Default::default()
            },
            ..Default::default()
        })
        .unwrap();
    assert!(
        sites
            .data
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["node"]["name"] == "TestWorker"
                && row["edges"].as_array().unwrap().len() == 2)
    );
    drop(engine);
    std::fs::write(
        root.join("worker_test.go"),
        "package fixture\nfunc TestWorker() {}\n",
    )
    .unwrap();
    let mut engine = Engine::open(&root, Some(&database)).unwrap();
    assert_eq!(engine.graph_query(&query).unwrap().data, before.data);
    engine.sync().unwrap();
    assert_eq!(engine.graph_query(&query).unwrap().data, json!([]));
    std::fs::rename(root.join("worker_test.go"), root.join("renamed_test.go")).unwrap();
    std::fs::write(
        root.join("renamed_test.go"),
        "package fixture\nfunc TestWorker() { Worker() }\n",
    )
    .unwrap();
    engine.sync().unwrap();
    assert_eq!(
        engine.graph_query(&query).unwrap().data,
        json!(["renamed_test.go"])
    );
    std::fs::remove_file(root.join("renamed_test.go")).unwrap();
    engine.sync().unwrap();
    assert_eq!(engine.graph_query(&query).unwrap().data, json!([]));
    let bad = GraphQueryOptions {
        path: Some("../worker.go".into()),
        ..query
    };
    assert!(engine.graph_query(&bad).is_err());
}

#[test]
fn module_budgets_preserve_directory_names_and_bound_many_entries() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("project");
    for path in [
        "nodes/a.ts",
        "nodes/b.ts",
        "nodes/c.ts",
        "edges/one.ts",
        "roots/one.ts",
    ] {
        let file = root.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, "export function run() {}\n").unwrap();
    }
    let mut engine = Engine::open(&root, Some(&temporary.path().join("index.sqlite"))).unwrap();
    engine.sync().unwrap();
    let result = engine
        .graph_query(&GraphQueryOptions {
            operation: GraphOperation::Modules,
            traversal: TraversalOptions {
                limit: 2,
                ..Default::default()
            },
            ..Default::default()
        })
        .unwrap();
    assert!(result.truncated);
    assert_eq!(
        result.data,
        json!({"edges":["edges/one.ts"],"nodes":["nodes/a.ts","nodes/b.ts"]})
    );
    std::fs::create_dir_all(root.join("focal")).unwrap();
    for index in 0..5 {
        std::fs::write(
            root.join(format!("focal/{index}.ts")),
            "export function focus() {}\n",
        )
        .unwrap();
    }
    engine.sync().unwrap();
    let result = engine
        .graph_query(&GraphQueryOptions {
            operation: GraphOperation::Modules,
            traversal: TraversalOptions {
                limit: 4,
                ..Default::default()
            },
            ..Default::default()
        })
        .unwrap();
    assert!(result.truncated);
    assert_eq!(result.data["focal"].as_array().unwrap().len(), 4);
    for index in 0..80 {
        let directory = root.join(format!("module_{index:03}_{}", "long_name_".repeat(8)));
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("main.ts"), "export function start() {}\n").unwrap();
    }
    engine.sync().unwrap();
    let result = engine
        .graph_query(&GraphQueryOptions {
            operation: GraphOperation::Modules,
            max_bytes: 1024,
            ..Default::default()
        })
        .unwrap();
    assert!(result.truncated);
    assert!(serde_json::to_vec(&result).unwrap().len() <= 1024);
    let modules = result.data.as_object().unwrap();
    assert!(modules.len() < 83);
    for (directory, files) in modules {
        assert!(
            files
                .as_array()
                .unwrap()
                .iter()
                .all(|file| file.as_str().unwrap().starts_with(&format!("{directory}/")))
        );
    }
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

#[test]
fn operation_defaults_preserve_explicit_overrides_and_large_complete_results() {
    for (operation, depth, limit) in [
        ("bfs", usize::MAX, 1000),
        ("dfs", usize::MAX, 1000),
        ("callers", 1, usize::MAX),
        ("callees", 1, usize::MAX),
        ("call_graph", 2, usize::MAX),
        ("impact", 3, 10000),
        ("filtered", usize::MAX, usize::MAX),
    ] {
        let options: GraphQueryOptions = serde_json::from_value(json!({
            "operation":operation,"traversal":{"direction":"incoming"}
        }))
        .unwrap();
        assert_eq!(
            (options.traversal.max_depth, options.traversal.limit),
            (depth, limit),
            "{operation}"
        );
        assert_eq!(options.max_bytes, usize::MAX);
    }
    let options: GraphQueryOptions = serde_json::from_value(json!({
        "operation":"impact","id":1,"traversal":{"max_depth":2048,"limit":12000},
        "max_edges":60000,"max_bytes":100000
    }))
    .unwrap();
    options.validate().unwrap();
    assert_eq!(
        (options.traversal.max_depth, options.traversal.limit),
        (2048, 12000)
    );
    assert_eq!((options.max_edges, options.max_bytes), (60000, 100000));

    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("project");
    std::fs::create_dir(&root).unwrap();
    let mut source = (0..1100)
        .map(|index| format!("export function task{index}() {{}}\n"))
        .collect::<String>();
    source.push_str("export function launch() {\n");
    source.push_str(&"task0();\n".repeat(1200));
    source.push_str("}\n");
    std::fs::write(root.join("tasks.ts"), source).unwrap();
    let mut engine = Engine::open(&root, Some(&temporary.path().join("index.sqlite"))).unwrap();
    engine.sync().unwrap();
    let options = GraphQueryOptions::for_operation(GraphOperation::Filtered);
    let result = engine.graph_query(&options).unwrap();
    assert!(result.data["nodes"].as_array().unwrap().len() > 1000);
    assert!(serde_json::to_vec(&result).unwrap().len() > 65536);
    assert!(!result.truncated);
    let id = engine.search("launch", 10, 12000).unwrap().results[0].id;
    let calls = engine
        .graph_query(&GraphQueryOptions {
            id: Some(id),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(calls.data["nodes"].as_array().unwrap().len(), 2);
    assert_eq!(calls.data["edges"].as_array().unwrap().len(), 1200);
    assert!(!calls.truncated);
    let clipped = engine
        .graph_query(&GraphQueryOptions {
            max_bytes: 1024,
            ..options
        })
        .unwrap();
    assert!(clipped.truncated);
    assert!(serde_json::to_vec(&clipped).unwrap().len() <= 1024);
}
