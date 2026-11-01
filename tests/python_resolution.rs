use prometeu_search::{Engine, GraphOptions};
use serde_json::Value;
use std::fs;

#[test]
fn typed_factory_answers_follow_committed_sources_and_dependency_edits() {
    let corpus: Value = serde_json::from_str(include_str!(
        "fixtures/upstream/resolution/python/cases.json"
    ))
    .unwrap();
    let projects = corpus["projects"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|project| {
            project["name"]
                .as_str()
                .unwrap()
                .ends_with("re-types the local when the factory changes what it returns")
        })
        .collect::<Vec<_>>();
    assert_eq!(projects.len(), 2);
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("project");
    fs::create_dir(&root).unwrap();
    let database = temporary.path().join("index.sqlite");
    for (path, source) in projects[0]["files"].as_object().unwrap() {
        fs::write(root.join(path), source.as_str().unwrap()).unwrap();
    }
    let mut engine = Engine::open(&root, Some(&database)).unwrap();
    engine.sync().unwrap();
    let expected = |project: &Value| {
        let edge = project["edges"]
            .as_array()
            .unwrap()
            .iter()
            .find(|edge| {
                edge["metadata"]["refName"] == "reg.get" && edge["metadata"]["typeFrom"].is_array()
            })
            .unwrap();
        let target = project["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|node| node["id"] == edge["target"])
            .unwrap();
        (
            target["qualifiedName"].as_str().unwrap().to_owned(),
            edge["metadata"].clone(),
            edge["line"].as_u64().unwrap() as usize,
            edge["column"].as_u64().unwrap() as u32,
        )
    };
    let query = |engine: &Engine| {
        let graph = engine
            .explore(
                "factory",
                &GraphOptions {
                    path: Some("use.py".into()),
                    depth: 1,
                    limit: 50,
                    max_bytes: 65536,
                    ..Default::default()
                },
            )
            .unwrap();
        let answers = graph
            .edges
            .iter()
            .filter(|edge| {
                edge.metadata
                    .as_ref()
                    .is_some_and(|metadata| metadata["refName"] == "reg.get")
            })
            .filter_map(|edge| {
                let target = edge.to?;
                let target = graph
                    .files
                    .iter()
                    .flat_map(|file| &file.symbols)
                    .find(|symbol| symbol.id == target)
                    .unwrap();
                Some((
                    target.qualified_name.clone(),
                    edge.metadata.clone().unwrap(),
                    edge.line,
                    edge.column.unwrap(),
                ))
            })
            .collect::<Vec<_>>();
        (graph.generation, answers)
    };
    let (generation, answers) = query(&engine);
    assert_eq!(answers, vec![expected(projects[0])]);
    fs::write(
        root.join("loader.py"),
        projects[1]["files"]["loader.py"].as_str().unwrap(),
    )
    .unwrap();
    assert_eq!(query(&engine), (generation, answers.clone()));
    drop(engine);
    engine = Engine::open(&root, Some(&database)).unwrap();
    assert_eq!(query(&engine), (generation, answers));
    engine.sync_paths(&["loader.py".into()]).unwrap();
    let (changed, answers) = query(&engine);
    assert!(changed > generation);
    assert_eq!(answers, vec![expected(projects[1])]);
    drop(engine);
    engine = Engine::open(&root, Some(&database)).unwrap();
    assert_eq!(query(&engine), (changed, answers));
    fs::remove_file(root.join("loader.py")).unwrap();
    engine.sync_paths(&["loader.py".into()]).unwrap();
    assert!(query(&engine).1.is_empty());
}
