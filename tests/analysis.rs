use prometeu_search::{DeadCodeOptions, Engine};
use std::fs;
use tempfile::TempDir;

#[test]
fn measurements_and_cycles_keep_snapshot_bounds_and_survive_reopen() {
    let temporary = TempDir::new().unwrap();
    let root = temporary.path().join("project");
    fs::create_dir(&root).unwrap();
    let database = temporary.path().join("index.sqlite");
    fs::write(
        root.join("a.ts"),
        "import { beta } from './b';\nexport function alpha() { beta(); }\n",
    )
    .unwrap();
    fs::write(
        root.join("b.ts"),
        "import { alpha } from './a';\nexport function beta() { alpha(); }\n",
    )
    .unwrap();
    let mut engine = Engine::open(&root, Some(&database)).unwrap();
    engine.sync().unwrap();
    let id = engine
        .search("alpha", 10, 12000)
        .unwrap()
        .results
        .iter()
        .find(|node| node.name == "alpha")
        .unwrap()
        .id;
    let metrics = engine.metrics(id, 1024).unwrap();
    assert_eq!(metrics.metrics.call_count, 1);
    assert_eq!(metrics.metrics.caller_count, 1);
    assert_eq!(metrics.metrics.depth, 1);
    let cycles = engine.cycles(10, 1024).unwrap();
    assert_eq!(cycles.groups, vec![vec!["a.ts", "b.ts"]]);
    assert!(!cycles.truncated);
    assert_eq!(metrics.generation, cycles.generation);
    fs::write(root.join("b.ts"), "export function beta() {}\n").unwrap();
    assert_eq!(engine.cycles(10, 1024).unwrap().groups, cycles.groups);
    drop(engine);
    let mut engine = Engine::open(&root, Some(&database)).unwrap();
    assert_eq!(engine.cycles(10, 1024).unwrap().groups, cycles.groups);
    engine.sync().unwrap();
    assert!(engine.cycles(10, 1024).unwrap().groups.is_empty());
    assert!(engine.metrics(0, 1024).is_err());
    assert!(engine.metrics(i64::MAX, 1024).is_err());
    assert!(engine.cycles(0, 1024).is_err());
    assert!(engine.cycles(1, 1000).is_err());
    assert!(
        engine
            .dead_code(&DeadCodeOptions {
                max_bytes: 1000,
                ..Default::default()
            })
            .is_err()
    );
}

#[test]
fn cycles_never_present_a_partial_component_as_a_complete_group() {
    let temporary = TempDir::new().unwrap();
    let root = temporary.path().join("project");
    fs::create_dir(&root).unwrap();
    let names: Vec<_> = (0..30)
        .map(|index| format!("module_{index:02}_{}", "x".repeat(40)))
        .collect();
    for (index, name) in names.iter().enumerate() {
        let next = (index + 1) % names.len();
        fs::write(root.join(format!("{name}.ts")),format!("import {{ run{next} }} from './{}';\nexport function run{index}() {{ run{next}(); }}\n",names[next])).unwrap();
    }
    let mut engine = Engine::open(&root, Some(&temporary.path().join("index.sqlite"))).unwrap();
    engine.sync().unwrap();
    let all = engine.cycles(50, 65536).unwrap();
    assert_eq!(all.groups.len(), 1);
    assert_eq!(all.groups[0].len(), 30);
    let bounded = engine.cycles(50, 1024).unwrap();
    assert_eq!(bounded.total, 1);
    assert!(bounded.groups.is_empty());
    assert!(bounded.truncated);
    assert!(serde_json::to_vec(&bounded).unwrap().len() <= 1024);
}
