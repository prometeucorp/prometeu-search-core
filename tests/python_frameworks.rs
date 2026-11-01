use prometeu_search::{Engine, GraphOptions};
use serde_json::Value;
use std::{collections::HashMap, fs};

#[test]
fn python_framework_winners_match_indexed_reference_after_reopen() {
    let corpus: Value = serde_json::from_str(include_str!(
        "fixtures/upstream/python-frameworks/cases.json"
    ))
    .unwrap();
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
            let reverse: HashMap<_, _> =
                ids.iter().map(|(canonical, id)| (*id, canonical)).collect();
            let nodes: HashMap<_, _> = project["nodes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|node| (node["id"].as_str().unwrap(), node))
                .collect();
            for source in nodes.values().filter(|node| {
                matches!(
                    node["kind"].as_str(),
                    Some("file" | "function" | "method" | "class")
                )
            }) {
                let canonical = source["id"].as_str().unwrap();
                let id = ids[canonical];
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
                let relevant = |name: &str| {
                    name == "_iterable_class"
                        || name == "router"
                        || name.ends_with("Model")
                        || name.ends_with("View")
                        || name.ends_with("Form")
                        || name.ends_with("_bp")
                        || name.ends_with("_blueprint")
                        || name.starts_with("get_")
                };
                let mut actual: Vec<_> = graph
                    .edges
                    .iter()
                    .filter(|edge| {
                        edge.from == id
                            && edge.to.is_some()
                            && edge
                                .metadata
                                .as_ref()
                                .and_then(|metadata| metadata["refName"].as_str())
                                .is_some_and(relevant)
                    })
                    .map(|edge| {
                        (
                            edge.kind.clone(),
                            reverse[&edge.to.unwrap()].as_str().to_owned(),
                            edge.line,
                            edge.column,
                            edge.metadata.clone().unwrap(),
                        )
                    })
                    .collect();
                let mut expected: Vec<_> = project["edges"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|edge| {
                        edge["source"] == canonical
                            && edge["metadata"]["refName"].as_str().is_some_and(relevant)
                    })
                    .map(|edge| {
                        (
                            edge["kind"].as_str().unwrap().to_owned(),
                            edge["target"].as_str().unwrap().to_owned(),
                            edge["line"].as_u64().unwrap() as usize,
                            edge["column"].as_u64().map(|column| column as u32),
                            edge["metadata"].clone(),
                        )
                    })
                    .collect();
                actual.sort_by(|a, b| (&a.0, &a.1, a.2, a.3).cmp(&(&b.0, &b.1, b.2, b.3)));
                expected.sort_by(|a, b| (&a.0, &a.1, a.2, a.3).cmp(&(&b.0, &b.1, b.2, b.3)));
                assert_eq!(
                    actual, expected,
                    "{} {canonical} {:?}",
                    project["name"], graph.edges
                );
            }
        }
    }
}
