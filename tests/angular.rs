use prometeu_search::{Engine, GraphOptions};
use serde_json::Value;
use std::{collections::HashMap, fs};
#[test]
fn angular_routes_and_navigation_match_indexed_reference_after_reopen() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/upstream/angular/projects.json")).unwrap();
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
            let connection = rusqlite::Connection::open(&database).unwrap();
            let mut query = connection
                .prepare(
                    "SELECT s.id,s.ordinal,f.analysis FROM symbols s JOIN files f ON s.path=f.path",
                )
                .unwrap();
            let mut ids = HashMap::new();
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
                    ids.insert(canonical.to_owned(), id);
                }
            }
            let nodes: HashMap<_, _> = project["nodes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|node| (node["id"].as_str().unwrap(), node))
                .collect();
            let routes = engine.search("kind:route", 50, 65536).unwrap();
            assert!(!routes.truncated);
            let expected: Vec<_> = nodes
                .values()
                .filter(|node| node["kind"] == "route")
                .collect();
            assert_eq!(routes.results.len(), expected.len());
            for expected in expected {
                let id = ids[expected["id"].as_str().unwrap()];
                let actual = engine.node(id, 65536).unwrap().node.unwrap();
                assert_eq!(
                    actual.name, expected["name"],
                    "{} {}",
                    project["id"], expected["id"]
                );
            }
            for expected in project["edges"].as_array().unwrap().iter().filter(|edge| {
                edge["metadata"]["framework"] == "angular-router"
                    || edge["metadata"]["synthesizedBy"]
                        .as_str()
                        .is_some_and(|name| name.starts_with("angular-"))
            }) {
                let source = nodes[expected["source"].as_str().unwrap()];
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
                assert!(
                    graph.edges.iter().any(|edge| edge.from
                        == ids[expected["source"].as_str().unwrap()]
                        && edge.to == Some(ids[expected["target"].as_str().unwrap()])
                        && edge.kind == expected["kind"]
                        && edge.metadata.as_ref() == Some(&expected["metadata"])),
                    "{} missing {expected:?}: {:?}",
                    project["id"],
                    graph.edges
                );
            }
        }
    }
}

fn template_targets(graph: &prometeu_search::GraphResult) -> Vec<String> {
    let mut targets = graph
        .edges
        .iter()
        .filter(|edge| {
            edge.metadata
                .as_ref()
                .and_then(|metadata| metadata["synthesizedBy"].as_str())
                .is_some_and(|name| name.starts_with("angular-"))
        })
        .map(|edge| edge.target.clone())
        .collect::<Vec<_>>();
    targets.sort();
    targets
}
#[test]
fn external_template_updates_commit_atomically_and_reopen_keeps_snapshot() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("project");
    fs::create_dir(&root).unwrap();
    let database = temporary.path().join("index.sqlite");
    fs::write(
        root.join("package.json"),
        r#"{"dependencies":{"@angular/core":"*","@angular/router":"*"}}"#,
    )
    .unwrap();
    fs::write(root.join("routes.ts"),"import {Routes} from '@angular/router'; import {Page} from './page'; export const routes:Routes=[{path:'first',component:Page},{path:'second',component:Page}];").unwrap();
    fs::write(root.join("page.ts"),"import {Component} from '@angular/core';\n@Component({selector:'app-page',templateUrl:'./page.html'})\nexport class Page {\n save() {}\n icon() {return '';}\n}\n").unwrap();
    let template = root.join("page.html");
    fs::write(
        &template,
        "<button (click)=\"save()\">Save</button><a routerLink=\"/first\">First</a>",
    )
    .unwrap();
    let mut engine = Engine::open(&root, Some(&database)).unwrap();
    engine.sync().unwrap();
    let options = GraphOptions {
        path: Some("page.ts".into()),
        line: Some(3),
        depth: 1,
        limit: 50,
        max_bytes: 65536,
        ..Default::default()
    };
    let before = engine.explore("Page", &options).unwrap();
    assert_eq!(template_targets(&before), ["/first", "save"]);
    fs::write(
        &template,
        "<i [title]=\"icon()\"></i><a routerLink=\"/second\">Second</a>",
    )
    .unwrap();
    assert_eq!(
        template_targets(&engine.explore("Page", &options).unwrap()),
        ["/first", "save"]
    );
    engine.sync_paths(&["page.html".into()]).unwrap();
    let after = engine.explore("Page", &options).unwrap();
    assert!(after.generation > before.generation);
    assert_eq!(template_targets(&after), ["/second", "icon"]);
    drop(engine);
    fs::write(&template, "<button (click)=\"save()\">Save</button>").unwrap();
    let mut engine = Engine::open(&root, Some(&database)).unwrap();
    assert_eq!(
        template_targets(&engine.explore("Page", &options).unwrap()),
        ["/second", "icon"]
    );
    fs::write(&template, vec![b' '; 1024 * 1024 + 1]).unwrap();
    assert!(engine.sync_paths(&["page.html".into()]).is_err());
    let preserved = engine.explore("Page", &options).unwrap();
    assert_eq!(preserved.generation, after.generation);
    assert_eq!(template_targets(&preserved), ["/second", "icon"]);
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        fs::remove_file(&template).unwrap();
        let outside = temporary.path().join("outside.html");
        fs::write(&outside, "<button (click)=\"save()\">Save</button>").unwrap();
        symlink(outside, &template).unwrap();
        assert!(engine.sync().is_err());
        assert_eq!(
            template_targets(&engine.explore("Page", &options).unwrap()),
            ["/second", "icon"]
        );
    }
    fs::remove_file(&template).unwrap();
    engine.sync_paths(&["page.html".into()]).unwrap();
    assert!(template_targets(&engine.explore("Page", &options).unwrap()).is_empty());
}
