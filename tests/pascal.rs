use prometeu_search::parser;
use serde_json::Value;
use std::path::Path;

fn normalize(mut value: Value) -> Value {
    value.as_object_mut().unwrap().remove("durationMs");
    for node in value["nodes"].as_array_mut().unwrap() {
        node.as_object_mut().unwrap().remove("updatedAt");
    }
    value
}

#[test]
fn complete_pascal_records_match_independent_reference() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/upstream/pascal/cases.json")).unwrap();
    let mut failures = Vec::new();
    for case in fixture["cases"].as_array().unwrap() {
        let actual = parser::extract_as(
            Path::new(case["path"].as_str().unwrap()),
            case["source"].as_str().unwrap(),
            "pascal",
        )
        .unwrap();
        let actual = normalize(serde_json::to_value(actual).unwrap());
        if actual != case["expected"] {
            for field in ["nodes", "edges", "unresolvedReferences", "errors"] {
                if actual[field] != case["expected"][field] {
                    let found = actual[field].as_array().unwrap();
                    let expected = case["expected"][field].as_array().unwrap();
                    let difference = (0..found.len().max(expected.len()))
                        .find(|&i| found.get(i) != expected.get(i))
                        .unwrap();
                    failures.push(format!("{} {field} expected {} actual {}, first {difference}: expected {:?}, actual {:?}", case["id"], expected.len(), found.len(), expected.get(difference), found.get(difference)));
                    break;
                }
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

#[test]
fn pascal_grammar_and_include_detection_match_reference() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/upstream/pascal/cases.json")).unwrap();
    let grammar = prometeu_extract::grammar_info("pascal").unwrap();
    assert_eq!(
        serde_json::to_value(grammar.node_kinds).unwrap(),
        fixture["grammar"]["nodeKinds"]
    );
    assert_eq!(
        serde_json::to_value(grammar.field_names).unwrap(),
        fixture["grammar"]["fieldNames"]
    );
    for case in fixture["cases"].as_array().unwrap() {
        let parsed =
            parser::parse(Path::new("sample.inc"), case["source"].as_str().unwrap()).unwrap();
        assert_eq!(
            parsed.language, case["detectedIncludeLanguage"],
            "{}",
            case["id"]
        );
    }
}

#[test]
fn extension_overrides_keep_filename_and_content_detection_rules() {
    for (path, source, language, expected) in [
        ("defs.inc", "{$DEFINE RELEASE}", "php", "php"),
        ("defs.custom", "{$DEFINE RELEASE}", "pascal", "pascal"),
        (
            "view.custom",
            "// @flow\nconst value: number = 1;",
            "javascript",
            "tsx",
        ),
        ("store.h", "namespace Store {}", "c", "cpp"),
        ("store.h", "@interface Store\n@end", "c", "objc"),
        ("store.h", "namespace Store {}", "php", "php"),
        (
            "service.app.src",
            "{application, service, []}.",
            "php",
            "erlang",
        ),
        ("conf/routes", "GET / controller.run", "php", "yaml"),
        ("extra.routes", "GET / controller.run", "php", "yaml"),
    ] {
        assert_eq!(
            parser::detect_language(Path::new(path), source, Some(language)).unwrap(),
            expected,
            "{path}"
        );
    }
    assert!(parser::detect_language(Path::new("store.custom"), "", Some("unknown")).is_err());
    let parsed = parser::parse_with_override(
        Path::new("store.custom"),
        "unit Store; interface implementation end.",
        Some("pascal"),
    )
    .unwrap();
    assert_eq!(parsed.language, "pascal");
}
