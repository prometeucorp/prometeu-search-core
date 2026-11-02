use prometeu_search::parser;
use serde_json::Value;
use std::path::Path;

fn records(value: &Value, field: &str) -> Vec<String> {
    let mut values: Vec<_> = value[field]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| {
            let mut v = v.clone();
            if field == "nodes" {
                v.as_object_mut().unwrap().remove("updatedAt");
            }
            v.sort_all_objects();
            v.to_string()
        })
        .collect();
    values.sort();
    values
}

#[test]
fn vbnet_matches_independent_reference() {
    let fixtures: Value =
        serde_json::from_str(include_str!("fixtures/upstream/supplemental/cases.json")).unwrap();
    let mut failures = Vec::new();
    for case in fixtures["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["language"] == "vbnet")
    {
        let actual = serde_json::to_value(
            parser::extract_as(
                Path::new(case["path"].as_str().unwrap()),
                case["source"].as_str().unwrap(),
                "vbnet",
            )
            .unwrap(),
        )
        .unwrap();
        for field in ["nodes", "edges", "unresolvedReferences", "errors"] {
            let actual = records(&actual, field);
            let expected = records(&case["expected"], field);
            if actual != expected {
                let i = (0..actual.len().max(expected.len()))
                    .find(|&i| actual.get(i) != expected.get(i))
                    .unwrap();
                failures.push(format!(
                    "{} {field}: expected {}, actual {}; first expected {:?}, actual {:?}",
                    case["id"],
                    expected.len(),
                    actual.len(),
                    expected.get(i),
                    actual.get(i)
                ));
                break;
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
