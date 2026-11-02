use prometeu_search::{Engine, GraphOptions, parser};
use std::fs;
use std::path::Path;
use tempfile::TempDir;

#[test]
fn framework_discovery_matches_complete_synthesis_project_corpus() {
    let corpus: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/upstream/synthesis/cases.json")).unwrap();
    let catalog: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/upstream/frameworks/detection.json")).unwrap();
    let mut projects = corpus["cases"].as_array().unwrap().clone();
    let mut covered = std::collections::BTreeSet::new();
    for mut case in catalog["cases"].as_array().unwrap().iter().cloned() {
        covered.extend(
            case["expected"]
                .as_array()
                .unwrap()
                .iter()
                .map(|name| name.as_str().unwrap().to_owned()),
        );
        case["activeFrameworks"] = case["expected"].clone();
        case["checkLifecycle"] = true.into();
        projects.push(case);
    }
    assert_eq!(
        covered,
        catalog["registered"]
            .as_array()
            .unwrap()
            .iter()
            .map(|name| name.as_str().unwrap().to_owned())
            .collect()
    );
    let mut differences = Vec::new();
    for case in &projects {
        let temporary = TempDir::new().unwrap();
        let root = temporary.path().join("project");
        fs::create_dir(&root).unwrap();
        for group in ["files", "assets"] {
            if let Some(files) = case[group].as_object() {
                for (path, text) in files {
                    let path = root.join(path);
                    fs::create_dir_all(path.parent().unwrap()).unwrap();
                    fs::write(path, text.as_str().unwrap()).unwrap();
                }
            }
        }
        let database = temporary.path().join("index.sqlite");
        let mut engine = Engine::open(&root, Some(&database)).unwrap();
        engine.sync().unwrap();
        let connection = rusqlite::Connection::open(&database).unwrap();
        let stored: String = connection
            .query_row(
                "SELECT value FROM metadata WHERE key='extraction_frameworks'",
                [],
                |row| row.get(0),
            )
            .unwrap_or_else(|_| "[]".into());
        let mut actual: Vec<String> = serde_json::from_str(&stored).unwrap();
        actual.sort();
        let mut expected: Vec<String> =
            serde_json::from_value(case["activeFrameworks"].clone()).unwrap();
        expected.sort();
        if actual != expected {
            differences.push(format!(
                "{}: expected {expected:?}; actual {actual:?}",
                case["id"]
            ));
        }
        if case["checkLifecycle"] == true {
            drop(engine);
            let mut engine = Engine::open(&root, Some(&database)).unwrap();
            assert_eq!(
                engine.sync().unwrap().indexed,
                0,
                "{} unchanged reopen",
                case["id"]
            );
            let files = case["files"].as_object().unwrap();
            for path in files.keys() {
                fs::remove_file(root.join(path)).unwrap();
            }
            let paths = files
                .keys()
                .map(std::path::PathBuf::from)
                .collect::<Vec<_>>();
            engine.sync_paths(&paths).unwrap();
            let stored: String = connection
                .query_row(
                    "SELECT value FROM metadata WHERE key='extraction_frameworks'",
                    [],
                    |row| row.get(0),
                )
                .unwrap_or_else(|_| "[]".into());
            assert_eq!(stored, "[]", "{} detector survives removal", case["id"]);
            for (path, text) in files {
                fs::write(root.join(path), text.as_str().unwrap()).unwrap();
            }
            engine.sync_paths(&paths).unwrap();
            let stored: String = connection
                .query_row(
                    "SELECT value FROM metadata WHERE key='extraction_frameworks'",
                    [],
                    |row| row.get(0),
                )
                .unwrap_or_else(|_| "[]".into());
            let mut restored: Vec<String> = serde_json::from_str(&stored).unwrap();
            restored.sort();
            if restored != expected {
                differences.push(format!(
                    "{} restored: expected {expected:?}; actual {restored:?}",
                    case["id"]
                ));
            }
        }
    }
    assert!(differences.is_empty(), "{}", differences.join("\n"));
}

#[test]
fn framework_records_project_to_valid_source_and_keep_metadata() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/upstream/frameworks/cases.json")).unwrap();
    for case in fixture["cases"].as_array().unwrap() {
        let path = Path::new(case["path"].as_str().unwrap());
        let source = case["source"].as_str().unwrap();
        let language = case["language"].as_str().unwrap();
        let parsed = parser::parse_with_frameworks(
            path,
            source,
            language,
            &[case["framework"].as_str().unwrap().into()],
        )
        .unwrap();
        for symbol in &parsed.symbols {
            assert!(
                source.get(symbol.start_byte..symbol.end_byte).is_some(),
                "{}: {}",
                case["id"],
                symbol.name
            );
        }
        let canonical = parsed.extraction.as_ref().unwrap();
        for expected in case["expected"]["nodes"].as_array().unwrap() {
            let node = canonical
                .nodes
                .iter()
                .find(|node| node.id == expected["id"].as_str().unwrap())
                .unwrap();
            let mut node = serde_json::to_value(node).unwrap();
            node.as_object_mut().unwrap().remove("updatedAt");
            assert_eq!(&node, expected, "{}", case["id"]);
        }
    }
}

