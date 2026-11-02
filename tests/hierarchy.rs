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
