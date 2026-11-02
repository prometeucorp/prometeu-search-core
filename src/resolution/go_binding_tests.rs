use super::*;
use serde_json::{Value, json};
#[test]
fn persisted_go_conventions_match_original_strategy_winners() {
    let corpus: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/upstream/frameworks/go-bindings.json"
    ))
    .unwrap();
    let mut differences = Vec::new();
    let named = |name: &str| {
        !name.is_empty()
            && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
            && (name.ends_with("Handler")
                || name.starts_with("Handle")
                || name.ends_with("Service")
                || name.ends_with("Repository")
                || name.ends_with("Store")
                || name.ends_with("Middleware")
                || name.starts_with("Auth")
                || name.starts_with("Log")
                || (name.len() > 1
                    && name.as_bytes()[0].is_ascii_uppercase()
                    && name.bytes().all(|b| b.is_ascii_alphabetic())))
    };
    for case in corpus["projects"].as_array().unwrap() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let database = temporary.path().join("index.sqlite");
        for (path, source) in case["sources"]
            .as_object()
            .unwrap()
            .iter()
            .chain(case["assets"].as_object().unwrap())
        {
            let path = root.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, source.as_str().unwrap()).unwrap();
        }
        let mut engine = crate::Engine::open(&root, Some(&database)).unwrap();
        engine.sync().unwrap();
        drop(engine);
        let engine = crate::Engine::open(&root, Some(&database)).unwrap();
        let graph = Graph::load(&engine.connection, 1).unwrap();
        let relevant = |edge: &&prometeu_extract::Edge| {
            edge.kind == "calls"
                || edge
                    .metadata
                    .as_ref()
                    .and_then(|m| m["refName"].as_str())
                    .is_some_and(named)
        };
        let mut actual: Vec<_> = graph
            .canonical_edges
            .iter()
            .flatten()
            .filter(relevant)
            .map(|edge| serde_json::to_value(edge).unwrap())
            .collect();
        let expected: Vec<prometeu_extract::Edge> =
            serde_json::from_value(case["allEdges"].clone()).unwrap();
        let mut expected: Vec<_> = expected
            .iter()
            .filter(relevant)
            .map(|edge| serde_json::to_value(edge).unwrap())
            .collect();
        actual.sort_by_key(Value::to_string);
        expected.sort_by_key(Value::to_string);
        if actual != expected {
            differences.push(json!({"origin":case["origin"],"actual":actual,"expected":expected}));
        }
    }
    if !differences.is_empty() {
        std::fs::write(
            "/tmp/prometeu-go-project-differences.json",
            serde_json::to_string_pretty(&differences).unwrap(),
        )
        .unwrap();
    }
    assert!(
        differences.is_empty(),
        "{} Go projects differ; see /tmp/prometeu-go-project-differences.json",
        differences.len()
    );
}