#[test]
fn manifest_changes_reconcile_framework_nodes_and_synthesized_edges() {
    let temporary = TempDir::new().unwrap();
    let root = temporary.path().join("project");
    fs::create_dir(&root).unwrap();
    let database = temporary.path().join("index.sqlite");
    fs::write(
        root.join("Screen.ts"),
        "export default codegenNativeComponent<NativeProps>('Screen');\n",
    )
    .unwrap();
    fs::write(
        root.join("Screen.kt"),
        "class ScreenView { fun show() {} }\n",
    )
    .unwrap();
    let mut engine = Engine::open(&root, Some(&database)).unwrap();
    engine.sync().unwrap();
    assert!(
        !engine
            .search("Screen", 50, 65536)
            .unwrap()
            .results
            .iter()
            .any(|node| node.kind == "component")
    );
    fs::write(
        root.join("package.json"),
        r#"{"dependencies":{"react-native":"*"}}"#,
    )
    .unwrap();
    let report = engine.sync_paths(&["package.json".into()]).unwrap();
    assert_eq!(report.indexed, 2);
    let matches = engine.search("Screen", 50, 65536).unwrap();
    let component = matches
        .results
        .iter()
        .find(|node| node.kind == "component")
        .unwrap();
    let metadata = engine
        .node(component.id, 12000)
        .unwrap()
        .node
        .unwrap()
        .metadata
        .unwrap();
    assert!(metadata.id.starts_with("fabric-component:"));
    let graph = engine
        .explore(
            "Screen",
            &GraphOptions {
                max_bytes: 65536,
                limit: 50,
                ..Default::default()
            },
        )
        .unwrap();
    assert!(graph.edges.iter().any(|edge| {
        edge.metadata
            .as_ref()
            .is_some_and(|metadata| metadata["synthesizedBy"] == "fabric-native-impl")
    }));
    drop(engine);
    let mut engine = Engine::open(&root, Some(&database)).unwrap();
    assert!(
        engine
            .search("Screen", 50, 65536)
            .unwrap()
            .results
            .iter()
            .any(|node| node.kind == "component")
    );
    assert_eq!(engine.sync().unwrap().indexed, 0);
    fs::remove_file(root.join("package.json")).unwrap();
    engine.sync_paths(&["package.json".into()]).unwrap();
    assert!(
        !engine
            .search("Screen", 50, 65536)
            .unwrap()
            .results
            .iter()
            .any(|node| node.kind == "component")
    );
    assert!(engine.node(component.id, 12000).is_err());
}

#[test]
fn expo_source_detection_and_workspace_manifest_detection_are_indexed() {
    let temporary = TempDir::new().unwrap();
    let root = temporary.path().join("project");
    fs::create_dir_all(root.join("packages/mobile")).unwrap();
    fs::write(root.join("Battery.swift"),"class BatteryModule: Module {\n func definition() { Name(\"Battery\"); AsyncFunction(\"getLevel\") {} }\n}\n").unwrap();
    fs::write(root.join("Battery.kt"),"class BatteryModule : Module() {\n fun definition() { Name(\"Battery\"); AsyncFunction(\"getLevel\") {} }\n}\n").unwrap();
    fs::write(
        root.join("View.ts"),
        "export default requireNativeComponent('Widget');\n",
    )
    .unwrap();
    fs::write(
        root.join("packages/mobile/package.json"),
        r#"{"devDependencies":{"react-native":"*"}}"#,
    )
    .unwrap();
    let mut engine = Engine::open(&root, Some(&temporary.path().join("index.sqlite"))).unwrap();
    engine.sync().unwrap();
    let result = engine.search("getLevel", 50, 65536).unwrap();
    assert_eq!(
        result
            .results
            .iter()
            .filter(|node| node.name == "getLevel")
            .count(),
        2
    );
    assert!(
        engine
            .search("Widget", 50, 65536)
            .unwrap()
            .results
            .iter()
            .any(|node| node.kind == "component")
    );
}

