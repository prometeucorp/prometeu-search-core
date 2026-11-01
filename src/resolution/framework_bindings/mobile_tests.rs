use super::*;
use serde_json::{Value, json};
#[test]
fn persisted_mobile_framework_edges_match_complete_reference_records() {
    let corpus: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/upstream/frameworks/mobile.json"
    ))
    .unwrap();
    let mut differences = Vec::new();
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
        let selected: HashSet<(String, String)> = corpus["cases"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|answer| {
                answer["origin"] == case["origin"] && answer["expected"][0].is_object()
            })
            .map(|answer| {
                (
                    answer["references"][0]["fromNodeId"]
                        .as_str()
                        .unwrap()
                        .to_owned(),
                    answer["references"][0]["referenceName"]
                        .as_str()
                        .unwrap()
                        .to_owned(),
                )
            })
            .collect();
        let relevant = |edge: &&prometeu_extract::Edge| {
            edge.metadata
                .as_ref()
                .and_then(|m| m["refName"].as_str())
                .is_some_and(|name| selected.contains(&(edge.source.clone(), name.to_owned())))
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
            differences.push(json!({"origin":case["origin"],"actual":actual,"expected":expected}));
        }
    }
    if !differences.is_empty() {
        std::fs::write(
            "/tmp/prometeu-mobile-differences.json",
            serde_json::to_string_pretty(&differences).unwrap(),
        )
        .unwrap();
    }
    assert!(
        differences.is_empty(),
        "{} project differences; see /tmp/prometeu-mobile-differences.json",
        differences.len()
    );
}
