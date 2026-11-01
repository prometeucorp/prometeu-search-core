use prometeu_search::{Engine, GraphOptions};
use serde_json::Value;
use std::{collections::HashMap, fs};

#[test]
fn controller_routes_match_reference_after_reopen() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/upstream/codeigniter/cases.json")).unwrap();
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
                        edge.to
                            .and_then(|to| ids.get(&to))
                            .map(|to| (edge.kind.clone(), to.clone()))
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
fn view_arrival_removal_and_loader_changes_invalidate_cached_edges() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("project");
    let database = temporary.path().join("index.sqlite");
    let controller = "application/controllers/Home.php";
    let view = "application/views/home.php";
    fs::create_dir_all(root.join("application/config")).unwrap();
    fs::create_dir_all(root.join("application/controllers")).unwrap();
    fs::create_dir_all(root.join("application/views")).unwrap();
    fs::write(
        root.join("application/config/routes.php"),
        "<?php $route['default_controller']='home';",
    )
    .unwrap();
    let source =
        "<?php\nclass Home {\n public function index() { $this->load->view('home'); }\n}\n";
    fs::write(root.join(controller), source).unwrap();
    let mut writer = Engine::open(&root, Some(&database)).unwrap();
    writer.sync().unwrap();
    let observer = Engine::open(&root, Some(&database)).unwrap();
    let targets = |engine: &Engine| {
        let graph = engine
            .explore(
                "index",
                &GraphOptions {
                    path: Some(controller.into()),
                    depth: 1,
                    limit: 50,
                    max_bytes: 65536,
                    ..Default::default()
                },
            )
            .unwrap();
        graph
            .edges
            .iter()
            .filter(|edge| {
                edge.kind == "calls"
                    && edge
                        .metadata
                        .as_ref()
                        .is_some_and(|metadata| metadata["framework"] == "codeigniter")
            })
            .map(|edge| {
                engine
                    .node(edge.to.unwrap(), 65536)
                    .unwrap()
                    .node
                    .unwrap()
                    .metadata
                    .unwrap()
                    .file_path
            })
            .collect::<Vec<_>>()
    };
    assert!(targets(&observer).is_empty());
    fs::write(root.join(view), "<?php echo 'home';").unwrap();
    writer.sync_paths(&[view.into()]).unwrap();
    assert_eq!(targets(&observer), [view]);
    fs::write(root.join(controller), source.replace("'home'", "'missing'")).unwrap();
    assert_eq!(targets(&observer), [view]);
    writer.sync_paths(&[controller.into()]).unwrap();
    assert!(targets(&observer).is_empty());
    fs::write(root.join(controller), source).unwrap();
    writer.sync_paths(&[controller.into()]).unwrap();
    assert_eq!(targets(&observer), [view]);
    fs::remove_file(root.join(view)).unwrap();
    writer.sync_paths(&[view.into()]).unwrap();
    assert!(targets(&observer).is_empty());
    drop(observer);
    assert!(targets(&Engine::open(&root, Some(&database)).unwrap()).is_empty());
}
