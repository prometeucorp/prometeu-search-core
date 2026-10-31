use prometeu_search::{Direction, Engine, parser};
use std::fs;
use std::path::Path;
use tempfile::TempDir;

#[test]
fn entrypoint_source_and_calls_have_a_file_scope() {
    for filename in ["main.ts", "main.js", "main.mjs"] {
        let source = "import './setup';\nbootstrap();\n";
        let parsed = parser::parse(Path::new(filename), source).unwrap();
        assert!(!parsed.has_errors);
        let file = parsed
            .symbols
            .iter()
            .find(|symbol| symbol.kind == "file")
            .unwrap();
        assert_eq!(file.name, filename);
        assert_eq!((file.start_line, file.end_line), (1, 2));
        assert_eq!(&source[file.start_byte..file.end_byte], source);
        let call = parsed
            .references
            .iter()
            .find(|reference| reference.target == "bootstrap")
            .unwrap();
        assert_eq!(parsed.symbols[call.caller.unwrap()].name, filename);
    }
}

#[test]
fn fallback_preserves_global_declarations_arrows_and_named_callers() {
    let source = "import './setup';\nconst config = 1;\nconst start = () => boot();\nfunction boot() { prepare(); }\nstart();\n";
    let parsed = parser::parse(Path::new("main.ts"), source).unwrap();
    assert!(!parsed.has_errors);
    for (name, kind) in [
        ("config", "variable"),
        ("start", "function"),
        ("boot", "function"),
        ("main.ts", "file"),
    ] {
        assert!(
            parsed
                .symbols
                .iter()
                .any(|symbol| symbol.name == name && symbol.kind == kind)
        );
    }
    let calls: Vec<_> = parsed
        .references
        .iter()
        .map(|reference| {
            (
                parsed.symbols[reference.caller.unwrap()].name.as_str(),
                reference.target.as_str(),
            )
        })
        .collect();
    assert!(calls.contains(&("start", "boot")));
    assert!(calls.contains(&("boot", "prepare")));
    assert!(calls.contains(&("main.ts", "start")));
    let declarations = parser::parse(Path::new("helpers.ts"), "function prepare() {}").unwrap();
    assert_eq!(declarations.symbols.len(), 1);
    assert_eq!(declarations.symbols[0].kind, "function");
}

#[test]
fn ordered_coverage_preserves_nested_declarations_and_export_wrappers() {
    for filename in ["nested.ts", "nested.js"] {
        let source = "class Store { save() { function nested() {} nested(); } }\n// Between declarations.\nfunction start() { function inner() {} inner(); }";
        let parsed = parser::parse(Path::new(filename), source).unwrap();
        assert!(!parsed.has_errors);
        assert_eq!(parsed.symbols.len(), 5);
        assert!(!parsed.symbols.iter().any(|symbol| symbol.kind == "file"));

        let exported = format!("{source}\nexport function launch() {{ start(); }}");
        let parsed = parser::parse(Path::new(filename), &exported).unwrap();
        assert!(!parsed.has_errors);
        assert_eq!(
            parsed
                .symbols
                .iter()
                .filter(|symbol| symbol.kind == "file")
                .count(),
            1
        );
        let call = parsed
            .references
            .iter()
            .find(|reference| reference.target == "start")
            .unwrap();
        assert_eq!(parsed.symbols[call.caller.unwrap()].name, "launch");
    }
}

#[test]
fn large_declaration_files_retain_trailing_entrypoint_calls() {
    let count = 20_000;
    let mut source: String = (0..count)
        .map(|index| format!("function f{index}() {{}}\n"))
        .collect();
    assert!(source.len() < 1024 * 1024);
    let parsed = parser::parse(Path::new("large.ts"), &source).unwrap();
    assert!(!parsed.has_errors);
    assert_eq!(parsed.symbols.len(), count);
    assert!(!parsed.symbols.iter().any(|symbol| symbol.kind == "file"));

    source.push_str("f19999();\n");
    let parsed = parser::parse(Path::new("large.ts"), &source).unwrap();
    assert!(!parsed.has_errors);
    assert_eq!(parsed.symbols.len(), count + 1);
    let call = parsed.references.last().unwrap();
    assert_eq!(call.target, "f19999");
    assert_eq!(parsed.symbols[call.caller.unwrap()].kind, "file");
}

#[test]
fn index_searches_imports_and_resolves_entrypoint_calls() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().join("project");
    fs::create_dir(&root).unwrap();
    fs::write(root.join("main.ts"), "import './setup';\nbootstrap();\n").unwrap();
    fs::write(
        root.join("bootstrap.js"),
        "export function bootstrap() {}\n",
    )
    .unwrap();
    let mut engine = Engine::open(&root, Some(&temp.path().join("index.sqlite"))).unwrap();
    engine.sync().unwrap();
    let results = engine.search("setup", 10, 12000).unwrap();
    assert!(results.results.iter().any(|hit| hit.path == "main.ts"
        && hit.kind == "file"
        && hit.snippet.contains("import './setup'")));
    let callers = engine
        .related("bootstrap", Direction::Callers, 10, 12000)
        .unwrap();
    assert_eq!(callers.relations.len(), 1);
    assert_eq!(callers.relations[0].caller.name, "main.ts");
    assert_eq!(
        callers.relations[0].candidate.as_ref().unwrap().path,
        "bootstrap.js"
    );

    fs::write(root.join("main.ts"), "import './teardown';\nshutdown();\n").unwrap();
    engine.sync().unwrap();
    assert!(
        engine
            .search("setup", 10, 12000)
            .unwrap()
            .results
            .is_empty()
    );
    assert!(
        engine
            .related("bootstrap", Direction::Callers, 10, 12000)
            .unwrap()
            .relations
            .is_empty()
    );
}
