use prometeu_extract::{Edge, Node};
use prometeu_search::hierarchy::build_type_hierarchy;
use serde_json::json;

#[test]
fn preferred_extends_edge_keeps_its_actual_parent() {
    let nodes: Vec<Node> = ["root", "a", "b", "child"]
        .into_iter()
        .map(|id| {
            serde_json::from_value(json!({
                "id": id, "name": id, "qualifiedName": id, "kind": "class",
                "filePath": "types.ts", "language": "typescript",
                "startLine": 1, "endLine": 1, "startColumn": 0, "endColumn": 1,
                "updatedAt": 0
            }))
            .unwrap()
        })
        .collect();
    let edges: Vec<Edge> = serde_json::from_value(json!([
        {"source":"a", "target":"root", "kind":"extends"},
        {"source":"b", "target":"root", "kind":"extends"},
        {"source":"child", "target":"a", "kind":"implements"},
        {"source":"child", "target":"b", "kind":"extends", "provenance":"heuristic"}
    ]))
    .unwrap();
    let hierarchy = build_type_hierarchy(&nodes, &edges, "root", false)
        .unwrap()
        .unwrap();
    let child = hierarchy
        .descendants
        .iter()
        .find(|entry| entry.node.id == "child")
        .unwrap();
    assert_eq!(child.parent_id, "b");
    assert_eq!(child.parent_id, child.edge.target);
    assert_eq!(child.relation, "extends");
    assert_eq!(child.depth, 2);
    assert!(child.synthesized);
}

#[test]
fn omitted_subtypes_are_counted_once_at_size_and_depth_bounds() {
    use prometeu_search::hierarchy::{MAX_DESCENDANT_DEPTH, MAX_DESCENDANTS};
    let node = |id: &str| -> Node {
        serde_json::from_value(json!({
            "id": id, "name": id, "qualifiedName": id, "kind": "class",
            "filePath": "types.ts", "language": "typescript",
            "startLine": 1, "endLine": 1, "startColumn": 0, "endColumn": 1,
            "updatedAt": 0
        }))
        .unwrap()
    };
    let edge = |source: &str, target: &str| -> Edge {
        serde_json::from_value(json!({"source":source, "target":target, "kind":"implements"}))
            .unwrap()
    };
    for nested in [false, true] {
        let mut nodes = vec![node("root")];
        let mut edges = Vec::new();
        let parent = if nested {
            nodes.push(node("parent"));
            edges.push(edge("parent", "root"));
            "parent"
        } else {
            "root"
        };
        for id in 0..=MAX_DESCENDANTS {
            let id = format!("child{id:04}");
            nodes.push(node(&id));
            edges.extend([edge(&id, parent), edge(&id, parent)]);
        }
        let hierarchy = build_type_hierarchy(&nodes, &edges, "root", false)
            .unwrap()
            .unwrap();
        assert!(hierarchy.bounded);
        assert_eq!(hierarchy.descendants.len(), MAX_DESCENDANTS);
        if nested {
            assert_eq!(hierarchy.direct_subtypes, 1);
            assert_eq!(
                hierarchy
                    .descendants
                    .iter()
                    .find(|entry| entry.node.id == "parent")
                    .unwrap()
                    .hidden_subtypes,
                2
            );
        } else {
            assert_eq!(hierarchy.direct_subtypes, MAX_DESCENDANTS + 1);
            assert_eq!(hierarchy.direct_implementers, MAX_DESCENDANTS + 1);
        }
    }
    let nodes: Vec<_> = (0..=MAX_DESCENDANT_DEPTH + 1)
        .map(|id| node(&format!("level{id}")))
        .collect();
    let edges: Vec<_> = (1..=MAX_DESCENDANT_DEPTH + 1)
        .flat_map(|id| {
            let edge = edge(&format!("level{id}"), &format!("level{}", id - 1));
            [edge.clone(), edge]
        })
        .collect();
    let hierarchy = build_type_hierarchy(&nodes, &edges, "level0", false)
        .unwrap()
        .unwrap();
    assert!(hierarchy.bounded);
    assert_eq!(hierarchy.descendants.len(), MAX_DESCENDANT_DEPTH);
    assert_eq!(hierarchy.descendants.last().unwrap().hidden_subtypes, 1);
}
