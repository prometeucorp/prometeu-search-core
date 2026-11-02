//! Compare native extraction with the pinned upstream TypeScript/WASM oracle.
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    id: String,
    language: String,
    file_path: String,
    input: String,
    expected: String,
}

#[derive(Deserialize)]
struct Corpus {
    revision: String,
    cases: Vec<Case>,
}

fn canonical_records(result: &Value, key: &str) -> Vec<String> {
    let mut records: Vec<_> = result[key]
        .as_array()
        .unwrap_or_else(|| panic!("missing {key} array"))
        .iter()
        .map(|record| {
            let mut record = record.clone();
            if key == "nodes" {
                record.as_object_mut().unwrap().remove("updatedAt");
            }
            record.sort_all_objects();
            serde_json::to_string(&record).unwrap()
        })
        .collect();
    // Treat records as multisets: duplicates remain part of the contract.
    records.sort();
    records
}

#[test]
fn every_native_language_has_an_independent_reference_corpus() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/upstream");
    let mut covered = BTreeSet::new();
    for (manifest, default_language) in [
        ("cases.json", None),
        ("additional/cases.json", None),
        ("supplemental/cases.json", None),
        ("pascal/cases.json", Some("pascal")),
    ] {
        let corpus: Value =
            serde_json::from_slice(&fs::read(root.join(manifest)).unwrap()).unwrap();
        assert_eq!(
            corpus["revision"],
            "6aa358ccbae30fbf27318f79ee183212f726f9ba"
        );
        for case in corpus["cases"].as_array().unwrap() {
            let language = case["language"].as_str().or(default_language).unwrap();
            covered.insert(language.to_owned());
        }
    }
    let missing: Vec<_> = prometeu_extract::LANGUAGES
        .into_iter()
        .filter(|language| !covered.contains(*language))
        .collect();
    assert!(
        missing.is_empty(),
        "registered native languages without an independent oracle: {missing:?}"
    );
}

#[test]
fn native_extraction_matches_pinned_typescript_wasm_reference() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/upstream");
    let corpus: Corpus =
        serde_json::from_slice(&fs::read(root.join("cases.json")).unwrap()).unwrap();
    assert_eq!(corpus.revision, "6aa358ccbae30fbf27318f79ee183212f726f9ba");
    let mut differences = Vec::new();
    for case in corpus.cases {
        let source = fs::read_to_string(root.join(&case.input)).unwrap();
        let expected: Value =
            serde_json::from_slice(&fs::read(root.join(&case.expected)).unwrap()).unwrap();
        let actual = prometeu_extract::extract(&case.file_path, &source, &case.language)
            .unwrap_or_else(|error| panic!("{}: extraction failed: {error}", case.id));
        let actual = serde_json::to_value(actual).unwrap();
        assert!(
            expected["nodes"].as_array().unwrap().len() > 3,
            "{}: empty oracle",
            case.id
        );
        assert!(
            !expected["edges"].as_array().unwrap().is_empty(),
            "{}: no edges",
            case.id
        );
        assert!(
            !expected["unresolvedReferences"]
                .as_array()
                .unwrap()
                .is_empty(),
            "{}: no references",
            case.id
        );
        for key in ["nodes", "edges", "unresolvedReferences", "errors"] {
            let actual = canonical_records(&actual, key);
            let expected = canonical_records(&expected, key);
            if actual != expected {
                let first = actual
                    .iter()
                    .zip(&expected)
                    .position(|(actual, expected)| actual != expected)
                    .unwrap_or(actual.len().min(expected.len()));
                differences.push(format!(
                    "{}: {key}, actual {} records, upstream {} records; first difference {first}:\nactual {:?}\nexpected {:?}",
                    case.id,
                    actual.len(),
                    expected.len(),
                    actual.get(first),
                    expected.get(first),
                ));
            }
        }
    }
    assert!(differences.is_empty(), "{}", differences.join("\n"));
}
