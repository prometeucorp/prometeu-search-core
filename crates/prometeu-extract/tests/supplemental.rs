//! Differential records captured by executing the pinned reference implementation.
use serde_json::Value;

fn normalize(mut value: Value) -> Value {
    value.as_object_mut().unwrap().remove("durationMs");
    for node in value["nodes"].as_array_mut().unwrap() {
        node.as_object_mut().unwrap().remove("updatedAt");
    }
    for field in ["nodes", "edges", "unresolvedReferences"] {
        value[field]
            .as_array_mut()
            .unwrap()
            .sort_by_cached_key(|record| record.to_string());
    }
    value
}

#[test]
fn supplemental_records_match_reference() {
    let corpus: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/upstream/supplemental/cases.json"
    ))
    .unwrap();
    for case in corpus["cases"].as_array().unwrap().iter().filter(|case| {
        matches!(
            case["language"].as_str(),
            Some("arkts" | "solidity" | "objc")
        ) && case["clean"] == true
    }) {
        let actual = prometeu_extract::extract(
            case["path"].as_str().unwrap(),
            case["input"]
                .as_str()
                .unwrap_or_else(|| case["source"].as_str().unwrap()),
            case["language"].as_str().unwrap(),
        )
        .unwrap();
        let actual = normalize(serde_json::to_value(actual).unwrap());
        let expected = normalize(case["expected"].clone());
        assert_eq!(actual, expected, "{}", case["id"]);
    }
}

#[test]
fn grammar_tables_match_reference() {
    let corpus: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/upstream/supplemental/cases.json"
    ))
    .unwrap();
    for (language, expected) in corpus["grammars"].as_object().unwrap() {
        let grammar = prometeu_extract::grammar_info(language).unwrap();
        assert_eq!(
            grammar.abi_version,
            expected["abiVersion"].as_u64().unwrap() as u32,
            "{language}"
        );
        assert_eq!(
            serde_json::to_value(grammar.node_kinds).unwrap(),
            expected["nodeKinds"],
            "{language}"
        );
        assert_eq!(
            serde_json::to_value(grammar.field_names).unwrap(),
            expected["fieldNames"],
            "{language}"
        );
    }
}
