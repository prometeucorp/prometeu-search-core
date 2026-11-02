use prometeu_search::{Engine, parser};
use serde_json::Value;
use std::path::Path;

#[test]
fn cfml_embedded_script_and_component_project_exact_source() {
    for source in [
        "<p>é😀</p><cfscript>function first(){ call(); }</cfscript>",
        "<cfcomponent>\n<cfscript>function first(){ call(); }</cfscript>\n    </cfcomponent>",
    ] {
        let parsed = parser::parse(Path::new("Probe.cfc"), source).unwrap();
        let function = parsed
            .symbols
            .iter()
            .find(|node| node.name == "first")
            .unwrap();
        assert_eq!(
            &source[function.start_byte..function.end_byte],
            "function first(){ call(); }"
        );
        if let Some(component) = parsed.symbols.iter().find(|node| node.kind == "class") {
            assert_eq!(&source[component.start_byte..component.end_byte], source);
        }
    }
}

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
            record.sort_all_objects();
            serde_json::to_string(&record).unwrap()
        })
        .collect::<Vec<_>>();
    out.sort();
    out
}

#[test]
fn cfml_family_matches_independent_reference_and_persists() {
    let corpus: Value =
        serde_json::from_str(include_str!("fixtures/upstream/cfml/cases.json")).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    std::fs::create_dir(&root).unwrap();
    let mut count = 0;
    let mut differences = Vec::new();
    for case in corpus["cases"].as_array().unwrap() {
        let path = Path::new(case["filePath"].as_str().unwrap());
        let source = case["source"].as_str().unwrap();
        let language = case["language"].as_str().unwrap();
        let actual =
            serde_json::to_value(parser::extract_as(path, source, language).unwrap()).unwrap();
        for field in ["nodes", "edges", "unresolvedReferences", "errors"] {
            let actual = records(&actual, field);
            let expected = records(&case["expected"], field);
            if actual != expected {
                let missing = expected
                    .iter()
                    .filter(|record| !actual.contains(record))
                    .take(4)
                    .collect::<Vec<_>>();
                let extra = actual
                    .iter()
                    .filter(|record| !expected.contains(record))
                    .take(4)
                    .collect::<Vec<_>>();
                differences.push(format!(
                    "{} {} {field}: missing {missing:?}; extra {extra:?}",
                    case["id"], case["origin"]
                ));
            }
        }
        if language != "cfquery" {
            parser::parse(path, source).unwrap_or_else(|error| panic!("{}: {error:#}", case["id"]));
            std::fs::write(
                root.join(format!(
                    "{}.{}",
                    case["id"].as_str().unwrap(),
                    path.extension().unwrap().to_str().unwrap()
                )),
                source,
            )
            .unwrap();
            count += 1;
        }
    }
    assert!(
        differences.is_empty(),
        "{} differences:\n{}",
        differences.len(),
        differences.join("\n")
    );
    let mut engine = Engine::open(&root, Some(&temp.path().join("cache.sqlite"))).unwrap();
    assert_eq!(engine.sync().unwrap().indexed, count);
    assert_eq!(engine.sync().unwrap().unchanged, count);
}

#[test]
fn cfml_grammar_tables_match_pinned_wasm() {
    let corpus: Value =
        serde_json::from_str(include_str!("fixtures/upstream/cfml/cases.json")).unwrap();
    for language in ["cfml", "cfscript", "cfquery"] {
        let grammar = prometeu_extract::grammar_for(language).unwrap();
        let expected = &corpus["grammars"][language];
        assert_eq!(
            grammar.abi_version(),
            expected["abiVersion"].as_u64().unwrap() as usize
        );
        assert_eq!(
            grammar.parse_state_count(),
            expected["stateCount"].as_u64().unwrap() as usize
        );
        assert_eq!(
            serde_json::to_value(
                (0..grammar.node_kind_count())
                    .map(|i| grammar.node_kind_for_id(i as u16))
                    .collect::<Vec<_>>()
            )
            .unwrap(),
            expected["nodeKinds"]
        );
        assert_eq!(
            serde_json::to_value(
                (1..=grammar.field_count())
                    .map(|i| grammar.field_name_for_id(i as u16))
                    .collect::<Vec<_>>()
            )
            .unwrap(),
            expected["fieldNames"]
        );
    }
}
