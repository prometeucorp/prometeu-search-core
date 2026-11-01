use prometeu_search::{Engine, GraphOptions};
use rusqlite::Connection;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;

#[test]
fn indexed_synthesis_retains_reference_metadata_after_reopen() {
    let corpus: Value =
        serde_json::from_str(include_str!("fixtures/upstream/synthesis/cases.json")).unwrap();
    let accepted = [
        "kotlin-platforms",
        "kotlin-typealias",
        "mybatis",
        "mybatis-kotlin-vendor",
        "react-render",
        "flutter-build",
        "arkui-state",
        "nix-1",
        "nix-2",
        "nix-3",
        "nix-4",
        "nix-6",
        "callback-field",
        "callback-import",
        "closure-0",
        "emitter-order-unicode",
        "spring-0",
        "mediatr-0",
        "svelte-loaders",
        "redux-thunks",
        "celery-0",
        "sidekiq-0",
        "erlang-0",
        "laravel-registration",
        "gin-0",
        "goframe-0",
        "rtk-0",
        "pinia-0",
        "vuex-0",
        "registry-0",
        "fabric-native",
        "expo-native",
        "rn-0",
        "rn-3",
        "rn-5",
        "vue-template-handlers",
        "next-markup-navigation",
        "next-call-navigation",
        "expo-call-navigation",
        "svelte-call-navigation",
        "next-proxy-0",
        "next-proxy-1",
        "next-proxy-2",
        "next-proxy-3",
        "next-proxy-4",
        "next-proxy-5",
        "next-proxy-10",
        "tanstack-original-app",
        "tanstack-code-tree",
        "vue-router-original-app",
        "vue-router-nested-layouts",
        "expo-helper-return",
        "svelte-markup-navigation",
        "svelte-app-boundaries",
        "arkui-2",
        "arkui-4",
        "arkui-numeric-same-file",
    ];
    for case in corpus["cases"].as_array().unwrap().iter().filter(|case| {
        accepted.contains(&case["id"].as_str().unwrap())
            || case["id"].as_str().unwrap().starts_with("jsx-")
            || case["id"].as_str().unwrap().starts_with("react-tables-")
    }) {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("project");
        fs::create_dir(&root).unwrap();
        let database = temporary.path().join("index.sqlite");
        if let Some(assets) = case.get("assets").and_then(Value::as_object) {
            for (path, source) in assets {
                let path = root.join(path);
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(path, source.as_str().unwrap()).unwrap();
            }
        }
        for (path, source) in case["files"].as_object().unwrap() {
            let path = root.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, source.as_str().unwrap()).unwrap();
        }
        let mut engine = Engine::open(&root, Some(&database)).unwrap();
        engine.sync().unwrap();
        drop(engine);
        let mut engine = Engine::open(&root, Some(&database)).unwrap();
        assert_eq!(engine.sync().unwrap().indexed, 0);
        let connection = Connection::open(&database).unwrap();
        let mut statement = connection
            .prepare(
                "SELECT s.id,s.ordinal,f.analysis FROM symbols s JOIN files f ON f.path=s.path",
            )
            .unwrap();
        let ids: HashMap<String, i64> = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .unwrap()
            .filter_map(|row| {
                let (id, ordinal, analysis) = row.unwrap();
                let analysis: Value = serde_json::from_str(&analysis).unwrap();
                analysis["extraction"]["nodes"]
                    .get(ordinal as usize)
                    .and_then(|node| node["id"].as_str())
                    .map(|canonical| (canonical.to_owned(), id))
            })
            .collect();
        if let Some(finalization) = case.get("finalization") {
            let mut statement = connection.prepare("SELECT analysis FROM files").unwrap();
            let files = statement
                .query_map([], |row| row.get::<_, String>(0))
                .unwrap()
                .map(|row| serde_json::from_str::<Value>(&row.unwrap()).unwrap())
                .collect::<Vec<_>>();
            for expected in finalization["nodes"].as_array().unwrap().iter() {
                let actual = files
                    .iter()
                    .flat_map(|file| file["extraction"]["nodes"].as_array().into_iter().flatten())
                    .find(|node| node["id"] == expected["id"])
                    .unwrap_or_else(|| {
                        panic!("{} missing canonical route {}", case["id"], expected["id"])
                    });
                let normalize = |node: &Value| {
                    let mut node = node.clone();
                    node["updatedAt"] = 0.into();
                    serde_json::from_value::<prometeu_extract::Node>(node).unwrap()
                };
                assert_eq!(
                    serde_json::to_value(normalize(actual)).unwrap(),
                    serde_json::to_value(normalize(expected)).unwrap(),
                    "{} canonical route",
                    case["id"]
                );
            }
            for expected in finalization["updates"].as_array().unwrap() {
                let actual = files
                    .iter()
                    .flat_map(|file| file["extraction"]["nodes"].as_array().into_iter().flatten())
                    .find(|node| node["id"] == expected["id"])
                    .unwrap();
                // SQL readback supplies false for omitted boolean flags; finalization changes only names.
                for field in ["id", "name", "qualifiedName", "signature"] {
                    assert_eq!(
                        actual[field], expected[field],
                        "{} updated route {field}",
                        case["id"]
                    );
                }
            }
            for expected in finalization["references"].as_array().unwrap() {
                assert!(
                    files
                        .iter()
                        .flat_map(|file| file["extraction"]["unresolvedReferences"]
                            .as_array()
                            .into_iter()
                            .flatten())
                        .any(|reference| reference == expected),
                    "{} missing canonical reference {expected}",
                    case["id"]
                );
            }
        }
        let nodes: HashMap<_, _> = case["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|node| (node["id"].as_str().unwrap(), node))
            .collect();
        for expected in case["edges"].as_array().unwrap().iter().filter(|edge| {
            edge["metadata"]["synthesizedBy"].is_string()
                || (edge["kind"] == "navigates" && edge["metadata"]["resolvedBy"] == "framework")
                || edge["metadata"]["framework"] == "vue-router"
        }) {
            let source = nodes[expected["source"].as_str().unwrap()];
            let options = GraphOptions {
                path: Some(source["filePath"].as_str().unwrap().to_owned()),
                line: Some(source["startLine"].as_u64().unwrap() as usize),
                depth: 1,
                limit: 50,
                max_bytes: 65536,
                ..Default::default()
            };
            let result = engine
                .explore(source["name"].as_str().unwrap(), &options)
                .unwrap();
            assert!(
                result.edges.iter().any(|edge| edge.from
                    == ids[expected["source"].as_str().unwrap()]
                    && edge.to == Some(ids[expected["target"].as_str().unwrap()])
                    && edge.kind == expected["kind"].as_str().unwrap()
                    && edge.metadata.as_ref() == Some(&expected["metadata"])
                    && edge.provenance.as_deref() == expected["provenance"].as_str()),
                "{} missing {expected:?}: {:?}",
                case["id"],
                result.edges
            );
        }
    }
}

