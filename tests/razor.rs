use prometeu_search::{Engine, parser};
use serde_json::Value;
use std::path::Path;

fn records(result: &Value, field: &str) -> Vec<String> {
    let mut out = result[field]
        .as_array()
        .unwrap()
        .iter()
        .map(|record| {
            let mut record = record.clone();
            if field == "nodes" {
                record.as_object_mut().unwrap().remove("updatedAt");
            }
            serde_json::to_string(&record).unwrap()
        })
        .collect::<Vec<_>>();
    out.sort();
    out
}

#[test]
fn razor_matches_independent_reference_and_reaches_the_index() {
    let corpus: Value =
        serde_json::from_str(include_str!("fixtures/upstream/razor/cases.json")).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    std::fs::create_dir(&root).unwrap();
    let cases = corpus["cases"].as_array().unwrap();
    let mut differences = Vec::new();
    for case in cases {
        let path = Path::new(case["filePath"].as_str().unwrap());
        let source = case["source"].as_str().unwrap();
        let actual = serde_json::to_value(parser::extract(path, source).unwrap()).unwrap();
        for field in ["nodes", "edges", "unresolvedReferences", "errors"] {
            let actual = records(&actual, field);
            let expected = records(&case["expected"], field);
            if actual != expected {
                differences.push(format!(
                    "{} {field}: actual {actual:?}, expected {expected:?}",
                    case["id"]
                ));
            }
        }
        let parsed = parser::parse(path, source).unwrap();
        assert!(parsed.symbols.iter().any(|node| node.kind == "component"));
        std::fs::write(
            root.join(format!(
                "{}.{}",
                case["id"].as_str().unwrap(),
                path.extension().unwrap().to_str().unwrap()
            )),
            source,
        )
        .unwrap();
    }
    assert!(differences.is_empty(), "{}", differences.join("\n"));
    let mut engine = Engine::open(&root, Some(&temp.path().join("cache.sqlite"))).unwrap();
    assert_eq!(engine.sync().unwrap().indexed, cases.len());
    assert_eq!(engine.sync().unwrap().unchanged, cases.len());
}