#[test]
fn indexed_backend_routes_and_handlers_match_reference_and_reopen() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/upstream/frameworks/projects.json")).unwrap();
    for project in fixture["projects"].as_array().unwrap() {
        let temporary = TempDir::new().unwrap();
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
        for reopened in [false, true] {
            if reopened {
                drop(engine);
                for path in project["files"].as_object().unwrap().keys() {
                    fs::write(root.join(path), "changed outside index\n").unwrap();
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
            let constants = engine.search("kind:constant", 50, 65536).unwrap();
            let all = routes
                .results
                .iter()
                .chain(&constants.results)
                .collect::<Vec<_>>();
            for expected in project["routes"]
                .as_array()
                .unwrap()
                .iter()
                .chain(project["bindings"].as_array().unwrap_or(&Vec::new()))
            {
                let node = all
                    .iter()
                    .map(|hit| engine.node(hit.id, 65536).unwrap().node.unwrap())
                    .find(|node| {
                        node.metadata
                            .as_ref()
                            .is_some_and(|node| node.id == expected["node"]["id"].as_str().unwrap())
                    })
                    .unwrap();
                let metadata = node.metadata.unwrap();
                let mut actual = serde_json::to_value(&metadata).unwrap();
                actual.as_object_mut().unwrap().remove("updatedAt");
                for (key, default) in [
                    ("visibility", serde_json::Value::Null),
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
                let relations = engine
                    .related(
                        &metadata.qualified_name,
                        prometeu_search::Direction::Callees,
                        50,
                        65536,
                    )
                    .unwrap();
                assert!(!relations.truncated);
                let mut actual: Vec<_> = relations
                    .relations
                    .iter()
                    .filter_map(|relation| relation.candidate.as_ref())
                    .map(|target| {
                        engine
                            .node(target.id, 65536)
                            .unwrap()
                            .node
                            .unwrap()
                            .metadata
                            .unwrap()
                            .id
                    })
                    .collect();
                actual.sort();
                actual.dedup();
                let mut targets: Vec<_> = expected["targets"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|target| target["node"]["id"].as_str().unwrap().to_owned())
                    .collect();
                if let Some(uncertain) = expected["uncertainTargets"].as_array() {
                    targets = uncertain
                        .iter()
                        .map(|value| value.as_str().unwrap().to_owned())
                        .collect();
                    assert!(
                        relations
                            .relations
                            .iter()
                            .all(|relation| relation.ambiguous && relation.confidence == "low")
                    );
                }
                targets.sort();
                targets.dedup();
                assert_eq!(actual, targets, "{} {}", project["name"], metadata.name);
            }
        }
    }
}

#[test]
fn changed_fastapi_mounts_rename_unchanged_routes_and_refresh_search() {
    let temporary = TempDir::new().unwrap();
    let root = temporary.path().join("project");
    fs::create_dir(&root).unwrap();
    let database = temporary.path().join("index.sqlite");
    let main = |prefix: &str| {
        format!(
            "from fastapi import FastAPI\nfrom routes import router\napp=FastAPI()\napp.include_router(router,prefix=\"{prefix}\")\n"
        )
    };
    fs::write(root.join("main.py"), main("/v1")).unwrap();
    fs::write(root.join("routes.py"),"from fastapi import APIRouter\nrouter=APIRouter(prefix=\"/items\")\n@router.get(\"\")\ndef list_items(): return []\n").unwrap();
    let mut engine = Engine::open(&root, Some(&database)).unwrap();
    engine.sync().unwrap();
    let first = engine.search("kind:route", 10, 65536).unwrap();
    let id = first.results[0].id;
    assert_eq!(first.results[0].name, "GET /v1/items");
    fs::write(root.join("main.py"), main("/v2")).unwrap();
    engine.sync_paths(&["main.py".into()]).unwrap();
    let second = engine.node(id, 65536).unwrap();
    assert_eq!(second.node.unwrap().name, "GET /v2/items");
    assert!(second.generation > first.generation);
    assert!(
        engine
            .search("kind:route name:\"/v1/\"", 10, 65536)
            .unwrap()
            .results
            .is_empty()
    );
    assert_eq!(
        engine
            .search("kind:route name:\"/v2/\"", 10, 65536)
            .unwrap()
            .results[0]
            .id,
        id
    );
    fs::write(
        root.join("main.py"),
        "from fastapi import FastAPI\napp=FastAPI()\n",
    )
    .unwrap();
    engine.sync_paths(&["main.py".into()]).unwrap();
    assert_eq!(
        engine.node(id, 65536).unwrap().node.unwrap().name,
        "GET /items"
    );
    drop(engine);
    let engine = Engine::open(&root, Some(&database)).unwrap();
    assert_eq!(
        engine.node(id, 65536).unwrap().node.unwrap().name,
        "GET /items"
    );
}
