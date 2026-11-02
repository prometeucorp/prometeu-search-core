use prometeu_extract::{Edge, Node};
use prometeu_search::graph_query::GraphSnapshot;
use serde_json::json;

#[test]
fn filters_visit_every_kind_once_and_keep_known_kind_order() {
    let nodes: Vec<Node> = [
        ("property", "property"),
        ("function", "function"),
        ("field", "field"),
        ("protocol", "protocol"),
        ("class", "class"),
        ("custom", "host_extension"),
        ("excluded", "file"),
    ]
    .into_iter()
    .map(|(id, kind)| {
        serde_json::from_value(json!({
            "id": id, "name": id, "qualifiedName": id, "kind": kind,
            "filePath": "types.ts", "language": "typescript",
            "startLine": 1, "endLine": 1, "startColumn": 0, "endColumn": 1,
            "updatedAt": 0
        }))
        .unwrap()
    })
    .collect();
    let edges: Vec<Edge> = serde_json::from_value(json!([
        {"source":"class", "target":"property", "kind":"contains"},
        {"source":"property", "target":"field", "kind":"references"},
        {"source":"protocol", "target":"excluded", "kind":"references"}
    ]))
    .unwrap();
    let snapshot = GraphSnapshot::new(&nodes, &edges).unwrap();
    let mut visited = Vec::new();
    let graph = snapshot.filtered(
        |node| {
            visited.push(node.id.clone());
            node.kind != "file"
        },
        true,
    );
    assert_eq!(
        visited,
        [
            "excluded", "class", "function", "property", "field", "protocol", "custom"
        ]
    );
    assert_eq!(
        graph
            .nodes
            .iter()
            .map(|node| node.id.as_str())
            .collect::<Vec<_>>(),
        [
            "class", "function", "property", "field", "protocol", "custom"
        ]
    );
    assert_eq!(graph.edges, edges[..2]);
    let properties = snapshot.filtered(|node| node.kind == "property", false);
    assert_eq!(properties.nodes.len(), 1);
    assert_eq!(properties.nodes[0].id, "property");
    assert!(properties.edges.is_empty());
}
