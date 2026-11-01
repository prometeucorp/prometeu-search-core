use super::*;
use serde_json::{Value, json};
#[test]
fn persisted_vapor_handlers_and_conventions_match_reference_edges() {
    let corpus: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/upstream/frameworks/vapor.json"
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
        let answers = corpus["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|answer| answer["origin"] == case["origin"])
            .unwrap();
        let selected: HashSet<(String, String)> = answers["references"]
            .as_array()
            .unwrap()
            .iter()
            .zip(answers["expected"].as_array().unwrap())
            .filter(|(r, expected)| {
                expected.is_object()
                    || r["referenceName"]
                        .as_str()
                        .is_some_and(|name| name.contains('@') || name == "AccountController")
            })
            .map(|(r, _)| {
                (
                    r["fromNodeId"].as_str().unwrap().to_owned(),
                    r["referenceName"].as_str().unwrap().to_owned(),
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
            "/tmp/prometeu-vapor-project-differences.json",
            serde_json::to_string_pretty(&differences).unwrap(),
        )
        .unwrap();
    }
    assert!(
        differences.is_empty(),
        "{} Vapor projects differ; see /tmp/prometeu-vapor-project-differences.json",
        differences.len()
    );
}
