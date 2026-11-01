use prometeu_search::Engine;
use serde_json::json;
use std::fs;
use tempfile::TempDir;

#[test]
fn dead_code_uses_committed_source_and_conservative_graph_exclusions() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().join("project");
    fs::create_dir(&root).unwrap();
    let original = "export function used() { return 1; }\nfunction forgotten() { return 2; }\nfunction mentioned() { return 3; }\nconst registration = 'mentioned';\nclass EmptyService { staleMethod() {} }\nexport function publicOutside() {}\n";
    fs::write(root.join("util.ts"), original).unwrap();
    fs::write(
        root.join("entry.ts"),
        "import { used } from './util';\nexport function run() { return used(); }\n",
    )
    .unwrap();
    let mut engine = Engine::open(&root, Some(&temp.path().join("index.sqlite"))).unwrap();
    engine.sync().unwrap();
    let report = engine.dead_code(&Default::default()).unwrap();
    let names: Vec<_> = report
        .report
        .entries
        .iter()
        .map(|entry| entry.node.name.as_str())
        .collect();
    assert!(names.contains(&"forgotten"), "{report:?}");
    assert!(names.contains(&"EmptyService"), "{report:?}");
    assert!(!names.contains(&"mentioned"));
    assert!(!names.contains(&"publicOutside"));
    assert!(
        report
            .report
            .entries
            .iter()
            .find(|entry| entry.node.name == "EmptyService")
            .unwrap()
            .members
            .iter()
            .any(|node| node.name == "staleMethod")
    );
    assert!(report.report.excluded.mentioned > 0);
    assert!(report.report.excluded.exported > 0);
    fs::write(
        root.join("util.ts"),
        original.replace(
            "const registration = 'mentioned';",
            "const registration = 'different';",
        ),
    )
    .unwrap();
    let unchanged = engine.dead_code(&Default::default()).unwrap();
    assert_eq!(
        serde_json::to_value(&unchanged).unwrap(),
        serde_json::to_value(&report).unwrap()
    );
    engine.sync().unwrap();
    let updated = engine.dead_code(&Default::default()).unwrap();
    assert!(updated.generation > report.generation);
    assert!(
        updated
            .report
            .entries
            .iter()
            .any(|entry| entry.node.name == "mentioned")
    );
    let options =
        serde_json::from_value(json!({"include_exported":true,"limit":1,"max_bytes":1024}))
            .unwrap();
    let bounded = engine.dead_code(&options).unwrap();
    assert!(serde_json::to_vec(&bounded).unwrap().len() <= 1024);
    assert!(bounded.truncated);
    let invalid = serde_json::from_value(json!({"kinds":["x".repeat(65)]})).unwrap();
    assert!(engine.dead_code(&invalid).is_err());
}

#[test]
fn isolated_files_are_not_reported_as_dead_symbols() {
    let temp = TempDir::new().unwrap();
    fs::write(
        temp.path().join("isolated.ts"),
        "export function outside() {}\nfunction invisibleEntry() {}\n",
    )
    .unwrap();
    let mut engine = Engine::open(temp.path(), Some(&temp.path().join("index.sqlite"))).unwrap();
    engine.sync().unwrap();
    let report = engine.dead_code(&Default::default()).unwrap();
    assert!(report.report.entries.is_empty());
    assert_eq!(report.report.excluded.unreachable_file, 1);
    assert_eq!(report.report.excluded.exported, 1);
}
