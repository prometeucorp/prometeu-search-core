//! Full component records compared with independently captured upstream output.
use prometeu_search::parser;
use serde_json::Value;
use std::path::Path;

#[test]
fn component_projection_keeps_inline_script_source_spans() {
    for language in ["vue", "svelte", "astro"] {
        let path = format!("Inline.{language}");
        for source in [
            "<p>é😀</p><script>function first() { call(); }</script>",
            "<p>é😀</p><script>function first() {\n call();\n}</script>",
            "<p>é😀</p><script>\nfunction first() { call(); }\n</script>",
            "<script>function first() { call(); }</script><script>function second() { call(); }</script>",
        ] {
            let parsed = parser::parse(Path::new(&path), source).unwrap();
            assert_eq!(
                parsed
                    .symbols
                    .iter()
                    .filter(|symbol| symbol.kind == "function")
                    .count(),
                if source.contains("second") { 2 } else { 1 }
            );
            for symbol in parsed
                .symbols
                .iter()
                .filter(|symbol| symbol.kind == "function")
            {
                let actual = &source[symbol.start_byte..symbol.end_byte];
                let expected = if source.contains("{\n") {
                    format!("function {}() {{\n call();\n}}", symbol.name)
                } else {
                    format!("function {}() {{ call(); }}", symbol.name)
                };
                assert_eq!(actual, expected, "{path} {}", symbol.name);
            }
        }
    }
}

#[test]
fn large_vue_template_keeps_every_call_location() {
    let rows = 12_000;
    let source = format!(
        "<template>\n{}</template>",
        "é😀<button @click=\"handle()\">go</button>\n".repeat(rows)
    );
    let extraction = parser::extract(Path::new("Large.vue"), &source).unwrap();
    assert!(extraction.errors.is_empty(), "{:?}", extraction.errors);
    let calls = extraction
        .unresolved_references
        .iter()
        .filter(|reference| reference.reference_name == "handle")
        .collect::<Vec<_>>();
    assert_eq!(calls.len(), rows);
    for (index, call) in calls.iter().enumerate() {
        assert_eq!((call.line, call.column), (index as u32 + 2, 19));
    }
}

#[test]
fn component_corpus_projects_valid_declaration_spans() {
    let corpus: Value =
        serde_json::from_str(include_str!("fixtures/upstream/components/cases.json")).unwrap();
    for case in corpus["cases"].as_array().unwrap() {
        let source = case["source"].as_str().unwrap();
        let parsed = parser::parse(Path::new(case["filePath"].as_str().unwrap()), source)
            .unwrap_or_else(|error| panic!("{}: {error:#}", case["id"]));
        for symbol in &parsed.symbols {
            let text = source
                .get(symbol.start_byte..symbol.end_byte)
                .unwrap_or_else(|| panic!("{} {}: invalid UTF-8 span", case["id"], symbol.name));
            if matches!(
                symbol.kind.as_str(),
                "function" | "method" | "class" | "interface"
            ) {
                // Named arrow functions span their expression, whose binding is outside it.
                assert!(
                    text.contains(&symbol.name) || text.contains("=>"),
                    "{} {}: projected {text:?}",
                    case["id"],
                    symbol.name
                );
            }
        }
        let canonical = serde_json::to_value(parsed.extraction.unwrap()).unwrap();
        let expected = expected_component(case);
        for field in ["nodes", "edges", "unresolvedReferences", "errors"] {
            assert_eq!(
                records(&canonical, field),
                records(&expected, field),
                "{}: projection changed canonical {field}",
                case["id"]
            );
        }
    }
}

