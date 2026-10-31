use prometeu_search::Engine;
use std::{collections::HashSet, fs};
use tempfile::TempDir;

#[test]
fn overview_aggregates_cross_file_evidence_and_observes_other_clients() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().join("project");
    fs::create_dir(&root).unwrap();
    fs::write(root.join("leaf.ts"), "export function leaf() {}").unwrap();
    fs::write(
        root.join("entry.ts"),
        "import { leaf } from './leaf'; function run() { leaf(); leaf(); run(); missing(); }",
    )
    .unwrap();
    fs::write(root.join("fallback.ts"), "function fallback() { leaf(); }").unwrap();
    let database = temp.path().join("index.sqlite");
    let mut engine = Engine::open(&root, Some(&database)).unwrap();
    assert!(engine.overview().unwrap().files.is_empty());
    engine.sync().unwrap();
    let overview = engine.overview().unwrap();
    assert!(!overview.truncated);
    assert_eq!(overview.files.len(), 3);
    assert_eq!(overview.edges.len(), 2);
    assert!(
        overview
            .files
            .iter()
            .all(|file| file.language == "typescript" && file.symbols > 0)
    );
    assert_eq!(
        overview
            .files
            .iter()
            .map(|file| file.symbols)
            .sum::<usize>(),
        engine.status().unwrap().symbols
    );
    let imported = overview
        .edges
        .iter()
        .find(|edge| edge.from == "entry.ts")
        .unwrap();
    assert_eq!(
        (imported.to.as_str(), imported.count, imported.uncertain),
        ("leaf.ts", 2, false)
    );
    assert!(
        overview
            .edges
            .iter()
            .find(|edge| edge.from == "fallback.ts")
            .unwrap()
            .uncertain
    );
    let first = serde_json::to_value(&overview).unwrap();
    assert_eq!(
        first,
        serde_json::to_value(engine.overview().unwrap()).unwrap()
    );

    let mut writer = Engine::open(&root, Some(&database)).unwrap();
    fs::remove_file(root.join("fallback.ts")).unwrap();
    fs::rename(root.join("leaf.ts"), root.join("renamed.ts")).unwrap();
    writer.sync().unwrap();
    let updated = engine.overview().unwrap();
    assert!(updated.generation > overview.generation);
    assert_eq!(updated.files.len(), 2);
    assert!(
        updated.edges.is_empty(),
        "a missing imported file must not resolve by name"
    );
    assert!(updated.files.iter().any(|file| file.path == "renamed.ts"));
}

#[test]
fn overview_limits_files_without_dangling_edges() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().join("project");
    fs::create_dir(&root).unwrap();
    for index in 0..1_005 {
        fs::write(
            root.join(format!("file{index:04}.ts")),
            format!("function f{index}() {{}}"),
        )
        .unwrap();
    }
    let mut engine = Engine::open(&root, Some(&temp.path().join("index.sqlite"))).unwrap();
    engine.sync().unwrap();
    let overview = engine.overview().unwrap();
    assert!(overview.truncated);
    assert_eq!(overview.files.len(), 1_000);
    assert_eq!(engine.status().unwrap().files, 1_005);
    assert_eq!(overview.files.first().unwrap().path, "file0000.ts");
    assert_eq!(overview.files.last().unwrap().path, "file0999.ts");
}

#[test]
fn overview_bounds_dense_edges_and_serialized_bytes() {
    for long_names in [false, true] {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("project");
        fs::create_dir(&root).unwrap();
        let suffix = if long_names {
            "x".repeat(100)
        } else {
            String::new()
        };
        let calls: String = (0..70).map(|index| format!("f{index}();")).collect();
        for index in 0..70 {
            fs::write(
                root.join(format!("file{index:02}{suffix}.ts")),
                format!("function f{index}() {{ {calls} }}"),
            )
            .unwrap();
        }
        let mut engine = Engine::open(&root, Some(&temp.path().join("index.sqlite"))).unwrap();
        engine.sync().unwrap();
        let overview = engine.overview().unwrap();
        assert!(overview.truncated);
        assert_eq!(overview.files.len(), 70);
        assert!(serde_json::to_vec(&overview).unwrap().len() <= 512 * 1024);
        if long_names {
            assert!(overview.edges.len() < 4_000);
        } else {
            assert_eq!(overview.edges.len(), 4_000);
        }
        let files: HashSet<_> = overview.files.iter().map(|file| &file.path).collect();
        assert!(overview.edges.iter().all(|edge| files.contains(&edge.from)
            && files.contains(&edge.to)
            && edge.from != edge.to));
    }
}
