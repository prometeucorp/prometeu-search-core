use prometeu_search::{Direction, Engine};
use std::fs;
use tempfile::TempDir;

fn project() -> (TempDir, Engine) {
    let temp = TempDir::new().unwrap();
    let root = temp.path().join("project");
    fs::create_dir(&root).unwrap();
    let engine = Engine::open(&root, Some(&temp.path().join("cache/index.sqlite"))).unwrap();
    (temp, engine)
}

#[test]
fn edits_renames_and_deletions_update_symbols_and_references() {
    let (temp, mut engine) = project();
    let root = temp.path().join("project");
    fs::write(root.join("entry.ts"), "function start() { save(); }").unwrap();
    assert_eq!(engine.sync().unwrap().indexed, 1);
    let unresolved = engine
        .related("start", Direction::Callees, 10, 12000)
        .unwrap();
    assert_eq!(unresolved.relations[0].resolution, "unresolved");
    let generation = engine.status().unwrap().generation;
    assert_eq!(engine.sync().unwrap().unchanged, 1);
    assert_eq!(engine.status().unwrap().generation, generation);

    fs::write(
        root.join("store.ts"),
        "export function save() { return 1; }",
    )
    .unwrap();
    assert_eq!(engine.sync().unwrap().indexed, 1);
    let resolved = engine
        .related("start", Direction::Callees, 10, 12000)
        .unwrap();
    assert_eq!(
        resolved.relations[0].candidate.as_ref().unwrap().path,
        "store.ts"
    );
    fs::rename(root.join("store.ts"), root.join("disk.ts")).unwrap();
    let report = engine.sync().unwrap();
    assert_eq!((report.indexed, report.removed), (1, 1));
    assert_eq!(
        engine.search("save", 10, 12000).unwrap().results[0].path,
        "disk.ts"
    );
    fs::write(root.join("entry.ts"), "function start() { load(); }").unwrap();
    fs::remove_file(root.join("disk.ts")).unwrap();
    engine.sync().unwrap();
    assert!(
        engine
            .related("save", Direction::Callers, 10, 12000)
            .unwrap()
            .relations
            .is_empty()
    );
    assert!(engine.search("save", 10, 12000).unwrap().results.is_empty());
}

#[test]
fn ignore_rules_generated_files_and_oversized_sources_are_excluded() {
    let (temp, mut engine) = project();
    let root = temp.path().join("project");
    fs::create_dir_all(root.join("node_modules/lib")).unwrap();
    fs::create_dir(root.join("nested")).unwrap();
    fs::write(root.join(".gitignore"), "secret.ts\n").unwrap();
    fs::write(root.join(".prometeu-searchignore"), "private.rs\n").unwrap();
    fs::write(root.join("nested/.gitignore"), "ignored.ts\n").unwrap();
    for path in [
        "secret.ts",
        "private.rs",
        "node_modules/lib/index.ts",
        "nested/ignored.ts",
    ] {
        fs::write(root.join(path), "function secret() {}").unwrap();
    }
    fs::write(root.join("huge.ts"), "x".repeat(1024 * 1024 + 1)).unwrap();
    fs::write(root.join("binary.ts"), [0xff, 0xfe]).unwrap();
    fs::write(root.join("public.ts"), "function visible() {}").unwrap();
    assert_eq!(engine.sync().unwrap().indexed, 1);
    assert_eq!(engine.status().unwrap().files, 1);
    fs::write(
        root.join(".prometeu-searchignore"),
        "private.rs\npublic.ts\n",
    )
    .unwrap();
    assert_eq!(engine.sync().unwrap().removed, 1);
    assert_eq!(engine.status().unwrap().files, 0);
}

#[test]
fn worktrees_never_reuse_each_others_database() {
    let (temp, mut engine) = project();
    fs::write(temp.path().join("project/first.ts"), "function first() {}").unwrap();
    engine.sync().unwrap();
    let second = temp.path().join("other");
    fs::create_dir(&second).unwrap();
    let database = temp.path().join("cache/index.sqlite");
    let error = Engine::open(&second, Some(&database)).err().unwrap();
    assert!(error.to_string().contains("different project root"));
    let mut other = Engine::open(&second, Some(&temp.path().join("cache/other.sqlite"))).unwrap();
    fs::write(second.join("second.rs"), "fn second() {}").unwrap();
    other.sync().unwrap();
    assert!(other.search("first", 10, 12000).unwrap().results.is_empty());
    assert!(
        engine
            .search("second", 10, 12000)
            .unwrap()
            .results
            .is_empty()
    );
}

#[test]
fn snapshots_preserve_source_until_reconciled_and_detect_same_size_edits() {
    let (temp, mut engine) = project();
    let file = temp.path().join("project/name.ts");
    fs::write(&file, "function alpha() { return 'café'; }").unwrap();
    engine.sync().unwrap();
    fs::write(&file, "function bravo() { return 'café'; }").unwrap();
    let old = engine.search("alpha", 10, 12000).unwrap();
    assert!(old.results[0].snippet.contains("alpha"));
    engine.sync().unwrap();
    assert!(
        engine
            .search("alpha", 10, 12000)
            .unwrap()
            .results
            .is_empty()
    );
    assert!(
        engine.search("bravo", 10, 12000).unwrap().results[0]
            .snippet
            .contains("café")
    );
}

