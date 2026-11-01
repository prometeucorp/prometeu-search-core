use super::*;
use crate::Engine;
use serde_json::Value;

#[test]
fn play_complete_framework_edges_match_indexed_reference() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/upstream/play/cases.json"
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
        let mut engine = Engine::open(&root, Some(&database)).unwrap();
        engine.sync().unwrap();
        for reopened in [false, true] {
            if reopened {
                let cached = engine
                    .connection
                    .prepare("SELECT path,analysis FROM files")
                    .unwrap()
                    .query_map([], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                    })
                    .unwrap()
                    .collect::<rusqlite::Result<Vec<_>>>()
                    .unwrap();
                for (path, source) in cached {
                    let mut analysis: Value = serde_json::from_str(&source).unwrap();
                    for reference in analysis["references"].as_array_mut().unwrap() {
                        reference.as_object_mut().unwrap().remove("column");
                    }
                    engine
                        .connection
                        .execute(
                            "UPDATE files SET analysis=?1 WHERE path=?2",
                            rusqlite::params![analysis.to_string(), path],
                        )
                        .unwrap();
                }
                drop(engine);
                for path in project["files"].as_object().unwrap().keys() {
                    std::fs::write(root.join(path), "modified after committed snapshot").unwrap();
                }
                engine = Engine::open(&root, Some(&database)).unwrap();
            }
            let graph = Graph::load(&engine.connection, 1).unwrap();
            let actual = graph
                .canonical_edges
                .iter()
                .flatten()
                .filter(|edge| {
                    edge.metadata
                        .as_ref()
                        .is_some_and(|metadata| metadata["framework"] == "play")
                })
                .cloned()
                .collect::<Vec<_>>();
            let expected: Vec<prometeu_extract::Edge> =
                serde_json::from_value(project["frameworkEdges"].clone()).unwrap();
            let normalize = |edges: Vec<prometeu_extract::Edge>| {
                let mut rows = edges
                    .into_iter()
                    .map(|edge| {
                        let mut value = serde_json::to_value(edge).unwrap();
                        value.sort_all_objects();
                        value.to_string()
                    })
                    .collect::<Vec<_>>();
                rows.sort();
                rows
            };
            assert_eq!(
                normalize(actual),
                normalize(expected),
                "{} reopened={reopened}",
                project["name"]
            );
        }
    }
}
