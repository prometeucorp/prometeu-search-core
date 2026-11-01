//! Full extraction and committed-source checks for the additional native walkers.
use prometeu_search::{Engine, parser};
use serde_json::Value;
use std::path::{Path, PathBuf};

fn cases() -> Vec<Value> {
    let corpus: Value =
        serde_json::from_str(include_str!("fixtures/upstream/supplemental/cases.json")).unwrap();
    corpus["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|case| {
            matches!(
                case["language"].as_str(),
                Some("arkts" | "objc" | "solidity")
            ) && case["clean"] == true
        })
        .cloned()
        .collect()
}

fn records(mut value: Value) -> Value {
    value.sort_all_objects();
    value.as_object_mut().unwrap().remove("durationMs");
    for node in value["nodes"].as_array_mut().unwrap() {
        node.as_object_mut().unwrap().remove("updatedAt");
    }
    for field in ["nodes", "edges", "unresolvedReferences"] {
        value[field]
            .as_array_mut()
            .unwrap()
            .sort_by_cached_key(Value::to_string);
    }
    value
}

#[test]
fn supplemental_wrapper_preserves_reference_records() {
    for case in cases() {
        let result = parser::extract_as(
            Path::new(case["path"].as_str().unwrap()),
            case["source"].as_str().unwrap(),
            case["language"].as_str().unwrap(),
        )
        .unwrap();
        assert_eq!(
            records(serde_json::to_value(result).unwrap()),
            records(case["expected"].clone()),
            "{}",
            case["id"]
        );
    }
}

#[test]
fn supplemental_corpus_persists_original_source_and_recovers_unchanged_files() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    std::fs::create_dir(&root).unwrap();
    let cases = cases();
    let paths: Vec<_> = cases
        .iter()
        .map(|case| {
            let mut path =
                PathBuf::from(case["id"].as_str().unwrap()).join(case["path"].as_str().unwrap());
            // Header fixtures also exercise pure C syntax; .m makes their selected
            // Objective-C grammar explicit when indexing through extension detection.
            if case["language"] == "objc" {
                path.set_extension("m");
            }
            std::fs::create_dir_all(root.join(&path).parent().unwrap()).unwrap();
            std::fs::write(root.join(&path), case["source"].as_str().unwrap()).unwrap();
            path
        })
        .collect();
    let database = temp.path().join("index.sqlite");
    let mut engine = Engine::open(&root, Some(&database)).unwrap();
    let first = engine.sync().unwrap();
    assert_eq!(first.indexed, cases.len());
    assert_eq!(first.syntax_errors, 0);
    assert_eq!(engine.sync().unwrap().unchanged, cases.len());
    drop(engine);
    let engine = Engine::open(&root, Some(&database)).unwrap();
    assert_eq!(engine.status().unwrap().files, cases.len());
    for (case, path) in cases.iter().zip(paths) {
        let file = engine
            .file(path.to_str().unwrap(), 1, None, 65536)
            .unwrap()
            .file
            .unwrap();
        assert_eq!(
            file.source,
            case["source"].as_str().unwrap(),
            "{}",
            case["id"]
        );
        assert_eq!(file.language, case["language"].as_str().unwrap());
        assert!(!file.syntax_errors, "{}", case["id"]);
    }
}
