use prometeu_search::Engine;
use std::fs;
use tempfile::TempDir;

fn project() -> (TempDir, Engine) {
    let temporary = TempDir::new().unwrap();
    let root = temporary.path().join("project");
    fs::create_dir_all(root.join("src")).unwrap();
    let engine = Engine::open(&root, Some(&temporary.path().join("index.sqlite"))).unwrap();
    (temporary, engine)
}

#[test]
fn file_and_node_reads_share_indexed_source_and_metadata() {
    let (temporary, mut engine) = project();
    let path = temporary.path().join("project/src/greeting.ts");
    let original = "/** Café greeting. */\nexport function greet(name: string): string {\n  return \"héllo \" + name;\n}\n";
    fs::write(&path, original).unwrap();
    fs::write(temporary.path().join("project/empty.ts"), "").unwrap();
    engine.sync().unwrap();
    let listed = engine.files("SRC/", 10, 12000).unwrap();
    assert_eq!(listed.files.len(), 1);
    assert_eq!(listed.files[0].path, "src/greeting.ts");
    assert_eq!(listed.files[0].language, "typescript");
    assert_eq!(listed.files[0].symbols, 1);

    let id = engine.search("greet", 10, 12000).unwrap().results[0].id;
    let result = engine.node(id, 12000).unwrap();
    assert_eq!(result.generation, listed.generation);
    let node = result.node.unwrap();
    assert_eq!(node.name, "greet");
    assert!(node.source.contains("return \"héllo \" + name;"));
    assert!(!node.source_truncated);
    let metadata = node.metadata.unwrap();
    assert_eq!(metadata.name, "greet");
    assert_eq!(metadata.is_exported, Some(true));
    assert!(metadata.docstring.unwrap().contains("Café greeting"));

    let sliced = engine.file("./src/greeting.ts", 2, Some(3), 12000).unwrap();
    let file = sliced.file.unwrap();
    assert_eq!(
        (file.start_line, file.end_line, file.total_lines),
        (2, 3, 5)
    );
    assert_eq!(
        file.source,
        "export function greet(name: string): string {\n  return \"héllo \" + name;\n"
    );
    assert!(
        engine
            .file("empty.ts", 1, None, 12000)
            .unwrap()
            .file
            .unwrap()
            .source
            .is_empty()
    );

    fs::write(&path, "export function farewell() {}\n").unwrap();
    let old = engine.file("src/greeting.ts", 1, None, 12000).unwrap();
    assert_eq!(old.generation, listed.generation);
    assert_eq!(old.file.unwrap().source, original);
    engine.sync().unwrap();
    let new = engine.file("src/greeting.ts", 1, None, 12000).unwrap();
    assert!(new.generation > listed.generation);
    assert_eq!(new.file.unwrap().source, "export function farewell() {}\n");
    fs::remove_file(path).unwrap();
    engine.sync().unwrap();
    assert!(engine.files("src/", 10, 12000).unwrap().files.is_empty());
    assert!(engine.file("src/greeting.ts", 1, None, 12000).is_err());
}

#[test]
fn reindexing_never_reuses_a_stale_node_id() {
    let (temporary, mut engine) = project();
    let path = temporary.path().join("project/entry.ts");
    fs::write(&path, "export function greet() {}\n").unwrap();
    engine.sync().unwrap();
    let old = engine.search("greet", 10, 12000).unwrap();
    let old = old
        .results
        .iter()
        .find(|node| node.name == "greet")
        .unwrap();
    assert_eq!(
        engine.node(old.id, 12000).unwrap().node.unwrap().name,
        "greet"
    );

    fs::write(&path, "export function farewell() {}\n").unwrap();
    engine.sync().unwrap();
    assert!(engine.node(old.id, 12000).is_err());
    let new = engine.search("farewell", 10, 12000).unwrap();
    let new = new
        .results
        .iter()
        .find(|node| node.name == "farewell")
        .unwrap();
    assert!(new.id > old.id);
    assert_eq!(
        engine.node(new.id, 12000).unwrap().node.unwrap().name,
        "farewell"
    );
}

#[test]
fn inspection_budgets_cover_escaped_utf8_and_file_metadata() {
    let (temporary, mut engine) = project();
    let root = temporary.path().join("project");
    let literal = serde_json::to_string(&"quoted \" 😀 \\ ".repeat(1000)).unwrap();
    fs::write(
        root.join("large.ts"),
        format!("export function large() {{ return {literal}; }}\n"),
    )
    .unwrap();
    for index in 0..8 {
        fs::write(
            root.join(format!("file_{index}_{}.ts", "x".repeat(180))),
            format!("export function item{index}() {{}}"),
        )
        .unwrap();
    }
    engine.sync().unwrap();
    let id = engine.search("large", 10, 12000).unwrap().results[0].id;
    let node = engine.node(id, 1024).unwrap();
    assert!(node.truncated);
    assert!(node.node.as_ref().unwrap().source_truncated);
    assert!(serde_json::to_vec(&node).unwrap().len() <= 1024);
    let file = engine.file("large.ts", 1, None, 1024).unwrap();
    assert!(file.truncated);
    assert!(file.file.as_ref().unwrap().source_truncated);
    assert!(serde_json::to_vec(&file).unwrap().len() <= 1024);
    let files = engine.files("file_", 50, 1024).unwrap();
    assert!(files.truncated);
    assert!(!files.files.is_empty());
    assert!(serde_json::to_vec(&files).unwrap().len() <= 1024);
    let limited = engine.files("", 2, 12000).unwrap();
    assert_eq!(limited.files.len(), 2);
    assert!(limited.truncated);
}

#[test]
fn inspection_rejects_invalid_ranges_and_stays_in_its_worktree() {
    let (temporary, mut engine) = project();
    fs::write(
        temporary.path().join("project/entry.ts"),
        "export const value = 1;\r\n",
    )
    .unwrap();
    fs::write(
        temporary.path().join("outside.ts"),
        "export const secret = 42;",
    )
    .unwrap();
    engine.sync().unwrap();
    let first = engine.file("entry.ts", 1, Some(1), 12000).unwrap();
    assert_eq!(first.file.unwrap().source, "export const value = 1;\r\n");
    let trailing = engine.file("entry.ts", 2, None, 12000).unwrap();
    assert_eq!(trailing.file.unwrap().source, "");
    for path in ["../outside.ts", "/outside.ts", ".", "", "outside.ts"] {
        assert!(engine.file(path, 1, None, 12000).is_err(), "{path}");
    }
    assert!(engine.files("outside", 10, 12000).unwrap().files.is_empty());
    assert!(engine.file("entry.ts", 0, None, 12000).is_err());
    assert!(engine.file("entry.ts", 2, Some(1), 12000).is_err());
    assert!(engine.file("entry.ts", 3, None, 12000).is_err());
    assert!(engine.file("entry.ts", 1, None, 1023).is_err());
    assert!(engine.files("", 0, 12000).is_err());
    assert!(engine.node(0, 12000).is_err());
    assert!(engine.node(i64::MAX, 12000).is_err());
}