#[test]
fn byte_budgets_include_json_escaping_and_preserve_first_match() {
    let (temp, mut engine) = project();
    let source = format!("function quoted() {{ return '{}'; }}", "\\\"é".repeat(1000));
    fs::write(temp.path().join("project/quotes.ts"), source).unwrap();
    engine.sync().unwrap();
    let result = engine.search("quoted", 10, 1024).unwrap();
    assert_eq!(result.results.len(), 1);
    assert!(result.results[0].snippet_truncated);
    assert!(serde_json::to_vec(&result).unwrap().len() <= 1024);
    assert!(engine.search("\" OR * ()", 10, 1024).is_ok());
    assert!(engine.search("quoted", 0, 1024).is_err());
    assert!(engine.search("quoted", 10, 10).is_err());
}

#[test]
fn ambiguous_targets_stay_candidates_and_results_are_bounded() {
    let (temp, mut engine) = project();
    let root = temp.path().join("project");
    fs::write(root.join("entry.ts"), "function start() { save(); }").unwrap();
    for name in ["first.ts", "second.ts"] {
        fs::write(root.join(name), "function save() {}").unwrap();
    }
    engine.sync().unwrap();
    let result = engine
        .related("start", Direction::Callees, 10, 1024)
        .unwrap();
    assert_eq!(result.relations.len(), 2);
    assert!(
        result
            .relations
            .iter()
            .all(|relation| relation.resolution == "name_match")
    );
    let limited = engine.related("save", Direction::Callers, 1, 1024).unwrap();
    assert_eq!(limited.relations.len(), 1);
    assert!(limited.truncated);
    assert!(serde_json::to_vec(&limited).unwrap().len() <= 1024);
}

#[test]
fn unrelated_or_incompatible_databases_are_rejected_without_mutation() {
    let (temp, _) = project();
    let database = temp.path().join("unrelated.sqlite");
    let connection = rusqlite::Connection::open(&database).unwrap();
    connection
        .execute("CREATE TABLE precious(value TEXT)", [])
        .unwrap();
    connection
        .execute("INSERT INTO precious VALUES('keep')", [])
        .unwrap();
    drop(connection);
    let before = fs::read(&database).unwrap();
    assert!(Engine::open(&temp.path().join("project"), Some(&database)).is_err());
    assert_eq!(fs::read(&database).unwrap(), before);
    let connection = rusqlite::Connection::open(&database).unwrap();
    connection.pragma_update(None, "user_version", 999).unwrap();
    let error = Engine::open(&temp.path().join("project"), Some(&database))
        .err()
        .unwrap();
    assert!(error.to_string().contains("unsupported database schema"));
}

#[test]
fn previous_extraction_cache_requires_rebuild_without_mutation() {
    let (temp, mut engine) = project();
    fs::write(temp.path().join("project/main.ts"), "bootstrap();").unwrap();
    engine.sync().unwrap();
    drop(engine);
    let database = temp.path().join("cache/index.sqlite");
    let connection = rusqlite::Connection::open(&database).unwrap();
    connection.pragma_update(None, "user_version", 1).unwrap();
    drop(connection);
    let before = fs::read(&database).unwrap();
    let error = Engine::open(&temp.path().join("project"), Some(&database))
        .err()
        .unwrap();
    assert!(error.to_string().contains("unsupported database schema 1"));
    assert_eq!(fs::read(&database).unwrap(), before);
}

#[test]
fn concurrent_clients_serialize_index_writes() {
    let (temp, engine) = project();
    let root = temp.path().join("project");
    let database = temp.path().join("cache/index.sqlite");
    fs::write(root.join("file.ts"), "function example() {}").unwrap();
    let threads: Vec<_> = (0..3)
        .map(|_| {
            let root = root.clone();
            let database = database.clone();
            std::thread::spawn(move || {
                let mut client = Engine::open(&root, Some(&database)).unwrap();
                client.sync().unwrap();
                assert_eq!(
                    client.search("example", 10, 12000).unwrap().results.len(),
                    1
                );
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
    assert_eq!(engine.status().unwrap().symbols, 1);
    assert_eq!(engine.status().unwrap().generation, 1);
}

#[cfg(unix)]
#[test]
fn symlinks_cannot_import_source_outside_root() {
    let (temp, mut engine) = project();
    let external = temp.path().join("outside.ts");
    fs::write(&external, "function hiddenSecret() {}").unwrap();
    std::os::unix::fs::symlink(&external, temp.path().join("project/link.ts")).unwrap();
    engine.sync().unwrap();
    assert_eq!(engine.status().unwrap().files, 0);
}

#[test]
fn opening_a_reader_does_not_wait_for_an_existing_index_writer() {
    let (temp, engine) = project();
    let database = temp.path().join("cache/index.sqlite");
    let writer = rusqlite::Connection::open(&database).unwrap();
    writer.execute_batch("BEGIN IMMEDIATE").unwrap();
    let root = temp.path().join("project");
    let (sender, receiver) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
        let result = Engine::open(&root, Some(&database)).and_then(|engine| engine.status());
        sender.send(result.map(|status| status.generation)).unwrap();
    });
    let result = receiver.recv_timeout(std::time::Duration::from_secs(2));
    writer.execute_batch("ROLLBACK").unwrap();
    reader.join().unwrap();
    assert_eq!(
        result.unwrap().unwrap(),
        engine.status().unwrap().generation
    );
}
