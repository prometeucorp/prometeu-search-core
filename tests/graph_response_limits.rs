use prometeu_search::{GraphOperation, GraphQueryResult};
use serde_json::json;
use std::collections::HashSet;

#[test]
fn trimming_computed_graphs_preserves_references_and_utf16_capacity() {
    let mut graph = GraphQueryResult {
        generation: 7,
        operation: GraphOperation::Bfs,
        data: json!({
            "nodes": (0..8).map(|id| json!({"id":id.to_string(),"name":"x".repeat(700)})).collect::<Vec<_>>(),
            "edges": (0..7).map(|id| json!({"source":id.to_string(),"target":(id+1).to_string()})).collect::<Vec<_>>(),
            "roots": ["0", "7"]
        }),
        ambiguous: false,
        truncated: false,
    };
    graph.limit_utf16(2048).unwrap();
    assert!(
        serde_json::to_string(&graph)
            .unwrap()
            .encode_utf16()
            .count()
            <= 2048
    );
    assert!(graph.truncated);
    let ids: HashSet<_> = graph.data["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|node| node["id"].as_str().unwrap())
        .collect();
    assert!(!ids.is_empty());
    for edge in graph.data["edges"].as_array().unwrap() {
        assert!(ids.contains(edge["source"].as_str().unwrap()));
        assert!(ids.contains(edge["target"].as_str().unwrap()));
    }
    for root in graph.data["roots"].as_array().unwrap() {
        assert!(ids.contains(root.as_str().unwrap()));
    }

    let mut unicode = GraphQueryResult {
        generation: 7,
        operation: GraphOperation::Modules,
        data: json!({"nodes": ["😀".repeat(300)]}),
        ambiguous: false,
        truncated: false,
    };
    let before = serde_json::to_string(&unicode).unwrap();
    assert!(before.len() > 1024 && before.encode_utf16().count() < 1024);
    unicode.limit_utf16(1024).unwrap();
    assert_eq!(serde_json::to_string(&unicode).unwrap(), before);
}
