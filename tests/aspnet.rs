use prometeu_search::{Direction, Engine};
use serde_json::Value;
use std::{collections::BTreeMap, fs, path::Path};
use tempfile::TempDir;

fn corpus() -> Value {
    serde_json::from_str(include_str!(
        "fixtures/upstream/frameworks/aspnet-projects.json"
    ))
    .unwrap()
}
fn write(root: &Path, files: &serde_json::Map<String, Value>) {
    for (path, source) in files {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source.as_str().unwrap()).unwrap();
    }
}
fn nodes(engine: &Engine) -> BTreeMap<String, (i64, prometeu_extract::Node)> {
    let hits = engine.search("kind:route", 50, 65536).unwrap();
    assert!(!hits.truncated);
    hits.results
        .into_iter()
        .map(|hit| {
            let node = engine
                .node(hit.id, 65536)
                .unwrap()
                .node
                .unwrap()
                .metadata
                .unwrap();
            (node.id.clone(), (hit.id, node))
        })
        .collect()
}
fn stored(mut node: Value) -> Value {
    node.as_object_mut().unwrap().remove("updatedAt");
    for (key, value) in [
        ("visibility", Value::Null),
        ("isExported", false.into()),
        ("isAsync", false.into()),
        ("isStatic", false.into()),
        ("isAbstract", false.into()),
    ] {
        node.as_object_mut().unwrap().entry(key).or_insert(value);
    }
    node
}

#[test]
fn persistent_routes_and_handler_targets_match_real_reference_projects() {
    let corpus = corpus();
    assert_eq!(
        corpus["revision"],
        "6aa358ccbae30fbf27318f79ee183212f726f9ba"
    );
    for project in corpus["projects"].as_array().unwrap() {
        let temporary = TempDir::new().unwrap();
        let root = temporary.path().join("project");
        let database = temporary.path().join("index.sqlite");
        write(&root, project["files"].as_object().unwrap());
        let mut engine = Engine::open(&root, Some(&database)).unwrap();
        engine.sync().unwrap();
        for reopened in [false, true] {
            if reopened {
                drop(engine);
                for path in project["files"].as_object().unwrap().keys() {
                    fs::write(root.join(path), "changed outside snapshot").unwrap();
                }
                engine = Engine::open(&root, Some(&database)).unwrap();
            }
            let actual = nodes(&engine);
            assert_eq!(
                actual.len(),
                project["routes"].as_array().unwrap().len(),
                "{}",
                project["id"]
            );
            for route in project["routes"].as_array().unwrap() {
                let (id, node) = &actual[route["node"]["id"].as_str().unwrap()];
                assert_eq!(
                    stored(serde_json::to_value(node).unwrap()),
                    route["node"],
                    "{}",
                    project["id"]
                );
                let source = engine.node(*id, 65536).unwrap().node.unwrap();
                assert!(!source.source_truncated);
                let related = engine
                    .related(&node.qualified_name, Direction::Callees, 50, 65536)
                    .unwrap();
                assert!(!related.truncated);
                let mut targets: Vec<_> = related
                    .relations
                    .iter()
                    .filter_map(|relation| relation.candidate.as_ref())
                    .map(|candidate| {
                        engine
                            .node(candidate.id, 65536)
                            .unwrap()
                            .node
                            .unwrap()
                            .metadata
                            .unwrap()
                            .id
                    })
                    .collect();
                targets.sort();
                targets.dedup();
                let mut expected: Vec<_> = route["targets"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|target| target["node"]["id"].as_str().unwrap().to_owned())
                    .collect();
                expected.sort();
                expected.dedup();
                assert_eq!(targets, expected, "{} {}", project["id"], node.name);
            }
        }
    }
}

#[test]
fn cross_file_route_renames_commit_without_replacing_route_ids() {
    let corpus = corpus();
    for (project_name, path, source, old, new) in [
        (
            "constant-variants",
            "Request.cs",
            "public struct Request { public static readonly string Route = \"changed/\"; }",
            "POST /items",
            "POST /changed",
        ),
        (
            "group-prefix-and-override",
            "Extensions.cs",
            "var group = app.MapGroup($\"/changed/{groupName}\");",
            "GET /api/Todos",
            "GET /changed/Todos",
        ),
        (
            "group-prefix-and-override",
            "Extensions.cs",
            "",
            "GET /api/Todos",
            "GET /Todos",
        ),
        (
            "group-prefix-and-override",
            "Extensions.cs",
            "var group = app.MapGroup($\"/{groupName}\");",
            "GET /api/Todos",
            "GET /Todos",
        ),
        (
            "group-prefix-and-override",
            "Extensions.cs",
            "var group = app.MapGroup($\"{groupName}\");",
            "GET /api/Todos",
            "GET /Todos",
        ),
    ] {
        let project = corpus["projects"]
            .as_array()
            .unwrap()
            .iter()
            .find(|project| project["id"] == project_name)
            .unwrap();
        let temporary = TempDir::new().unwrap();
        let root = temporary.path().join("project");
        let database = temporary.path().join("index.sqlite");
        write(&root, project["files"].as_object().unwrap());
        let mut engine = Engine::open(&root, Some(&database)).unwrap();
        let first = engine.sync().unwrap();
        let before = nodes(&engine);
        let (id, node) = before.values().find(|(_, node)| node.name == old).unwrap();
        fs::write(root.join(path), source).unwrap();
        let report = engine.sync_paths(&[path.into()]).unwrap();
        assert_eq!(report.indexed, 1);
        assert!(report.generation > first.generation);
        let after = nodes(&engine);
        assert_eq!(after[&node.id].0, *id);
        assert_eq!(after[&node.id].1.name, new);
        assert_eq!(engine.node(*id, 65536).unwrap().node.unwrap().name, new);
        assert!(
            engine
                .search(&format!("kind:route name:\"{new}\""), 50, 65536)
                .unwrap()
                .results
                .iter()
                .any(|hit| hit.id == *id)
        );
        assert_eq!(
            engine.sync().unwrap().generation,
            report.generation,
            "finalization must be idempotent"
        );
        drop(engine);
        let reopened = Engine::open(&root, Some(&database)).unwrap();
        assert_eq!(reopened.node(*id, 65536).unwrap().node.unwrap().name, new);
        let mut fresh = Engine::open(&root, Some(&temporary.path().join("fresh.sqlite"))).unwrap();
        fresh.sync().unwrap();
        let names = |engine: &Engine| {
            nodes(engine)
                .into_iter()
                .map(|(id, (_, node))| (id, node.name))
                .collect::<BTreeMap<_, _>>()
        };
        assert_eq!(names(&reopened), names(&fresh));
    }
}
