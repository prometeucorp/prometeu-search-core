use serde_json::{Value, json};
#[test]
fn indexed_workspace_bindings_match_complete_reference_edges() {
    let corpus: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/upstream/frameworks/cargo-workspace.json"
    ))
    .unwrap();
    let mut differences = Vec::new();
    for case in corpus["projects"].as_array().unwrap() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let database = temporary.path().join("index.sqlite");
        for (path, source) in case["files"].as_object().unwrap() {
            let path = root.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, source.as_str().unwrap()).unwrap();
        }
        let mut engine = crate::Engine::open(&root, Some(&database)).unwrap();
        engine.sync().unwrap();
        drop(engine);
        let engine = crate::Engine::open(&root, Some(&database)).unwrap();
        let graph = super::super::Graph::load(&engine.connection, 1).unwrap();
        let relevant = |edge: &&prometeu_extract::Edge| {
            edge.metadata
                .as_ref()
                .and_then(|m| m["refName"].as_str())
                .is_some_and(|name| matches!(name, "helper" | "core_api"))
        };
        let mut actual: Vec<_> = graph
            .canonical_edges
            .iter()
            .flatten()
            .filter(relevant)
            .map(|e| serde_json::to_value(e).unwrap())
            .collect();
        let expected: Vec<prometeu_extract::Edge> =
            serde_json::from_value(case["allEdges"].clone()).unwrap();
        let mut expected: Vec<_> = expected
            .iter()
            .filter(relevant)
            .map(|e| serde_json::to_value(e).unwrap())
            .collect();
        actual.sort_by_key(Value::to_string);
        expected.sort_by_key(Value::to_string);
        if actual != expected {
            differences.push(json!({"id":case["id"],"actual":actual,"expected":expected}));
        }
    }
    if !differences.is_empty() {
        std::fs::write(
            "/tmp/prometeu-cargo-project-differences.json",
            serde_json::to_string_pretty(&differences).unwrap(),
        )
        .unwrap();
    }
    assert!(
        differences.is_empty(),
        "{} Cargo projects differ; see /tmp/prometeu-cargo-project-differences.json",
        differences.len()
    );
}
