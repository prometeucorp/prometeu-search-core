use super::*;
use serde_json::{Value, json};
#[test]
fn play_routes_and_overloads_match_reference() {
    let corpus: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/upstream/play/cases.json"
    ))
    .unwrap();
    for case in corpus["cases"].as_array().unwrap() {
        let result = extract(
            case["path"].as_str().unwrap(),
            case["source"].as_str().unwrap(),
        )
        .unwrap();
        let mut actual = json!({"nodes":result.nodes,"references":result.unresolved_references});
        for node in actual["nodes"].as_array_mut().unwrap() {
            node.as_object_mut().unwrap().remove("updatedAt");
        }
        assert_eq!(
            actual, case["expected"],
            "{} {}",
            case["path"], case["source"]
        );
    }
    for case in corpus["parameters"].as_array().unwrap() {
        let actual = java::parameters(case["signature"].as_str())
            .unwrap()
            .map(|parameters| {
                parameters
                    .into_iter()
                    .map(|parameter| json!([parameter.kind, parameter.varargs]))
                    .collect::<Vec<_>>()
            });
        let expected = case["expected"].as_array().map(|parameters| {
            parameters
                .iter()
                .map(|parameter| json!([parameter["type"], parameter["varargs"]]))
                .collect::<Vec<_>>()
        });
        assert_eq!(actual, expected, "{}", case["signature"]);
    }
    for case in corpus["splits"].as_array().unwrap() {
        assert_eq!(
            json!(arguments::split(case["source"].as_str().unwrap())),
            case["expected"]
        );
    }
    for case in corpus["parens"].as_array().unwrap() {
        let sources = BTreeMap::from([("test.java", case["source"].as_str().unwrap())]);
        let ctx = Context::new(&[], &sources, &[]).unwrap();
        assert_eq!(
            json!(
                arguments::list(
                    &ctx,
                    "test.java",
                    1,
                    case["column"].as_u64().unwrap() as u32,
                    case["name"].as_str().unwrap()
                )
                .unwrap()
            ),
            case["expected"],
            "{case}"
        );
    }
    for project in corpus["projects"].as_array().unwrap() {
        let mut nodes = project["nodes"].clone();
        for node in nodes.as_array_mut().unwrap() {
            node["updatedAt"] = json!(0)
        }
        let nodes: Vec<Node> = serde_json::from_value(nodes).unwrap();
        let edges: Vec<prometeu_extract::Edge> =
            serde_json::from_value(project["edges"].clone()).unwrap();
        let sources = project["files"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(path, source)| (path.as_str(), source.as_str().unwrap()))
            .collect();
        let ctx = Context::new(&nodes, &sources, &edges).unwrap();
        for query in project["arguments"].as_array().unwrap() {
            let reference: UnresolvedReference =
                serde_json::from_value(query["reference"].clone()).unwrap();
            assert_eq!(
                json!(
                    java::argument_type(&ctx, query["source"].as_str().unwrap(), &reference)
                        .unwrap()
                ),
                query["expected"],
                "{} argument {} at {}:{}",
                project["name"],
                query["source"],
                reference.file_path.as_deref().unwrap(),
                reference.line
            );
        }
        for query in project["overloads"].as_array().unwrap() {
            let reference: UnresolvedReference =
                serde_json::from_value(query["reference"].clone()).unwrap();
            let node = nodes
                .iter()
                .find(|node| node.id == query["target"])
                .unwrap();
            assert_eq!(
                ctx.retarget(node, &reference).unwrap().id,
                query["expected"],
                "{} overload {} at {}:{} to {}:{}",
                project["name"],
                reference.reference_name,
                reference.file_path.as_deref().unwrap(),
                reference.line,
                node.file_path,
                node.start_line
            );
        }
        for query in project["resolutions"].as_array().unwrap() {
            let reference: UnresolvedReference =
                serde_json::from_value(query["reference"].clone()).unwrap();
            assert_eq!(
                claims(&reference.reference_name).unwrap(),
                query["claimed"].as_bool().unwrap()
            );
            let actual = ctx
                .resolve(&reference)
                .unwrap()
                .map(|node| json!({"id":node.id,"confidence":0.9}))
                .unwrap_or(Value::Null);
            assert_eq!(
                actual,
                query["expected"],
                "{} {} at {}:{}",
                project["name"],
                reference.reference_name,
                reference.file_path.as_deref().unwrap(),
                reference.line
            );
        }
    }
}
