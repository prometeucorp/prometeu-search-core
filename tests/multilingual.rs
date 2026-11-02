use prometeu_search::{Direction, Engine, parser};
use serde_json::Value;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

#[test]
fn dart_callable_snippets_include_the_entire_final_body_line() {
    let source =
        "int foo()\n{ return 123; }\nclass Example {\n  int value()\n  { return 456; }\n}\n";
    let mut raw = parser::extract(Path::new("example.dart"), source).unwrap();
    let parsed = parser::parse(Path::new("example.dart"), source).unwrap();
    for (name, expected) in [
        ("foo", "int foo()\n{ return 123; }"),
        ("value", "int value()\n  { return 456; }"),
    ] {
        let symbol = parsed
            .symbols
            .iter()
            .find(|symbol| symbol.name == name)
            .unwrap();
        assert_eq!(&source[symbol.start_byte..symbol.end_byte], expected);
    }
    let mut projected = parsed.extraction.unwrap();
    for node in raw.nodes.iter_mut().chain(&mut projected.nodes) {
        node.updated_at = 0;
    }
    assert_eq!(
        projected.nodes, raw.nodes,
        "snippet correction must preserve canonical coordinates"
    );
}

#[test]
fn every_native_language_reaches_the_persistent_index() {
    let corpus_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/upstream");
    let corpus: Value =
        serde_json::from_slice(&fs::read(corpus_root.join("cases.json")).unwrap()).unwrap();
    let temp = TempDir::new().unwrap();
    let root = temp.path().join("project");
    fs::create_dir(&root).unwrap();
    let mut count = 0;
    for case in corpus["cases"].as_array().unwrap() {
        let directory = root.join(case["id"].as_str().unwrap());
        fs::create_dir(&directory).unwrap();
        let mut path = directory.join(case["filePath"].as_str().unwrap());
        // Upstream intentionally parses the same .js fixture in both modes.
        if case["language"] == "jsx" {
            path.set_extension("jsx");
        }
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let source = fs::read_to_string(corpus_root.join(case["input"].as_str().unwrap())).unwrap();
        fs::write(&path, source).unwrap();
        count += 1;
    }
    let database = temp.path().join("index.sqlite");
    let mut engine = Engine::open(&root, Some(&database)).unwrap();
    let first = engine.sync().unwrap();
    assert_eq!(first.indexed, count);
    assert_eq!(first.syntax_errors, 0);
    assert!(engine.status().unwrap().symbols > 1_000);
    assert_eq!(engine.sync().unwrap().unchanged, count);
    drop(engine);
    let engine = Engine::open(&root, Some(&database)).unwrap();
    assert_eq!(engine.status().unwrap().files, count);
}

#[test]
fn python_imports_resolve_and_incremental_edits_remove_stale_calls() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().join("project");
    fs::create_dir_all(root.join("app")).unwrap();
    fs::write(root.join("app/store.py"), "def save():\n    pass\n").unwrap();
    fs::write(
        root.join("app/main.py"),
        "from .store import save as persist\ndef run():\n    persist()\n",
    )
    .unwrap();
    fs::write(root.join("other.py"), "def save():\n    pass\n").unwrap();
    let mut engine = Engine::open(&root, Some(&temp.path().join("index.sqlite"))).unwrap();
    engine.sync().unwrap();
    let callers = engine
        .related("save", Direction::Callers, 10, 12000)
        .unwrap();
    assert_eq!(callers.relations.len(), 2, "{callers:?}");
    for (kind, line) in [("imports", 1), ("calls", 3)] {
        let relation = callers
            .relations
            .iter()
            .find(|relation| relation.kind == kind)
            .unwrap();
        assert_eq!(relation.call_line, line);
        assert_eq!(relation.candidate.as_ref().unwrap().path, "app/store.py");
        assert_eq!(relation.resolution, "import_match");
    }
    fs::write(root.join("app/main.py"), "def run():\n    pass\n").unwrap();
    assert_eq!(
        engine.sync_paths(&["app/main.py".into()]).unwrap().indexed,
        1
    );
    assert!(
        engine
            .related("save", Direction::Callers, 10, 12000)
            .unwrap()
            .relations
            .is_empty()
    );
}

#[test]
fn unicode_offsets_and_partial_source_remain_explicit() {
    let source = "# 😀\r\ndef café():\r\n    return '你好'\r\n";
    let parsed = parser::parse(Path::new("unicode.py"), source).unwrap();
    let symbol = parsed.symbols.iter().find(|s| s.name == "café").unwrap();
    assert_eq!(
        &source[symbol.start_byte..symbol.end_byte],
        "def café():\r\n    return '你好'"
    );
    let partial = parser::parse(
        Path::new("partial.py"),
        "def intact():\n    return 1\ndef broken(\n",
    )
    .unwrap();
    assert!(partial.has_errors);
    assert!(partial.symbols.iter().any(|s| s.name == "intact"));
    assert!(
        partial
            .extraction
            .unwrap()
            .errors
            .iter()
            .any(|e| e["code"] == "native_parse_recovery")
    );
}

