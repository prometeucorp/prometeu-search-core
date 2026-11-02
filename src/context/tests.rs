use super::*;
use serde_json::{Value, json};
fn canonical(mut value: Value) -> Value {
    fn visit(value: &mut Value) {
        match value {
            Value::Object(map) => {
                map.remove("updatedAt");
                for key in ["visibility", "provenance"] {
                    if map.get(key).is_some_and(Value::is_null) {
                        map.remove(key);
                    }
                }
                if map.get("confidence").is_some_and(|v| v == "") {
                    map.remove("confidence");
                }
                for value in map.values_mut() {
                    visit(value);
                }
            }
            Value::Array(values) => {
                for value in values {
                    visit(value);
                }
            }
            _ => {}
        }
    }
    visit(&mut value);
    value
}
#[test]
fn independent_context_builder_and_formatters() {
    let corpus: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/upstream/context/cases.json"
    ))
    .unwrap();
    for case in corpus["symbols"].as_array().unwrap() {
        assert_eq!(
            json!(extract_symbols_from_query(case["query"].as_str().unwrap()).unwrap()),
            case["expected"],
            "symbols: {}",
            case["query"]
        );
    }
    for case in corpus["bytes"].as_array().unwrap() {
        assert_eq!(
            format_bytes(case["value"].as_u64().unwrap() as usize),
            case["expected"].as_str().unwrap()
        );
    }
    for case in corpus["hops"].as_array().unwrap() {
        let edge: Edge = serde_json::from_value(case["edge"].clone()).unwrap();
        assert_eq!(
            json!(formatter::hop(&edge)),
            case["expected"],
            "hop {}",
            case["edge"]
        );
    }
    let mut filter_deviations = 0;
    for project in corpus["projects"].as_array().unwrap() {
        let mut node_values = project["nodes"].clone();
        for node in node_values.as_array_mut().unwrap() {
            node["updatedAt"] = json!(0);
        }
        let nodes: Vec<Node> = serde_json::from_value(node_values).unwrap();
        let edges: Vec<Edge> = serde_json::from_value(project["edges"].clone()).unwrap();
        let sources = project["files"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(path, source)| (path.as_str(), source.as_str().unwrap()))
            .collect();
        let search = |request: &ContextQuery| -> Result<Vec<ContextHit>> {
            let request = serde_json::to_value(request)?;
            let answer = project["answers"]
                .as_array()
                .unwrap()
                .iter()
                .find(|answer| answer["request"] == request)
                .unwrap_or_else(|| panic!("missing oracle {}: {request}", project["id"]));
            Ok(serde_json::from_value(answer["hits"].clone())?)
        };
        let generated = |path: &str| {
            project["generated"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v == path)
        };
        let deprioritized = |path: &str| {
            project["deprioritized"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v == path)
        };
        let builder = ContextBuilder::new(
            &nodes,
            &edges,
            &sources,
            &search,
            &deprioritized,
            &generated,
        )
        .unwrap();
        for case in project["cases"].as_array().unwrap() {
            let query = case["query"].as_str().unwrap();
            if let Some(options) = case.get("options") {
                let options: FindContextOptions = serde_json::from_value(options.clone()).unwrap();
                let actual = builder.find(query, &options).unwrap();
                let mut expected = case["expected"].clone();
                for node in expected["nodes"].as_array_mut().unwrap() {
                    node["updatedAt"] = json!(0);
                }
                let mut expected: ContextSubgraph = serde_json::from_value(expected).unwrap();
                // Preserve the captured answers, correcting only records excluded by
                // explicit host filters, which the pinned reader did not enforce.
                expected.nodes.retain(|node| {
                    options
                        .node_kinds
                        .as_ref()
                        .is_none_or(|kinds| kinds.is_empty() || kinds.contains(&node.kind))
                });
                let ids: HashSet<_> = expected.nodes.iter().map(|node| node.id.as_str()).collect();
                expected.roots.retain(|id| ids.contains(id.as_str()));
                expected.edges.retain(|edge| {
                    (options.edge_kinds.is_empty() || options.edge_kinds.contains(&edge.kind))
                        && ids.contains(edge.source.as_str())
                        && ids.contains(edge.target.as_str())
                });
                if expected.nodes.is_empty() && !expected.confidence.is_empty() {
                    expected.confidence = "high".into();
                }
                let expected = canonical(json!(expected));
                if expected != canonical(case["expected"].clone()) {
                    filter_deviations += 1;
                }
                assert_eq!(
                    canonical(json!(actual)),
                    expected,
                    "find {} {query:?} {}",
                    project["id"],
                    case["options"]
                );
            } else {
                let options: BuildContextOptions =
                    serde_json::from_value(case["buildOptions"].clone()).unwrap();
                let context = builder
                    .build(&TaskInput::Query(query.to_string()), &options)
                    .unwrap();
                assert_eq!(
                    canonical(json!(context)),
                    canonical(case["context"].clone()),
                    "build {} {query:?}",
                    project["id"]
                );
                assert_eq!(
                    builder.markdown(&context),
                    case["markdown"].as_str().unwrap(),
                    "markdown {} {query:?}",
                    project["id"]
                );
                assert_eq!(
                    canonical(
                        serde_json::from_str(&format_context_as_json(&context).unwrap()).unwrap()
                    ),
                    canonical(case["json"].clone()),
                    "JSON {} {query:?}",
                    project["id"]
                );
                assert_eq!(
                    format_subgraph_tree(&context.subgraph, &context.entry_points),
                    case["tree"].as_str().unwrap(),
                    "tree {} {query:?}",
                    project["id"]
                );
            }
        }
    }
    assert_eq!(filter_deviations, 19);
}
#[test]
fn independent_context_sql_candidates() {
    let corpus: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/upstream/context/cases.json"
    ))
    .unwrap();
    for project in corpus["projects"].as_array().unwrap() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("project");
        std::fs::create_dir(&root).unwrap();
        for (path, source) in project["files"].as_object().unwrap() {
            let path = root.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, source.as_str().unwrap()).unwrap();
        }
        let mut engine =
            crate::Engine::open(&root, Some(&temporary.path().join("index.sqlite"))).unwrap();
        engine.sync().unwrap();
        for answer in project["answers"].as_array().unwrap() {
            let request: ContextQuery = serde_json::from_value(answer["request"].clone()).unwrap();
            let actual = query::search(&engine.connection, &root, &request).unwrap();
            let expected: Vec<ContextHit> = serde_json::from_value(answer["hits"].clone()).unwrap();
            assert_eq!(
                actual.iter().map(|h| &h.id).collect::<Vec<_>>(),
                expected.iter().map(|h| &h.id).collect::<Vec<_>>(),
                "candidate order {} {}",
                project["id"],
                answer["request"]
            );
            for (a, b) in actual.iter().zip(expected) {
                assert!(
                    (a.score - b.score).abs() < 1e-10,
                    "candidate score {} {}: {} != {}",
                    project["id"],
                    answer["request"],
                    a.score,
                    b.score
                );
            }
        }
    }
}
#[test]
fn indexed_context_matches_reference_after_reopen() {
    let corpus: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/upstream/context/cases.json"
    ))
    .unwrap();
    let mut differences = Vec::new();
    for project in corpus["projects"].as_array().unwrap() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let database = temporary.path().join("index.sqlite");
        for (path, source) in project["files"].as_object().unwrap() {
            let path = root.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, source.as_str().unwrap()).unwrap();
        }
        let mut engine = crate::Engine::open(&root, Some(&database)).unwrap();
        engine.sync().unwrap();
        for reopen in [false, true] {
            if reopen {
                drop(engine);
                for path in project["files"].as_object().unwrap().keys() {
                    std::fs::write(root.join(path), "changed outside index").unwrap();
                }
                engine = crate::Engine::open(&root, Some(&database)).unwrap();
            }
            for case in project["cases"].as_array().unwrap().iter().filter(|c| {
                c.get("buildOptions").is_some()
                    && c["query"].as_str().is_some_and(|q| !q.trim().is_empty())
            }) {
                let options: BuildContextOptions =
                    serde_json::from_value(case["buildOptions"].clone()).unwrap();
                let result = engine
                    .context(
                        &TaskInput::Query(case["query"].as_str().unwrap().to_string()),
                        &options,
                        65536,
                    )
                    .unwrap();
                assert!(
                    !result.truncated,
                    "unexpected budget clipping {} {}",
                    project["id"], case["query"]
                );
                let actual: Value =
                    serde_json::from_str(&format_context_as_json(&result.context).unwrap())
                        .unwrap();
                let actual = canonical(actual);
                let expected = canonical(case["json"].clone());
                let actual_full = canonical(json!(result.context));
                let expected_full = canonical(case["context"].clone());
                if actual != expected
                    || actual_full != expected_full
                    || result.markdown() != case["markdown"].as_str().unwrap()
                {
                    differences.push(json!({"project":project["id"],"query":case["query"],"reopen":reopen,"actual":actual,"expected":expected,"actualFull":actual_full,"expectedFull":expected_full,"actualMarkdown":result.markdown(),"expectedMarkdown":case["markdown"]}));
                }
            }
        }
    }
    if !differences.is_empty() {
        let path = std::env::temp_dir().join("prometeu-context-index-diffs.json");
        std::fs::write(&path, serde_json::to_vec_pretty(&differences).unwrap()).unwrap();
        panic!(
            "{} context scenarios differ; records in {}",
            differences.len(),
            path.display()
        );
    }
}