#[test]
fn cached_synthesis_rebuilds_when_a_second_connection_changes_sources() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("project");
    fs::create_dir(&root).unwrap();
    let database = temporary.path().join("index.sqlite");
    let file = root.join("Counter.ts");
    fs::write(&file,"class Counter {\n increment() { this.setState({ count: 1 }); }\n render() { return null; }\n}\n").unwrap();
    let mut first = Engine::open(&root, Some(&database)).unwrap();
    first.sync().unwrap();
    let options = GraphOptions {
        depth: 1,
        limit: 50,
        max_bytes: 65536,
        ..Default::default()
    };
    let before = first.explore("increment", &options).unwrap();
    assert!(before.edges.iter().any(|edge| {
        edge.metadata
            .as_ref()
            .is_some_and(|m| m["synthesizedBy"] == "react-render")
    }));
    fs::write(
        &file,
        "class Counter {\n increment() { return 1; }\n render() { return null; }\n}\n",
    )
    .unwrap();
    let mut second = Engine::open(&root, Some(&database)).unwrap();
    second.sync().unwrap();
    let after = first.explore("increment", &options).unwrap();
    assert!(after.generation > before.generation);
    assert!(!after.edges.iter().any(|edge| {
        edge.metadata
            .as_ref()
            .is_some_and(|m| m["synthesizedBy"] == "react-render")
    }));
}

