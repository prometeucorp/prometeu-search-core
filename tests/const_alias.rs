use prometeu_search::{Engine, GraphOptions};
use rusqlite::Connection;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;

#[test]
fn stored_alias_links_match_complete_reference_edges_after_reopen() {
    let corpus: Value =
        serde_json::from_str(include_str!("fixtures/upstream/synthesis/const-alias.json")).unwrap();
    for case in corpus["cases"].as_array().unwrap() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("project");
        fs::create_dir(&root).unwrap();
        let database = temporary.path().join("index.sqlite");
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
        let mut ids = HashMap::new();
        for row in statement
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
            if let Some(node) = analysis["extraction"]["nodes"].get(ordinal as usize) {
                ids.insert(id, node.clone());
            }
        }
        let expected = case["expected"].as_array().unwrap();
        for source in case["nodes"].as_array().unwrap().iter().filter(|node| {
            matches!(node["kind"].as_str(), Some("constant" | "variable"))
                && node["name"] == node["qualifiedName"]
        }) {
            let options = GraphOptions {
                path: Some(source["filePath"].as_str().unwrap().to_owned()),
                line: Some(source["startLine"].as_u64().unwrap() as usize),
                depth: 1,
                limit: 50,
                max_bytes: 65536,
                ..Default::default()
            };
            let graph = engine
                .explore(source["name"].as_str().unwrap(), &options)
                .unwrap();
            let mut actual = Vec::new();
            for edge in graph.edges.iter().filter(|edge| {
                edge.metadata
                    .as_ref()
                    .is_some_and(|m| m["synthesizedBy"] == "const-alias")
                    && ids
                        .get(&edge.from)
                        .is_some_and(|node| node["id"] == source["id"])
            }) {
                let value = serde_json::json!({"source":ids[&edge.from]["id"],"target":ids[&edge.to.unwrap()]["id"],"kind":edge.kind,"line":edge.line,"provenance":edge.provenance,"metadata":edge.metadata});
                actual.push(value);
            }
            let mut wanted: Vec<_> = expected
                .iter()
                .filter(|edge| edge["source"] == source["id"])
                .cloned()
                .collect();
            actual.sort_by_key(Value::to_string);
            wanted.sort_by_key(Value::to_string);
            assert_eq!(actual, wanted, "{}: {}", case["id"], source["name"]);
        }
    }
}

#[test]
fn removing_alias_wiring_invalidates_a_cached_graph() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("project");
    fs::create_dir(&root).unwrap();
    let database = temporary.path().join("index.sqlite");
    let file = root.join("main.ts");
    fs::write(
        &file,
        "export function work() {}\nexport const alias=work;\n",
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
    let before = first.explore("alias", &options).unwrap();
    assert!(before.edges.iter().any(|e| {
        e.metadata
            .as_ref()
            .is_some_and(|m| m["synthesizedBy"] == "const-alias")
    }));
    fs::write(&file, "export function work() {}\nexport const alias=0;\n").unwrap();
    let mut second = Engine::open(&root, Some(&database)).unwrap();
    second.sync().unwrap();
    let after = first.explore("alias", &options).unwrap();
    assert!(after.generation > before.generation);
    assert!(!after.edges.iter().any(|e| {
        e.metadata
            .as_ref()
            .is_some_and(|m| m["synthesizedBy"] == "const-alias")
    }));
}
