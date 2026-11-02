use prometeu_search::{Engine, GraphOptions};
use serde_json::Value;
use std::{collections::HashMap, fs};

#[test]
fn drupal_records_and_framework_edges_match_reference_after_reopen() {
    let corpus: Value =
        serde_json::from_str(include_str!("fixtures/upstream/drupal/cases.json")).unwrap();
    for project in corpus["projects"].as_array().unwrap() {
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
            let connection = rusqlite::Connection::open(&database).unwrap();
            let mut query = connection
                .prepare(
                    "SELECT s.id,s.ordinal,f.analysis FROM symbols s JOIN files f ON s.path=f.path",
                )
                .unwrap();
            let mut ids = HashMap::new();
            let mut records = HashMap::new();
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
                let node = &analysis["extraction"]["nodes"][ordinal as usize];
                if let Some(canonical) = node["id"].as_str() {
                    ids.insert(canonical.to_owned(), id);
                    records.insert(canonical.to_owned(), node.clone());
                }
            }
            let reverse: HashMap<_, _> =
                ids.iter().map(|(canonical, id)| (*id, canonical)).collect();
            let nodes: HashMap<_, _> = project["nodes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|node| (node["id"].as_str().unwrap(), node))
                .collect();
            for extracted in project["extracted"].as_array().unwrap() {
                for expected in extracted["expected"]["nodes"].as_array().unwrap() {
                    let id = expected["id"].as_str().unwrap();
                    if !nodes.contains_key(id) {
                        assert!(!records.contains_key(id), "inactive framework emitted {id}");
                        continue;
                    }
                    let mut actual = records
                        .get(id)
                        .unwrap_or_else(|| panic!("missing {id}"))
                        .clone();
                    actual.as_object_mut().unwrap().remove("updatedAt");
                    assert_eq!(&actual, expected, "{} {id}", project["name"]);
                }
            }
            for (canonical, source) in &nodes {
                let Some(&id) = ids.get(*canonical) else {
                    continue;
                };
                let graph = engine
                    .explore(
                        source["name"].as_str().unwrap(),
                        &GraphOptions {
                            path: Some(source["filePath"].as_str().unwrap().into()),
                            line: Some(source["startLine"].as_u64().unwrap() as usize),
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
                    .filter(|edge| {
                        edge.from == id
                            && edge
                                .metadata
                                .as_ref()
                                .is_some_and(|metadata| metadata["framework"] == "drupal")
                    })
                    .map(|edge| {
                        (
                            edge.kind.clone(),
                            reverse[&edge.to.unwrap()].as_str().to_owned(),
                            edge.line,
                            edge.metadata.clone().unwrap(),
                        )
                    })
                    .collect();
                let mut expected: Vec<_> = project["edges"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|edge| edge["source"] == *canonical)
                    .map(|edge| {
                        (
                            edge["kind"].as_str().unwrap().to_owned(),
                            edge["target"].as_str().unwrap().to_owned(),
                            edge["line"].as_u64().unwrap() as usize,
                            edge["metadata"].clone(),
                        )
                    })
                    .collect();
                actual.sort_by(|a, b| (&a.0, &a.1, a.2).cmp(&(&b.0, &b.1, b.2)));
                expected.sort_by(|a, b| (&a.0, &a.1, a.2).cmp(&(&b.0, &b.1, b.2)));
                assert_eq!(
                    actual, expected,
                    "{} {canonical} {:?}",
                    project["name"], graph.edges
                );
                if source["kind"] == "component"
                    || source["kind"] == "route"
                    || source["filePath"] == "src/Checkout.php" && source["name"] == "run"
                {
                    let mut actual: Vec<_> = graph
                        .edges
                        .iter()
                        .filter(|edge| {
                            edge.from == id
                                && edge.to.is_some()
                                && matches!(edge.kind.as_str(), "calls" | "references")
                        })
                        .map(|edge| {
                            (
                                edge.kind.as_str(),
                                reverse[&edge.to.unwrap()].as_str(),
                                edge.line,
                            )
                        })
                        .collect();
                    let mut expected: Vec<_> = project["allEdges"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .filter(|edge| {
                            edge["source"] == *canonical
                                && matches!(edge["kind"].as_str(), Some("calls" | "references"))
                        })
                        .map(|edge| {
                            (
                                edge["kind"].as_str().unwrap(),
                                edge["target"].as_str().unwrap(),
                                edge["line"].as_u64().unwrap() as usize,
                            )
                        })
                        .collect();
                    actual.sort();
                    expected.sort();
                    assert_eq!(
                        actual, expected,
                        "complete resolved edges: {} {canonical}",
                        project["name"]
                    );
                }
            }
        }
    }
}

#[test]
fn service_target_arrival_removal_and_detection_changes_commit_together() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("project");
    fs::create_dir_all(root.join("src")).unwrap();
    let database = temporary.path().join("index.sqlite");
    fs::write(
        root.join("composer.json"),
        r#"{"name":"drupal/shop","type":"drupal-module"}"#,
    )
    .unwrap();
    fs::write(
        root.join("shop.services.yml"),
        "services:\n  shop.cart:\n    class: Drupal\\shop\\Cart\n",
    )
    .unwrap();
    let mut engine = Engine::open(&root, Some(&database)).unwrap();
    engine.sync().unwrap();
    let options = GraphOptions {
        path: Some("shop.services.yml".into()),
        depth: 1,
        limit: 50,
        max_bytes: 65536,
        ..Default::default()
    };
    let targets = |engine: &Engine, graph: prometeu_search::GraphResult| {
        graph
            .edges
            .into_iter()
            .filter(|edge| {
                edge.metadata
                    .as_ref()
                    .is_some_and(|meta| meta["framework"] == "drupal")
            })
            .map(|edge| {
                engine
                    .node(edge.to.unwrap(), 65536)
                    .unwrap()
                    .node
                    .unwrap()
                    .name
            })
            .collect::<Vec<_>>()
    };
    assert!(
        targets(
            &engine,
            engine.explore("service:shop.cart", &options).unwrap()
        )
        .is_empty()
    );
    fs::write(
        root.join("src/Cart.php"),
        "<?php\nnamespace Drupal\\shop;\nclass Cart {}\n",
    )
    .unwrap();
    assert!(
        targets(
            &engine,
            engine.explore("service:shop.cart", &options).unwrap()
        )
        .is_empty()
    );
    engine
        .sync_paths(&[std::path::PathBuf::from("src/Cart.php")])
        .unwrap();
    assert_eq!(
        targets(
            &engine,
            engine.explore("service:shop.cart", &options).unwrap()
        ),
        ["Cart"]
    );
    drop(engine);
    fs::remove_file(root.join("src/Cart.php")).unwrap();
    engine = Engine::open(&root, Some(&database)).unwrap();
    assert_eq!(
        targets(
            &engine,
            engine.explore("service:shop.cart", &options).unwrap()
        ),
        ["Cart"]
    );
    engine
        .sync_paths(&[std::path::PathBuf::from("src/Cart.php")])
        .unwrap();
    assert!(
        targets(
            &engine,
            engine.explore("service:shop.cart", &options).unwrap()
        )
        .is_empty()
    );
    fs::write(root.join("composer.json"), "{}").unwrap();
    engine
        .sync_paths(&[std::path::PathBuf::from("composer.json")])
        .unwrap();
    assert!(
        engine
            .search("kind:component", 50, 65536)
            .unwrap()
            .results
            .is_empty()
    );
    fs::write(
        root.join("composer.json"),
        r#"{"require":{"drupal/core":"*"}}"#,
    )
    .unwrap();
    engine
        .sync_paths(&[std::path::PathBuf::from("composer.json")])
        .unwrap();
    assert_eq!(
        engine
            .search("kind:component", 50, 65536)
            .unwrap()
            .results
            .len(),
        1
    );
}
