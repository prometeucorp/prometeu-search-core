use prometeu_search::{Engine, GraphOptions};
use rusqlite::Connection;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;

#[test]
fn stored_c_function_pointer_links_match_complete_reference_edges_after_reopen() {
    let corpus: Value =
        serde_json::from_str(include_str!("fixtures/upstream/synthesis/c-fnptr.json")).unwrap();
    for case in corpus["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|case| case.get("maxPerChannel").is_none() && case["oracleOnly"] != true)
    {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("project");
        fs::create_dir(&root).unwrap();
        let database = temporary.path().join("index.sqlite");
        for (path, source) in case["files"]
            .as_object()
            .unwrap()
            .iter()
            .chain(case["auxiliary"].as_object().unwrap())
        {
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
        for source in case["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|node| matches!(node["kind"].as_str(), Some("function" | "method")))
        {
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
                    .is_some_and(|m| m["synthesizedBy"] == "fn-pointer-dispatch")
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

fn callback_targets(result: &prometeu_search::GraphResult) -> Vec<String> {
    let mut names: Vec<_> = result
        .edges
        .iter()
        .filter(|edge| {
            edge.metadata
                .as_ref()
                .is_some_and(|m| m["synthesizedBy"] == "fn-pointer-dispatch")
        })
        .map(|edge| edge.target.clone())
        .collect();
    names.sort();
    names
}
#[test]
fn unindexed_include_changes_commit_atomically_and_queries_keep_the_snapshot() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("project");
    fs::create_dir(&root).unwrap();
    let database = temporary.path().join("index.sqlite");
    fs::write(root.join("main.c"),"struct Ops { int (*call)(void); };\nint first(void){return 1;}\nint second(void){return 2;}\n#include \"table.def\"\nint dispatch(struct Ops *o){return o->call();}\n").unwrap();
    let table = root.join("table.def");
    fs::write(&table, "static struct Ops ops = { first };\n").unwrap();
    let mut engine = Engine::open(&root, Some(&database)).unwrap();
    engine.sync().unwrap();
    let options = GraphOptions {
        depth: 1,
        limit: 50,
        max_bytes: 65536,
        ..Default::default()
    };
    let before = engine.explore("dispatch", &options).unwrap();
    assert_eq!(callback_targets(&before), ["first"]);
    fs::write(&table, "static struct Ops ops = { second };\n").unwrap();
    assert_eq!(
        callback_targets(&engine.explore("dispatch", &options).unwrap()),
        ["first"]
    );
    engine.sync_paths(&["table.def".into()]).unwrap();
    let after = engine.explore("dispatch", &options).unwrap();
    assert!(after.generation > before.generation);
    assert_eq!(callback_targets(&after), ["second"]);
    drop(engine);
    fs::write(&table, "static struct Ops ops = { first };\n").unwrap();
    let mut engine = Engine::open(&root, Some(&database)).unwrap();
    assert_eq!(
        callback_targets(&engine.explore("dispatch", &options).unwrap()),
        ["second"]
    );
    fs::write(&table, vec![b' '; 1024 * 1024 + 1]).unwrap();
    assert!(engine.sync_paths(&["table.def".into()]).is_err());
    let preserved = engine.explore("dispatch", &options).unwrap();
    assert_eq!(preserved.generation, after.generation);
    assert_eq!(callback_targets(&preserved), ["second"]);
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        fs::remove_file(&table).unwrap();
        let outside = temporary.path().join("outside.def");
        fs::write(&outside, "static struct Ops ops = { first };\n").unwrap();
        symlink(&outside, &table).unwrap();
        assert!(engine.sync().is_err());
        assert_eq!(
            callback_targets(&engine.explore("dispatch", &options).unwrap()),
            ["second"]
        );
    }
}
