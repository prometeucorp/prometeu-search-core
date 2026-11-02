use super::*;

#[test]
fn comment_masks_match_reference_and_preserve_source_bytes() {
    let corpus: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/upstream/synthesis/comments.json"
    ))
    .unwrap();
    for case in corpus["cases"].as_array().unwrap() {
        let source = case["source"].as_str().unwrap();
        let actual = comments::strip_for_regex(source, case["language"].as_str().unwrap());
        assert_eq!(
            actual,
            case["byteExpected"].as_str().unwrap(),
            "{}",
            case["id"]
        );
        assert_eq!(actual.len(), source.len(), "{}", case["id"]);
    }
}

#[test]
fn string_masks_match_reference_regex_and_escape_handling() {
    let corpus: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/upstream/synthesis/strings.json"
    ))
    .unwrap();
    for case in corpus["cases"].as_array().unwrap() {
        let source = case["source"].as_str().unwrap();
        assert_eq!(
            comments::blank_string_contents(source).unwrap(),
            case["byteExpected"].as_str().unwrap(),
            "{}",
            case["id"]
        );
    }
}

#[test]
fn empty_graph_has_no_synthesis() {
    assert!(
        synthesize(&[], &[], &BTreeMap::new(), &[])
            .unwrap()
            .is_empty()
    );
}

#[test]
fn canonical_edges_match_independent_reference_pipeline() {
    let corpus: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/upstream/synthesis/cases.json"
    ))
    .unwrap();
    for case in corpus["cases"].as_array().unwrap() {
        let nodes: Vec<Node> = case["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|node| {
                let mut node = node.clone();
                node["updatedAt"] = json!(0);
                serde_json::from_value(node).unwrap()
            })
            .collect();
        let base_edges: Vec<Edge> = serde_json::from_value(case["baseEdges"].clone()).unwrap();
        let sources: BTreeMap<_, _> = case["files"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(path, source)| (path.as_str(), source.as_str().unwrap()))
            .collect();
        let unresolved: Vec<UnresolvedReference> =
            serde_json::from_value(case.get("unresolved").cloned().unwrap_or(json!([]))).unwrap();
        let workspace_dirs: Vec<String> =
            serde_json::from_value(case.get("workspaceDirs").cloned().unwrap_or(json!([])))
                .unwrap();
        let imports: Vec<ImportBinding> =
            serde_json::from_value(case.get("imports").cloned().unwrap_or(json!([]))).unwrap();
        let manifests = case
            .get("assets")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
            .map(|(path, source)| (path.as_str(), source.as_str().unwrap()))
            .collect();
        let frameworks: Vec<String> =
            serde_json::from_value(case.get("activeFrameworks").cloned().unwrap_or(json!([])))
                .unwrap();
        let actual = synthesize_with_imports(
            &nodes,
            &base_edges,
            &sources,
            &unresolved,
            &workspace_dirs,
            &SynthesisEnvironment {
                imports: &imports,
                manifests: &manifests,
                auxiliary_sources: &BTreeMap::new(),
                frameworks: &frameworks,
                resolve_import: &|reference| {
                    Ok(case
                        .get("importAnswers")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .find(|row| {
                            row["reference"]["fromNodeId"] == reference.from_node_id
                                && row["reference"]["referenceName"] == reference.reference_name
                                && row["reference"]["line"] == reference.line
                        })
                        .and_then(|row| row["result"].as_str().map(str::to_owned)))
                },
                go_package_dir: &|_, _| None,
                resolve_module: &|specifier, from, language| {
                    case.get("modules")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .find(|row| {
                            row["specifier"] == specifier
                                && row["from"] == from
                                && row["language"] == language
                        })
                        .and_then(|row| row["result"].as_str().map(str::to_owned))
                },
            },
        )
        .unwrap();
        let expected: Vec<Edge> = serde_json::from_value(case["edges"].clone()).unwrap();
        let expected: Vec<_> = expected
            .into_iter()
            .filter(|edge| {
                edge.metadata
                    .as_ref()
                    .is_some_and(|m| m.get("synthesizedBy").is_some())
            })
            .collect();
        let normalize = |edges: Vec<Edge>| {
            let mut records: Vec<_> = edges
                .into_iter()
                .map(|edge| {
                    let mut value = serde_json::to_value(edge).unwrap();
                    value.sort_all_objects();
                    value.to_string()
                })
                .collect();
            records.sort();
            records
        };
        assert_eq!(normalize(actual), normalize(expected), "{}", case["id"]);
    }
}

#[test]
fn go_prerequisites_and_promotions_match_reference_pipeline() {
    let corpus: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/upstream/synthesis/go/cases.json"
    ))
    .unwrap();
    for case in corpus["cases"].as_array().unwrap() {
        let mut nodes = case["nodes"].clone();
        for node in nodes.as_array_mut().unwrap() {
            node["updatedAt"] = json!(0);
        }
        let nodes: Vec<Node> = serde_json::from_value(nodes).unwrap();
        let edges: Vec<Edge> = serde_json::from_value(case["rawBaseEdges"].clone()).unwrap();
        let unresolved: Vec<UnresolvedReference> =
            serde_json::from_value(case["unresolved"].clone()).unwrap();
        let sources = case["files"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(path, source)| (path.as_str(), source.as_str().unwrap()))
            .collect();
        let actual = synthesize(&nodes, &edges, &sources, &unresolved).unwrap();
        let expected: Vec<Edge> = serde_json::from_value(case["synthesized"].clone()).unwrap();
        let normalize = |edges: Vec<Edge>| {
            let mut records: Vec<_> = edges
                .into_iter()
                .map(|edge| {
                    let mut value = serde_json::to_value(edge).unwrap();
                    value.sort_all_objects();
                    value.to_string()
                })
                .collect();
            records.sort();
            records
        };
        let actual = normalize(actual);
        let expected = normalize(expected);
        assert_eq!(actual, expected, "{}", case["id"]);
    }
}
