use prometeu_extract::{Edge, Node};
use prometeu_search::hierarchy::{build_type_hierarchy, can_have_hierarchy, count_implementers};
use serde_json::Value;

#[test]
fn hierarchy_records_match_independent_indexed_and_bounded_graphs() {
    let corpus: Value =
        serde_json::from_str(include_str!("fixtures/upstream/hierarchy/cases.json")).unwrap();
    let mut differences = Vec::new();
    for case in corpus["projects"]
        .as_array()
        .unwrap()
        .iter()
        .chain(corpus["snapshots"].as_array().unwrap())
    {
        let nodes: Vec<Node> = serde_json::from_value(case["nodes"].clone()).unwrap();
        let edges: Vec<Edge> = serde_json::from_value(case["edges"].clone()).unwrap();
        for query in case["queries"].as_array().unwrap() {
            let id = query["id"].as_str().unwrap();
            let node = nodes.iter().find(|node| node.id == id).unwrap();
            assert_eq!(
                can_have_hierarchy(node),
                query["eligible"].as_bool().unwrap()
            );
            assert_eq!(
                count_implementers(&edges, id),
                query["count"].as_u64().unwrap() as usize
            );
            for (overrides, name) in [(true, "hierarchy"), (false, "withoutOverrides")] {
                let actual = build_type_hierarchy(&nodes, &edges, id, overrides).unwrap();
                let expected: Option<prometeu_search::hierarchy::TypeHierarchy> =
                    serde_json::from_value(query[name].clone()).unwrap();
                if actual != expected {
                    differences.push(serde_json::json!({"case":case["id"],"query":id,"overrides":overrides,"actual":actual,"expected":expected}));
                }
            }
        }
    }
    if !differences.is_empty() {
        std::fs::write(
            std::env::temp_dir().join("prometeu-hierarchy-differences.json"),
            serde_json::to_string_pretty(&differences).unwrap(),
        )
        .unwrap();
    }
    assert!(
        differences.is_empty(),
        "{} hierarchy answers differ; see temporary prometeu-hierarchy-differences.json",
        differences.len()
    );
}

fn clear_times(hierarchy: &mut Option<prometeu_search::hierarchy::TypeHierarchy>) {
    if let Some(hierarchy) = hierarchy {
        hierarchy.focus.updated_at = 0;
        for entry in hierarchy
            .ancestors
            .iter_mut()
            .chain(&mut hierarchy.descendants)
        {
            entry.node.updated_at = 0;
        }
    }
}

#[test]
fn engine_hierarchies_match_indexed_reference_before_and_after_reopening() {
    let corpus: Value =
        serde_json::from_str(include_str!("fixtures/upstream/hierarchy/cases.json")).unwrap();
    let mut differences = Vec::new();
    for case in corpus["projects"].as_array().unwrap() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("project");
        let database = temporary.path().join("cache.sqlite");
        for (path, source) in case["files"].as_object().unwrap() {
            let path = root.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, source.as_str().unwrap()).unwrap();
        }
        let mut engine = prometeu_search::Engine::open(&root, Some(&database)).unwrap();
        engine.sync().unwrap();
        for reopen in [false, true] {
            if reopen {
                drop(engine);
                for path in case["files"].as_object().unwrap().keys() {
                    std::fs::write(root.join(path), "changed live source").unwrap();
                }
                engine = prometeu_search::Engine::open(&root, Some(&database)).unwrap();
            }
            for query in case["queries"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|query| query["eligible"] == true)
            {
                let node = case["nodes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|node| node["id"] == query["id"])
                    .unwrap();
                let hit = engine
                    .search(node["name"].as_str().unwrap(), 50, 65536)
                    .unwrap()
                    .results
                    .into_iter()
                    .find(|hit| {
                        hit.name == node["name"]
                            && hit.kind == node["kind"]
                            && hit.path == node["filePath"]
                            && hit.start_line == node["startLine"].as_u64().unwrap() as usize
                    })
                    .unwrap();
                let mut actual = engine.hierarchy(hit.id, 65536).unwrap().hierarchy;
                clear_times(&mut actual);
                let expected: Option<prometeu_search::hierarchy::TypeHierarchy> =
                    serde_json::from_value(query["hierarchy"].clone()).unwrap();
                if actual != expected {
                    differences.push(serde_json::json!({"case":case["id"],"query":node["name"],"reopen":reopen,"actual":actual,"expected":expected}));
                }
            }
        }
    }
    if !differences.is_empty() {
        std::fs::write(
            std::env::temp_dir().join("prometeu-hierarchy-index-differences.json"),
            serde_json::to_string_pretty(&differences).unwrap(),
        )
        .unwrap();
    }
    assert!(
        differences.is_empty(),
        "{} indexed hierarchy differences; see temporary prometeu-hierarchy-index-differences.json",
        differences.len()
    );
}

#[test]
fn hierarchy_generations_budgets_and_cli_remain_consistent() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("project");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("base.ts"), "export class Base { run() {} }\n").unwrap();
    std::fs::write(
        root.join("child.ts"),
        "import { Base } from './base';\nexport class Child extends Base { run() {} }\n",
    )
    .unwrap();
    let database = temporary.path().join("index.sqlite");
    let mut writer = prometeu_search::Engine::open(&root, Some(&database)).unwrap();
    writer.sync().unwrap();
    let observer = prometeu_search::Engine::open(&root, Some(&database)).unwrap();
    let id = observer
        .search("Base", 10, 65536)
        .unwrap()
        .results
        .iter()
        .find(|hit| hit.name == "Base" && hit.kind == "class")
        .unwrap()
        .id;
    let before = observer.hierarchy(id, 65536).unwrap();
    assert_eq!(before.hierarchy.as_ref().unwrap().descendants.len(), 1);
    let bounded = observer.hierarchy(id, 1024).unwrap();
    assert!(bounded.truncated);
    assert!(serde_json::to_vec(&bounded).unwrap().len() <= 1024);
    std::fs::write(root.join("child.ts"), "export class Child { run() {} }\n").unwrap();
    assert_eq!(
        observer.hierarchy(id, 65536).unwrap().generation,
        before.generation
    );
    writer.sync_paths(&["child.ts".into()]).unwrap();
    let after = observer.hierarchy(id, 65536).unwrap();
    assert!(after.generation > before.generation);
    assert!(after.hierarchy.is_none());
    assert!(observer.hierarchy(0, 12000).is_err());
    assert!(observer.hierarchy(id, 100).is_err());
}
