use crate::Engine;
use serde_json::Value;

fn normalized(values: impl IntoIterator<Item = Value>) -> Vec<String> {
    let mut rows = values
        .into_iter()
        .map(|mut value| {
            value.sort_all_objects();
            value.to_string()
        })
        .collect::<Vec<_>>();
    rows.sort();
    rows
}

#[test]
fn python_complete_graph_edges_match_index_and_reopened_snapshot() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/upstream/resolution/python/cases.json"
    ))
    .unwrap();
    let mut differences = Vec::new();
    for project in fixture["projects"].as_array().unwrap() {
        let expected_edges: Vec<prometeu_extract::Edge> =
            serde_json::from_value(project["edges"].clone()).unwrap();
        let expected = normalized(
            expected_edges
                .iter()
                .map(|edge| serde_json::to_value(edge).unwrap()),
        );
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("project");
        let database = temporary.path().join("index.sqlite");
        for (path, source) in project["files"].as_object().unwrap() {
            let path = root.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, source.as_str().unwrap()).unwrap();
        }
        let mut engine = Engine::open(&root, Some(&database)).unwrap();
        engine.sync().unwrap();
        for reopened in [false, true] {
            if reopened {
                drop(engine);
                for path in project["files"].as_object().unwrap().keys() {
                    std::fs::write(root.join(path), "changed outside the committed index").unwrap();
                }
                engine = Engine::open(&root, Some(&database)).unwrap();
            }
            let graph = super::super::Graph::load(&engine.connection, 1).unwrap();
            let actual = normalized(
                graph
                    .canonical_edges
                    .iter()
                    .flatten()
                    .map(|edge| serde_json::to_value(edge).unwrap()),
            );
            if actual != expected {
                let mut missing = expected.clone();
                let mut extra = Vec::new();
                for edge in actual {
                    if let Some(at) = missing.iter().position(|other| other == &edge) {
                        missing.remove(at);
                    } else {
                        extra.push(edge);
                    }
                }
                let values = |rows: Vec<String>| {
                    rows.iter()
                        .map(|row| serde_json::from_str::<Value>(row).unwrap())
                        .collect::<Vec<_>>()
                };
                differences.push(serde_json::json!({"project":project["name"],"files":project["files"],"reopened":reopened,"missing":values(missing),"extra":values(extra)}));
                break;
            }
        }
    }
    let path = std::env::temp_dir().join("prometeu-python-complete-diffs.json");
    std::fs::write(&path, serde_json::to_vec_pretty(&differences).unwrap()).unwrap();
    assert!(
        differences.is_empty(),
        "{} projects differ; {}",
        differences.len(),
        path.display()
    );
}
