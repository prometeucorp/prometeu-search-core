use super::*;
use crate::Engine;
use serde_json::Value;

#[test]
fn python_complete_selected_edges_match_indexed_reference() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/upstream/python-frameworks/cases.json"
    ))
    .unwrap();
    for project in fixture["projects"].as_array().unwrap() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("project");
        let database = temporary.path().join("index.sqlite");
        for (path, source) in project["files"].as_object().unwrap() {
            let path = root.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, source.as_str().unwrap()).unwrap();
        }
        let frameworks: Vec<String> =
            serde_json::from_value(project["frameworks"].clone()).unwrap();
        let selected = |edge: &prometeu_extract::Edge| {
            let Some(metadata) = edge.metadata.as_ref() else {
                return false;
            };
            let Some(name) = metadata["refName"].as_str() else {
                return false;
            };
            let reference: prometeu_extract::UnresolvedReference =
                serde_json::from_value(serde_json::json!({
                    "fromNodeId": edge.source,
                    "referenceName": name,
                    "referenceKind": metadata["refKind"].as_str().unwrap_or(&edge.kind),
                    "line": edge.line.unwrap_or(1),
                    "column": edge.column.unwrap_or(0),
                    "language": "python"
                }))
                .unwrap();
            crate::frameworks::python_resolution::Bindings::handles(&reference, &frameworks)
                .unwrap()
        };
        let normalize = |edges: Vec<prometeu_extract::Edge>| {
            let mut rows = edges
                .into_iter()
                .filter(&selected)
                .map(|edge| {
                    let mut value = serde_json::to_value(edge).unwrap();
                    value.sort_all_objects();
                    value.to_string()
                })
                .collect::<Vec<_>>();
            rows.sort();
            rows
        };
        let expected = normalize(serde_json::from_value(project["edges"].clone()).unwrap());
        let mut engine = Engine::open(&root, Some(&database)).unwrap();
        engine.sync().unwrap();
        for reopened in [false, true] {
            if reopened {
                drop(engine);
                for path in project["files"].as_object().unwrap().keys() {
                    std::fs::write(root.join(path), "modified after committed snapshot").unwrap();
                }
                engine = Engine::open(&root, Some(&database)).unwrap();
            }
            let graph = Graph::load(&engine.connection, 1).unwrap();
            let actual = normalize(graph.canonical_edges.iter().flatten().cloned().collect());
            assert_eq!(actual, expected, "{} reopened={reopened}", project["name"]);
        }
    }
}
