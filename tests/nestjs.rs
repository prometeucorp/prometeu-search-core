use prometeu_search::{Engine, GraphOptions};
use serde_json::Value;
use std::{collections::HashMap, fs};

#[test]
fn nestjs_routes_mounts_and_edges_match_reference_after_reopen() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/upstream/nestjs/cases.json")).unwrap();
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
fn removing_bootstrap_prefix_and_version_restores_route_without_id_churn() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("project");
    let database = temporary.path().join("index.sqlite");
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("package.json"),
        r#"{"dependencies":{"@nestjs/common":"*"}}"#,
    )
    .unwrap();
    fs::write(root.join("src/main.ts"),"const app=NestFactory.create(AppModule); app.setGlobalPrefix('api'); app.enableVersioning({defaultVersion:'1'});").unwrap();
    fs::write(
        root.join("src/users.controller.ts"),
        "@Controller('users')\nexport class UsersController {\n@Get()\nlist() {}\n}\n",
    )
    .unwrap();
    let mut writer = Engine::open(&root, Some(&database)).unwrap();
    writer.sync().unwrap();
    let reader = Engine::open(&root, Some(&database)).unwrap();
    let route = |engine: &Engine| {
        engine
            .search("kind:route", 10, 65536)
            .unwrap()
            .results
            .remove(0)
    };
    let before = route(&reader);
    assert_eq!(before.name, "GET /api/v1/users");
    fs::write(
        root.join("src/main.ts"),
        "const app=NestFactory.create(AppModule);",
    )
    .unwrap();
    assert_eq!(route(&reader).name, before.name);
    writer.sync_paths(&["src/main.ts".into()]).unwrap();
    let after = route(&reader);
    assert_eq!(after.id, before.id);
    assert_eq!(after.name, "GET /users");
    assert!(
        reader
            .search("kind:route name:api", 10, 65536)
            .unwrap()
            .results
            .is_empty()
    );
    drop(reader);
    assert_eq!(
        route(&Engine::open(&root, Some(&database)).unwrap()).name,
        "GET /users"
    );
}
