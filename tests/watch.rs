use prometeu_search::Engine;
use std::fs;
use std::thread;
use std::time::{Duration, Instant};
use tempfile::TempDir;

fn wait_for(mut ready: impl FnMut() -> bool, message: &str) {
    let deadline = Instant::now() + Duration::from_secs(8);
    while !ready() {
        assert!(Instant::now() < deadline, "{message}");
        thread::sleep(Duration::from_millis(50));
    }
}

fn contains(engine: &Engine, name: &str) -> bool {
    engine
        .search(name, 10, 8192)
        .unwrap()
        .results
        .iter()
        .any(|hit| hit.name == name)
}

#[test]
fn watcher_reconciles_edits_and_dotted_directory_rename_and_delete() {
    let temporary = TempDir::new().unwrap();
    let root = temporary.path().join("project");
    let original = root.join("src.v2");
    fs::create_dir_all(&original).unwrap();
    let mut engine = Engine::open(&root, Some(&temporary.path().join("index.sqlite"))).unwrap();
    engine.sync().unwrap();
    let _watcher = engine.watch().unwrap();

    fs::write(
        original.join("source.ts"),
        "export function original() {}\n",
    )
    .unwrap();
    wait_for(
        || contains(&engine, "original"),
        "file creation was not indexed",
    );
    fs::write(original.join("source.ts"), "export function changed() {}\n").unwrap();
    wait_for(
        || contains(&engine, "changed") && !contains(&engine, "original"),
        "file edit was not indexed",
    );

    let renamed = root.join("src.v3");
    fs::rename(&original, &renamed).unwrap();
    wait_for(
        || {
            engine
                .search("changed", 10, 8192)
                .unwrap()
                .results
                .iter()
                .any(|hit| hit.path == "src.v3/source.ts")
        },
        "dotted directory rename was not indexed",
    );
    fs::remove_dir_all(&renamed).unwrap();
    wait_for(
        || !contains(&engine, "changed"),
        "dotted directory removal was not indexed",
    );
}

#[test]
fn follower_takes_over_and_indexes_without_query_reconciliation() {
    let temporary = TempDir::new().unwrap();
    let root = temporary.path().join("project");
    fs::create_dir(&root).unwrap();
    fs::write(root.join("source.rs"), "fn original() {}\n").unwrap();
    let mut engine = Engine::open(&root, Some(&temporary.path().join("index.sqlite"))).unwrap();
    engine.sync().unwrap();
    let owner = engine.watch().unwrap();
    let follower = engine.watch().unwrap();
    assert!(owner.is_owner());
    assert!(!follower.is_owner());
    drop(owner);
    wait_for(|| follower.is_owner(), "follower did not acquire ownership");
    fs::write(root.join("source.rs"), "fn after_handoff() {}\n").unwrap();
    wait_for(
        || contains(&engine, "after_handoff") && !contains(&engine, "original"),
        "follower did not index after owner exit",
    );
}

#[test]
fn extensionless_database_inside_project_does_not_trigger_itself() {
    let temporary = TempDir::new().unwrap();
    fs::write(temporary.path().join("source.rs"), "fn original() {}\n").unwrap();
    let mut engine = Engine::open(temporary.path(), Some(&temporary.path().join("index"))).unwrap();
    engine.sync().unwrap();
    let _watcher = engine.watch().unwrap();
    fs::write(temporary.path().join("source.rs"), "fn changed() {}\n").unwrap();
    wait_for(|| contains(&engine, "changed"), "file edit was not indexed");
    // Let any remaining native event batch drain before checking that the database stays quiet.
    thread::sleep(Duration::from_secs(1));
    let last_indexed = engine.status().unwrap().last_indexed;
    thread::sleep(Duration::from_secs(2));
    assert_eq!(
        engine.status().unwrap().last_indexed,
        last_indexed,
        "index writes triggered a watcher loop"
    );
}