#[test]
fn component_oracle_corpus_reaches_the_persistent_index() {
    let corpus: Value =
        serde_json::from_str(include_str!("fixtures/upstream/components/cases.json")).unwrap();
    let cases = corpus["cases"].as_array().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    std::fs::create_dir(&root).unwrap();
    for case in cases {
        std::fs::write(
            root.join(format!(
                "{}.{}",
                case["id"].as_str().unwrap(),
                case["language"].as_str().unwrap()
            )),
            case["source"].as_str().unwrap(),
        )
        .unwrap();
    }
    let database = temp.path().join("index.sqlite");
    let mut engine = prometeu_search::Engine::open(&root, Some(&database)).unwrap();
    assert_eq!(engine.sync().unwrap().indexed, cases.len());
    assert_eq!(engine.sync().unwrap().unchanged, cases.len());
    drop(engine);
    let engine = prometeu_search::Engine::open(&root, Some(&database)).unwrap();
    assert_eq!(engine.status().unwrap().files, cases.len());
}

#[test]
fn indexed_component_nodes_return_their_original_script_source() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    std::fs::create_dir(&root).unwrap();
    let mut engine =
        prometeu_search::Engine::open(&root, Some(&temp.path().join("index.sqlite"))).unwrap();
    for language in ["vue", "svelte", "astro"] {
        let name = format!("inline_{language}");
        std::fs::write(
            root.join(format!("Inline.{language}")),
            format!("<p>é😀</p><script>function {name}() {{ return 42; }}</script>"),
        )
        .unwrap();
    }
    engine.sync().unwrap();
    for language in ["vue", "svelte", "astro"] {
        let name = format!("inline_{language}");
        let found = engine.search(&name, 10, 12000).unwrap();
        let symbol = found
            .results
            .iter()
            .find(|symbol| symbol.name == name)
            .unwrap();
        let node = engine.node(symbol.id, 12000).unwrap().node.unwrap();
        assert_eq!(node.source, format!("function {name}() {{ return 42; }}"));
    }
}

fn expected_component(case: &Value) -> Value {
    let mut expected = case["expected"].clone();
    let call = match case["id"].as_str().unwrap() {
        "vue-50" => "defineComponent",
        "vue-51" => "Vue.extend",
        "vue-52" => "Vue.component",
        _ => return expected,
    };
    // These factory calls precede the method on the same line. Keep the pinned
    // fixture intact while correcting only its three mistaken method owners.
    let references = expected["unresolvedReferences"].as_array_mut().unwrap();
    assert_eq!(references.len(), 1);
    assert_eq!(references[0]["referenceName"], call);
    assert_eq!(references[0]["line"], 2);
    assert_eq!(references[0]["column"], 15);
    references[0]["fromNodeId"] = format!("file:{}", case["filePath"].as_str().unwrap()).into();
    expected
}

fn records(result: &Value, field: &str) -> Vec<String> {
    let mut records: Vec<_> = result[field]
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
        .collect();
    records.sort();
    records
}

#[test]
fn component_extraction_matches_typescript_wasm_oracle() {
    let corpus: Value =
        serde_json::from_str(include_str!("fixtures/upstream/components/cases.json")).unwrap();
    assert_eq!(
        corpus["revision"],
        "6aa358ccbae30fbf27318f79ee183212f726f9ba"
    );
    let mut differences = Vec::new();
    for case in corpus["cases"].as_array().unwrap() {
        let actual = parser::extract_as(
            Path::new(case["filePath"].as_str().unwrap()),
            case["source"].as_str().unwrap(),
            case["language"].as_str().unwrap(),
        )
        .unwrap();
        let actual = serde_json::to_value(actual).unwrap();
        let reference = expected_component(case);
        for field in ["nodes", "edges", "unresolvedReferences", "errors"] {
            let expected = records(&reference, field);
            let actual = records(&actual, field);
            if actual != expected {
                let missing: Vec<_> = expected
                    .iter()
                    .filter(|record| !actual.contains(record))
                    .take(3)
                    .collect();
                let extra: Vec<_> = actual
                    .iter()
                    .filter(|record| !expected.contains(record))
                    .take(3)
                    .collect();
                differences.push(format!(
                    "{} {} {field}: missing {missing:?}, extra {extra:?}",
                    case["id"], case["origin"]
                ));
            }
        }
    }
    assert!(
        differences.is_empty(),
        "{} component differences:\n{}",
        differences.len(),
        differences.join("\n")
    );
}
