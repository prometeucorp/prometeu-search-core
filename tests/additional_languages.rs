//! Additional native AST walkers against the pinned TypeScript/WASM reference.
use serde_json::Value;

fn check(language: &str) {
    let corpus: Value =
        serde_json::from_str(include_str!("fixtures/upstream/additional/cases.json")).unwrap();
    assert_eq!(
        corpus["revision"],
        "6aa358ccbae30fbf27318f79ee183212f726f9ba"
    );
    let mut checked = 0;
    let mut differences = Vec::new();
    for case in corpus["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|case| case["language"] == language)
    {
        checked += 1;
        let actual = prometeu_extract::extract(
            case["filePath"].as_str().unwrap(),
            case["source"].as_str().unwrap(),
            language,
        )
        .unwrap_or_else(|error| panic!("{}: {error}", case["id"]));
        let mut actual = serde_json::to_value(actual).unwrap();
        actual.as_object_mut().unwrap().remove("durationMs");
        for node in actual["nodes"].as_array_mut().unwrap() {
            node.as_object_mut().unwrap().remove("updatedAt");
        }
        for field in ["nodes", "edges", "unresolvedReferences", "errors"] {
            let records = |result: &Value| {
                let mut values: Vec<_> = result[field]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|value| {
                        let mut value = value.clone();
                        value.sort_all_objects();
                        serde_json::to_string(&value).unwrap()
                    })
                    .collect();
                values.sort();
                values
            };
            let expected = records(&case["expected"]);
            let actual = records(&actual);
            if actual != expected {
                let first = actual
                    .iter()
                    .zip(&expected)
                    .position(|(a, b)| a != b)
                    .unwrap_or(actual.len().min(expected.len()));
                differences.push(format!("{} {} {field}: actual {} records, expected {}; first difference {first}: actual {:?}, expected {:?}", case["id"], case["origin"], actual.len(), expected.len(), actual.get(first), expected.get(first)));
            }
        }
    }
    assert!(checked > 0, "missing {language} oracle cases");
    assert!(differences.is_empty(), "{}", differences.join("\n"));
}

#[test]
fn nix_matches_upstream() {
    check("nix");
}
#[test]
fn erlang_matches_upstream() {
    check("erlang");
}
#[test]
fn terraform_matches_upstream() {
    check("terraform");
}
#[test]
fn cobol_matches_upstream() {
    check("cobol");
}

#[test]
fn additional_grammar_tables_match_the_pinned_wasm_artifacts() {
    let corpus: Value =
        serde_json::from_str(include_str!("fixtures/upstream/additional/cases.json")).unwrap();
    for language in ["nix", "erlang", "terraform", "cobol"] {
        let actual = prometeu_extract::grammar_info(language).unwrap();
        let expected = &corpus["grammars"][language];
        assert_eq!(
            serde_json::to_value(actual.node_kinds).unwrap(),
            expected["nodeKinds"],
            "{language} node-kind table"
        );
        assert_eq!(
            serde_json::to_value(actual.field_names).unwrap(),
            expected["fieldNames"],
            "{language} field table"
        );
    }
}

#[test]
fn additional_languages_survive_persistent_indexing_and_source_inspection() {
    use prometeu_search::Engine;
    use std::fs;
    let corpus: Value =
        serde_json::from_str(include_str!("fixtures/upstream/additional/cases.json")).unwrap();
    let cases = corpus["cases"].as_array().unwrap();
    let temporary = tempfile::TempDir::new().unwrap();
    let root = temporary.path().join("project");
    for case in cases {
        let path = root.join(case["filePath"].as_str().unwrap());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, case["source"].as_str().unwrap()).unwrap();
    }
    let database = temporary.path().join("index.sqlite");
    let mut engine = Engine::open(&root, Some(&database)).unwrap();
    assert_eq!(engine.sync().unwrap().indexed, cases.len());
    assert_eq!(engine.sync().unwrap().unchanged, cases.len());
    drop(engine);
    let engine = Engine::open(&root, Some(&database)).unwrap();
    assert_eq!(engine.status().unwrap().files, cases.len());
    let connection = rusqlite::Connection::open(&database).unwrap();
    for case in cases {
        let path = case["filePath"].as_str().unwrap();
        let file = engine.file(path, 1, None, 65536).unwrap().file.unwrap();
        assert_eq!(file.source, case["source"], "{} full source", case["id"]);
        assert_eq!(file.language, case["language"]);
        let expected = case["expected"]["nodes"].as_array().unwrap();
        let mut statement = connection
            .prepare("SELECT id,ordinal FROM symbols WHERE path=?1 ORDER BY ordinal")
            .unwrap();
        let rows = statement
            .query_map([path], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)? as usize))
            })
            .unwrap();
        for row in rows {
            let (id, ordinal) = row.unwrap();
            let node = engine.node(id, 65536).unwrap().node.unwrap();
            let mut metadata = serde_json::to_value(node.metadata.unwrap()).unwrap();
            metadata.as_object_mut().unwrap().remove("updatedAt");
            assert_eq!(
                metadata, expected[ordinal],
                "{} stored symbol {}",
                case["id"], node.name
            );
        }
    }
}