#[test]
fn theme_json_eligibility_reconciles_when_theme_markers_change() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().join("project");
    fs::create_dir_all(root.join("shop/templates/customers")).unwrap();
    fs::create_dir_all(root.join("shop/layout")).unwrap();
    fs::create_dir_all(root.join("unrelated/templates")).unwrap();
    let template = r#"{"sections":{"main":{"type":"product"}}}"#;
    fs::write(root.join("shop/templates/customers/login.json"), template).unwrap();
    fs::write(root.join("unrelated/templates/sample.json"), template).unwrap();
    let mut engine = Engine::open(&root, Some(&temp.path().join("index.sqlite"))).unwrap();
    assert_eq!(engine.sync().unwrap().indexed, 0);
    fs::write(
        root.join("shop/layout/theme.liquid"),
        "{% render 'header' %}",
    )
    .unwrap();
    assert_eq!(
        engine
            .sync_paths(&["shop/layout/theme.liquid".into()])
            .unwrap()
            .indexed,
        2
    );
    let files = engine.files("", 50, 12000).unwrap();
    assert_eq!(files.files.len(), 2);
    assert!(
        files
            .files
            .iter()
            .all(|file| file.language == "liquid" && file.path.starts_with("shop/"))
    );
    fs::remove_file(root.join("shop/layout/theme.liquid")).unwrap();
    assert_eq!(
        engine
            .sync_paths(&["shop/layout/theme.liquid".into()])
            .unwrap()
            .removed,
        2
    );
}

#[cfg(unix)]
#[test]
fn unreadable_theme_marker_preserves_the_committed_index() {
    use std::os::unix::fs::PermissionsExt;

    let temp = TempDir::new().unwrap();
    let root = temp.path().join("project");
    fs::create_dir_all(root.join("config")).unwrap();
    fs::create_dir_all(root.join("templates")).unwrap();
    let marker = root.join("config/settings_schema.json");
    fs::write(&marker, "[]").unwrap();
    fs::write(
        root.join("templates/index.json"),
        r#"{"sections":{"main":{"type":"product"}}}"#,
    )
    .unwrap();
    let mut engine = Engine::open(&root, Some(&temp.path().join("index.sqlite"))).unwrap();
    let committed = engine.sync().unwrap();
    assert_eq!(committed.indexed, 1);
    let permissions = fs::metadata(&marker).unwrap().permissions();
    fs::set_permissions(&marker, fs::Permissions::from_mode(0o0)).unwrap();
    // Privileged users can read mode-000 files; this case cannot exercise their errors.
    if fs::File::open(&marker).is_ok() {
        fs::set_permissions(&marker, permissions).unwrap();
        return;
    }
    let result = engine.sync_paths(&["config/settings_schema.json".into()]);
    fs::set_permissions(&marker, permissions).unwrap();
    let error = result.unwrap_err();
    assert!(error.to_string().contains("cannot inspect theme marker"));
    let files = engine.files("", 10, 12000).unwrap();
    assert_eq!(files.generation, committed.generation);
    assert_eq!(files.files.len(), 1);
    assert_eq!(files.files[0].path, "templates/index.json");
    assert_eq!(engine.sync().unwrap().unchanged, 1);
}

#[cfg(unix)]
#[test]
fn symlinked_theme_marker_cannot_change_eligibility_or_read_outside_source() {
    use std::os::unix::fs::symlink;

    let temp = TempDir::new().unwrap();
    let root = temp.path().join("project");
    fs::create_dir_all(root.join("config")).unwrap();
    fs::create_dir_all(root.join("templates")).unwrap();
    let marker = root.join("config/settings_schema.json");
    fs::write(&marker, "[]").unwrap();
    fs::write(root.join("templates/index.json"), r#"{"sections":{}}"#).unwrap();
    let mut engine = Engine::open(&root, Some(&temp.path().join("index.sqlite"))).unwrap();
    let committed = engine.sync().unwrap();
    let outside = temp.path().join("outside.json");
    fs::write(&outside, "[]").unwrap();
    fs::remove_file(&marker).unwrap();
    symlink(&outside, &marker).unwrap();
    assert!(engine.sync().is_err());
    let files = engine.files("", 10, 12000).unwrap();
    assert_eq!(files.generation, committed.generation);
    assert_eq!(files.files.len(), 1);
}
