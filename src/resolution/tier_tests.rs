//! Channel synthesis through the persistent index and reopened snapshots.
use super::super::Graph;
use crate::{Engine, GraphOptions};
use serde_json::Value;

#[test]
fn indexed_channels_match_reference_and_reopen() {
    let corpus: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/upstream/synthesis/tier/cases.json"
    ))
    .unwrap();
    let supported = [
        "tier-original",
        "requests-1",
        "requests-2",
        "requests-3",
        "requests-4",
        "requests-5",
        "requests-6",
        "requests-7",
        "http-specificity",
        "queue-socket-bus",
    ];
    for project in corpus["projects"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|project| supported.contains(&project["id"].as_str().unwrap()))
    {
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
                for path in project["files"].as_object().unwrap().keys() {
                    std::fs::write(root.join(path), "changed live source").unwrap();
                }
                engine = Engine::open(&root, Some(&database)).unwrap();
            }
            let graph = Graph::load(&engine.connection, 1).unwrap();
            let mut actual: Vec<_> = graph
                .canonical_edges
                .iter()
                .flatten()
                .filter(|edge| {
                    edge.metadata
                        .as_ref()
                        .and_then(|metadata| metadata["synthesizedBy"].as_str())
                        .is_some_and(|pass| {
                            ["http-client", "queue-job", "event-bus", "test-request"]
                                .contains(&pass)
                        })
                })
                .map(|edge| serde_json::to_value(edge).unwrap())
                .collect();
            let mut expected: Vec<_> = ["cross", "requests"]
                .iter()
                .flat_map(|pass| {
                    project["expected"][pass]
                        .as_array()
                        .unwrap()
                        .iter()
                        .cloned()
                })
                .collect();
            actual.sort_by_key(|value| serde_json::to_string(value).unwrap());
            expected.sort_by_key(|value| serde_json::to_string(value).unwrap());
            assert_eq!(actual, expected, "{} reopened={reopened}", project["id"]);
        }
    }
}

#[test]
fn channel_cache_tracks_route_changes_and_removal() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("project");
    std::fs::create_dir(&root).unwrap();
    let database = temporary.path().join("index.sqlite");
    std::fs::write(
        root.join("package.json"),
        r#"{"dependencies":{"express":"*"}}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("routes.ts"),
        "function handler() {}\napp.get('/old',handler);\n",
    )
    .unwrap();
    std::fs::write(
        root.join("client.ts"),
        "function load(){return fetch('/old')}\n",
    )
    .unwrap();
    let mut first = Engine::open(&root, Some(&database)).unwrap();
    first.sync().unwrap();
    let options = GraphOptions {
        depth: 1,
        limit: 50,
        max_bytes: 65536,
        ..Default::default()
    };
    let before = first.explore("load", &options).unwrap();
    assert!(before.edges.iter().any(|edge| {
        edge.metadata
            .as_ref()
            .is_some_and(|value| value["synthesizedBy"] == "http-client")
    }));
    let mut second = Engine::open(&root, Some(&database)).unwrap();
    std::fs::write(
        root.join("routes.ts"),
        "function handler() {}\napp.get('/new',handler);\n",
    )
    .unwrap();
    second.sync_paths(&["routes.ts".into()]).unwrap();
    let after = first.explore("load", &options).unwrap();
    assert!(after.generation > before.generation);
    assert!(!after.edges.iter().any(|edge| {
        edge.metadata
            .as_ref()
            .is_some_and(|value| value["synthesizedBy"] == "http-client")
    }));
    std::fs::write(
        root.join("client.ts"),
        "function load(){return fetch('/new')}\n",
    )
    .unwrap();
    second.sync_paths(&["client.ts".into()]).unwrap();
    let after = first.explore("load", &options).unwrap();
    assert!(after.edges.iter().any(|edge| {
        edge.metadata
            .as_ref()
            .is_some_and(|value| value["synthesizedBy"] == "http-client" && value["href"] == "/new")
    }));
    std::fs::remove_file(root.join("routes.ts")).unwrap();
    second.sync_paths(&["routes.ts".into()]).unwrap();
    assert!(
        !first
            .explore("load", &options)
            .unwrap()
            .edges
            .iter()
            .any(|edge| edge
                .metadata
                .as_ref()
                .is_some_and(|value| value["synthesizedBy"] == "http-client"))
    );
}
