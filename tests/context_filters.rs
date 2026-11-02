use prometeu_extract::{Edge, Node};
use prometeu_search::context::{ContextBuilder, ContextHit, ContextQuery, FindContextOptions};
use serde_json::json;
use std::collections::{BTreeMap, HashSet};

#[test]
fn requested_kinds_restrict_seeds_hierarchy_and_merged_edges() {
    let nodes: Vec<Node> = [
        ("Service", "class"),
        ("Base", "interface"),
        ("Parent", "class"),
        ("run", "function"),
        ("value", "property"),
        ("Hidden", "protocol"),
        ("Beyond", "class"),
    ]
    .into_iter()
    .map(|(id, kind)| {
        serde_json::from_value(json!({
            "id":id,"name":id,"qualifiedName":id,"kind":kind,
            "filePath":format!("{id}.ts"),"language":"typescript",
            "startLine":1,"endLine":1,"startColumn":0,"endColumn":1,"updatedAt":0
        }))
        .unwrap()
    })
    .collect();
    let edges: Vec<Edge> = serde_json::from_value(json!([
        {"source":"Service","target":"Base","kind":"extends"},
        {"source":"Base","target":"Parent","kind":"extends"},
        {"source":"Service","target":"run","kind":"calls"},
        {"source":"Service","target":"run","kind":"references"},
        {"source":"Service","target":"value","kind":"references"},
        {"source":"Hidden","target":"Service","kind":"extends"},
        {"source":"Beyond","target":"Hidden","kind":"extends"}
    ]))
    .unwrap();
    let sources = BTreeMap::new();
    let search = |request: &ContextQuery| {
        let kinds = match request {
            ContextQuery::Text { kinds, .. } | ContextQuery::ExactNames { kinds, .. } => kinds,
            ContextQuery::Substring { .. } => return Ok(vec![]),
        };
        Ok(
            if kinds.is_empty() || kinds.iter().any(|kind| kind == "class") {
                vec![ContextHit {
                    id: "Service".into(),
                    score: 1.0,
                }]
            } else {
                vec![]
            },
        )
    };
    let flag = |_: &str| false;
    let builder = ContextBuilder::new(&nodes, &edges, &sources, &search, &flag, &flag).unwrap();
    for (edge_kinds, node_kinds, expected_ids, expected_edges) in [
        (vec!["calls"], None, vec!["Service", "run"], 1),
        (vec!["extends"], Some(vec!["class"]), vec!["Service"], 0),
        (
            vec!["extends"],
            Some(vec!["class", "interface"]),
            vec!["Service", "Base", "Parent"],
            2,
        ),
        (vec!["calls"], Some(vec!["function"]), vec![], 0),
    ] {
        let options = FindContextOptions {
            edge_kinds: edge_kinds.into_iter().map(str::to_string).collect(),
            node_kinds: node_kinds.map(|kinds| kinds.into_iter().map(str::to_string).collect()),
            ..Default::default()
        };
        let graph = builder.find("Service", &options).unwrap();
        assert_eq!(
            graph
                .nodes
                .iter()
                .map(|node| node.id.as_str())
                .collect::<Vec<_>>(),
            expected_ids
        );
        assert_eq!(graph.edges.len(), expected_edges);
        let ids: HashSet<_> = graph.nodes.iter().map(|node| node.id.as_str()).collect();
        assert!(graph.roots.iter().all(|id| ids.contains(id.as_str())));
        assert!(graph.nodes.iter().all(|node| {
            options
                .node_kinds
                .as_ref()
                .is_none_or(|kinds| kinds.contains(&node.kind))
        }));
        assert!(
            graph
                .edges
                .iter()
                .all(|edge| options.edge_kinds.contains(&edge.kind)
                    && ids.contains(edge.source.as_str())
                    && ids.contains(edge.target.as_str()))
        );
    }
}
