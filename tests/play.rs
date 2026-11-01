use prometeu_search::{Engine, GraphOptions};
use serde_json::Value;
use std::{collections::HashMap, fs};

#[test]
fn play_routes_and_edges_match_reference_after_reopen() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/upstream/play/cases.json")).unwrap();
    for project in fixture["projects"].as_array().unwrap() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("project");
        fs::create_dir(&root).unwrap();
        let database = temporary.path().join("index.sqlite");
        for (path, source) in project["files"].as_object().unwrap() {
            let path = root.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, source.as_str().unwrap()).unwrap();
        }
        let mut engine = Engine::open(&root, Some(&database)).unwrap();
        engine.sync().unwrap();
        for reopen in [false, true] {
            if reopen {
                drop(engine);
                for path in project["files"].as_object().unwrap().keys() {
                    fs::write(root.join(path), "changed outside index").unwrap();
                }
                engine = Engine::open(&root, Some(&database)).unwrap();
            }
            let routes = engine.search("kind:route", 50, 65536).unwrap();
            assert!(!routes.truncated);
            assert_eq!(
                routes.results.len(),
                project["routes"].as_array().unwrap().len(),
                "{}",
                project["name"]
            );
            let mut ids = HashMap::new();
            let mut connection = rusqlite::Connection::open(&database).unwrap();
            let transaction = connection.transaction().unwrap();
            let mut query = transaction
                .prepare(
                    "SELECT s.id,s.ordinal,f.analysis FROM symbols s JOIN files f ON s.path=f.path",
                )
                .unwrap();
            for row in query
                .query_map([], |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                })
                .unwrap()
            {
                let (id, ordinal, analysis) = row.unwrap();
                let analysis: Value = serde_json::from_str(&analysis).unwrap();
                if let Some(canonical) =
                    analysis["extraction"]["nodes"][ordinal as usize]["id"].as_str()
                {
                    ids.insert(id, canonical.to_owned());
                }
            }
            let metadata = |mut value: Value| {
                value.sort_all_objects();
                value.to_string()
            };
            for expected in project["routes"].as_array().unwrap() {
                let hit = routes
                    .results
                    .iter()
                    .find(|hit| {
                        ids.get(&hit.id)
                            .is_some_and(|id| id == expected["node"]["id"].as_str().unwrap())
                    })
                    .unwrap();
                let node = engine
                    .node(hit.id, 65536)
                    .unwrap()
                    .node
                    .unwrap()
                    .metadata
                    .unwrap();
                let mut actual = serde_json::to_value(&node).unwrap();
                actual.as_object_mut().unwrap().remove("updatedAt");
                for (key, default) in [
                    ("visibility", Value::Null),
                    ("isExported", false.into()),
                    ("isAsync", false.into()),
                    ("isStatic", false.into()),
                    ("isAbstract", false.into()),
                ] {
                    actual
                        .as_object_mut()
                        .unwrap()
                        .entry(key)
                        .or_insert(default);
                }
                assert_eq!(actual, expected["node"], "{}", project["name"]);
                let graph = engine
                    .explore(
                        &node.name,
                        &GraphOptions {
                            path: Some(node.file_path.clone()),
                            line: Some(node.start_line as usize),
                            depth: 1,
                            limit: 50,
                            max_bytes: 65536,
                            ..Default::default()
                        },
                    )
                    .unwrap();
                let mut actual: Vec<_> = graph
                    .edges
                    .iter()
                    .filter(|edge| edge.from == hit.id)
                    .filter_map(|edge| {
                        edge.to.and_then(|to| ids.get(&to)).map(|to| {
                            (
                                edge.kind.clone(),
                                to.clone(),
                                metadata(edge.metadata.clone().unwrap_or(Value::Null)),
                            )
                        })
                    })
                    .collect();
                actual.sort();
                actual.dedup();
                let mut targets: Vec<_> = expected["targets"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|target| {
                        (
                            target["kind"].as_str().unwrap().to_owned(),
                            target["node"]["id"].as_str().unwrap().to_owned(),
                            metadata(target["metadata"].clone()),
                        )
                    })
                    .collect();
                targets.sort();
                targets.dedup();
                assert_eq!(actual, targets, "{} {}", project["name"], node.name);
            }
        }
    }
}

#[test]
fn play_overloads_refresh_across_connections_and_marker_changes() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("project");
    fs::create_dir(&root).unwrap();
    let database = temporary.path().join("index.sqlite");
    fs::write(root.join("build.sbt"), "enablePlugins(PlayJava)\n").unwrap();
    fs::write(root.join("Api.java"), "class Api {\n public static int pick(int value) { return 1; }\n public static int pick(String value) { return 2; }\n}\n").unwrap();
    let caller = |argument: &str| {
        format!("class Client {{\n int call() {{ return Api.pick({argument}); }}\n}}\n")
    };
    fs::write(root.join("Client.java"), caller("1")).unwrap();
    let mut writer = Engine::open(&root, Some(&database)).unwrap();
    writer.sync().unwrap();
    let reader = Engine::open(&root, Some(&database)).unwrap();
    let options = GraphOptions {
        path: Some("Client.java".into()),
        line: Some(2),
        depth: 1,
        limit: 50,
        max_bytes: 65536,
        ..Default::default()
    };
    let target = |engine: &Engine| {
        let graph = engine.explore("call", &options).unwrap();
        let targets = graph
            .edges
            .iter()
            .filter(|edge| {
                edge.metadata
                    .as_ref()
                    .is_some_and(|m| m["framework"] == "play")
            })
            .filter_map(|edge| edge.to)
            .map(|id| {
                engine
                    .node(id, 65536)
                    .unwrap()
                    .node
                    .unwrap()
                    .metadata
                    .unwrap()
                    .start_line
            })
            .collect::<Vec<_>>();
        (graph.generation, targets)
    };
    let before = target(&reader);
    assert_eq!(before.1, vec![2]);
    fs::write(root.join("Client.java"), caller("\"value\"")).unwrap();
    assert_eq!(target(&reader).1, vec![2]);
    writer.sync_paths(&["Client.java".into()]).unwrap();
    let after = target(&reader);
    assert!(after.0 > before.0);
    assert_eq!(after.1, vec![3]);
    drop(reader);
    let reopened = Engine::open(&root, Some(&database)).unwrap();
    assert_eq!(target(&reopened).1, vec![3]);
    fs::write(root.join("build.sbt"), "scalaVersion := \"3.3.0\"\n").unwrap();
    writer.sync_paths(&["build.sbt".into()]).unwrap();
    assert!(target(&reopened).1.is_empty());
    fs::write(root.join("build.sbt"), "enablePlugins(PlayJava)\n").unwrap();
    writer.sync_paths(&["build.sbt".into()]).unwrap();
    assert_eq!(target(&reopened).1, vec![3]);
}