#[test]
fn cross_file_react_routes_follow_consumers_and_incremental_edits() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("project");
    fs::create_dir(&root).unwrap();
    let database = temporary.path().join("index.sqlite");
    fs::write(
        root.join("package.json"),
        r#"{"dependencies":{"react":"18","react-router-dom":"6"}}"#,
    )
    .unwrap();
    fs::write(
        root.join("Page.tsx"),
        "export function Page() { return <main />; }\n",
    )
    .unwrap();
    let routes = |path: &str| {
        format!(
            "import {{ Page }} from './Page';\nexport const routes = [{{path: '{path}', element: <Page />}}];\n"
        )
    };
    fs::write(root.join("routes.tsx"), routes("/first")).unwrap();
    let app = "import { useRoutes } from 'react-router-dom';\nimport { routes } from './routes';\nexport function App() { return useRoutes(routes); }\n";
    fs::write(root.join("App.tsx"), app).unwrap();
    let mut engine = Engine::open(&root, Some(&database)).unwrap();
    engine.sync().unwrap();
    let first = engine.search("kind:route", 20, 65536).unwrap();
    assert_eq!(first.results.len(), 1);
    assert_eq!(first.results[0].name, "/first");
    let first_id = first.results[0].id;
    assert_eq!(engine.sync().unwrap().indexed, 0);
    assert_eq!(
        engine.search("kind:route", 20, 65536).unwrap().results[0].id,
        first_id
    );

    fs::write(root.join("routes.tsx"), routes("/second")).unwrap();
    engine.sync_paths(&["routes.tsx".into()]).unwrap();
    let second = engine.search("kind:route", 20, 65536).unwrap();
    assert_eq!(second.results.len(), 1);
    assert_eq!(second.results[0].name, "/second");
    assert!(second.generation > first.generation);
    assert!(
        engine
            .search("kind:route name:/first", 20, 65536)
            .unwrap()
            .results
            .is_empty()
    );
    drop(engine);
    let mut engine = Engine::open(&root, Some(&database)).unwrap();
    assert_eq!(engine.sync().unwrap().indexed, 0);
    assert_eq!(
        engine.search("kind:route", 20, 65536).unwrap().results[0].id,
        second.results[0].id
    );

    fs::write(
        root.join("App.tsx"),
        "export function App() { return null; }\n",
    )
    .unwrap();
    engine.sync_paths(&["App.tsx".into()]).unwrap();
    assert!(
        engine
            .search("kind:route", 20, 65536)
            .unwrap()
            .results
            .is_empty()
    );
    fs::write(root.join("App.tsx"), app).unwrap();
    engine.sync_paths(&["App.tsx".into()]).unwrap();
    assert_eq!(
        engine.search("kind:route", 20, 65536).unwrap().results[0].name,
        "/second"
    );
    fs::remove_file(root.join("App.tsx")).unwrap();
    engine.sync_paths(&["App.tsx".into()]).unwrap();
    assert!(
        engine
            .search("kind:route", 20, 65536)
            .unwrap()
            .results
            .is_empty()
    );
    drop(engine);
    let engine = Engine::open(&root, Some(&database)).unwrap();
    assert!(
        engine
            .search("kind:route", 20, 65536)
            .unwrap()
            .results
            .is_empty()
    );
}
