//! Differential checks against the pinned upstream custom-format extractors.
use serde::Deserialize;
use serde_json::Value;
use std::path::Path;

#[derive(Deserialize)]
struct Case {
    id: String,
    path: String,
    language: String,
    source: String,
    expected: Value,
}

#[derive(Deserialize)]
struct Corpus {
    revision: String,
    cases: Vec<Case>,
}

#[test]
fn custom_formats_match_upstream_records_and_positions() {
    let corpus: Corpus = serde_json::from_str(include_str!("fixtures/formats/cases.json")).unwrap();
    assert_eq!(corpus.revision, "6aa358ccbae30fbf27318f79ee183212f726f9ba");
    let mut differences = Vec::new();
    for case in corpus.cases {
        let actual = prometeu_search::parser::extract_as(
            Path::new(&case.path),
            &case.source,
            &case.language,
        )
        .unwrap_or_else(|error| panic!("{}: {error:#}", case.id));
        let mut actual = serde_json::to_value(actual).unwrap();
        actual.as_object_mut().unwrap().remove("durationMs");
        for node in actual["nodes"].as_array_mut().unwrap() {
            node.as_object_mut().unwrap().remove("updatedAt");
        }
        if actual != case.expected {
            differences.push(format!(
                "{}\nactual: {actual}\nexpected: {}",
                case.id, case.expected
            ));
        }
    }
    assert!(differences.is_empty(), "{}", differences.join("\n"));
}
