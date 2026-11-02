use prometeu_search::Engine;
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

fn project() -> (TempDir, PathBuf, Engine) {
    let temporary = TempDir::new().unwrap();
    let root = temporary.path().join("project");
    fs::create_dir(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let engine = Engine::open(&root, Some(&temporary.path().join("index.sqlite"))).unwrap();
    (temporary, root, engine)
}

fn contains(engine: &Engine, name: &str) -> bool {
    engine
        .search(name, 10, 12000)
        .unwrap()
        .results
        .iter()
        .any(|hit| hit.name == name)
}

#[test]
fn targeted_batches_leave_other_changes_for_refresh_and_handle_renames() {
    let (_temporary, root, mut engine) = project();
    fs::create_dir(root.join("nested")).unwrap();
    fs::write(root.join("first.ts"), "function first() {}").unwrap();
    fs::write(root.join("nested/second.ts"), "function second() {}").unwrap();
    engine.sync().unwrap();
    fs::write(root.join("first.ts"), "function changed() {}").unwrap();
    fs::write(root.join("nested/second.ts"), "function pending() {}").unwrap();
    let report = engine.sync_paths(&[PathBuf::from("first.ts")]).unwrap();
    assert_eq!((report.indexed, report.unchanged), (1, 0));
    assert_eq!(report.hashed, 1);
    assert!(contains(&engine, "changed"));
    assert!(contains(&engine, "second"));
    assert!(!contains(&engine, "pending"));
    let report = engine.refresh().unwrap();
    assert_eq!((report.indexed, report.unchanged), (1, 1));
    assert_eq!(report.hashed, 1);
    assert!(contains(&engine, "pending"));
    fs::rename(root.join("first.ts"), root.join("renamed.ts")).unwrap();
    let report = engine
        .sync_paths(&[root.join("first.ts"), root.join("renamed.ts")])
        .unwrap();
    assert_eq!((report.indexed, report.removed), (1, 1));
    assert_eq!(
        engine.search("changed", 10, 12000).unwrap().results[0].path,
        "renamed.ts"
    );
    fs::remove_file(root.join("renamed.ts")).unwrap();
    assert_eq!(
        engine
            .sync_paths(&[root.join("renamed.ts")])
            .unwrap()
            .removed,
        1
    );
    assert!(!contains(&engine, "changed"));
}

#[test]
fn targeted_ignore_rules_match_recovery_and_rule_changes_reconcile_all() {
    let (_temporary, root, mut engine) = project();
    fs::create_dir(root.join("nested")).unwrap();
    fs::create_dir(root.join("node_modules")).unwrap();
    fs::write(root.join(".gitignore"), "ignored.ts\n").unwrap();
    fs::write(root.join("nested/.ignore"), "*.ts\n!allowed.ts\n").unwrap();
    let names = [
        "ignored.ts",
        "nested/blocked.ts",
        "nested/allowed.ts",
        "node_modules/dependency.ts",
        "visible.ts",
    ];
    for path in names {
        fs::write(root.join(path), "function present() {}").unwrap();
    }
    engine
        .sync_paths(&names.map(|path| root.join(path)))
        .unwrap();
    let mut paths: Vec<_> = engine
        .search("present", 10, 12000)
        .unwrap()
        .results
        .into_iter()
        .map(|hit| hit.path)
        .collect();
    paths.sort();
    assert_eq!(paths, vec!["nested/allowed.ts", "visible.ts"]);
    let files = engine.search("kind:file", 10, 12000).unwrap();
    assert_eq!(files.results.len(), 2);
    assert!(
        files
            .results
            .iter()
            .all(|hit| hit.snippet.contains("function present"))
    );
    let generation = engine.status().unwrap().generation;
    assert_eq!(engine.sync().unwrap().indexed, 0);
    assert_eq!(engine.status().unwrap().generation, generation);
    fs::write(root.join(".prometeu-searchignore"), "visible.ts\n").unwrap();
    assert_eq!(
        engine
            .sync_paths(&[root.join(".prometeu-searchignore")])
            .unwrap()
            .removed,
        1
    );
    assert_eq!(engine.status().unwrap().files, 1);
}

#[test]
fn targeted_invalid_sources_and_deleted_source_named_directories_are_removed() {
    let (_temporary, root, mut engine) = project();
    fs::create_dir(root.join("folder.ts")).unwrap();
    fs::write(root.join("folder.ts/source.ts"), "function nested() {}").unwrap();
    fs::write(root.join("source.ts"), "function valid() {}").unwrap();
    engine.sync().unwrap();
    fs::write(root.join("source.ts"), [0xff, 0xfe]).unwrap();
    assert_eq!(
        engine
            .sync_paths(&[root.join("source.ts")])
            .unwrap()
            .removed,
        1
    );
    fs::remove_dir_all(root.join("folder.ts")).unwrap();
    assert_eq!(
        engine
            .sync_paths(&[root.join("folder.ts")])
            .unwrap()
            .removed,
        1
    );
    assert_eq!(engine.status().unwrap().files, 0);
    assert!(
        engine
            .sync_paths(&[PathBuf::from("../outside.ts")])
            .is_err()
    );
    assert!(
        engine
            .sync_paths(&[root.parent().unwrap().join("outside.ts")])
            .is_err()
    );
}

#[cfg(unix)]
#[test]
fn metadata_refresh_detects_same_size_edits_with_restored_mtime() {
    let (_temporary, root, mut engine) = project();
    let path = root.join("source.ts");
    fs::write(&path, "function before() {}").unwrap();
    let modified = fs::metadata(&path).unwrap().modified().unwrap();
    engine.sync().unwrap();
    fs::write(&path, "function after_() {}").unwrap();
    fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(modified))
        .unwrap();
    assert_eq!(engine.refresh().unwrap().indexed, 1);
    assert!(contains(&engine, "after_"));
    assert!(!contains(&engine, "before"));
    let report = engine.refresh().unwrap();
    assert_eq!((report.unchanged, report.hashed), (1, 0));
}

#[cfg(unix)]
#[test]
fn targeted_batches_do_not_read_symlink_sources_or_ancestors() {
    let (temporary, root, mut engine) = project();
    fs::create_dir(root.join("nested")).unwrap();
    fs::write(root.join("nested/source.ts"), "function original() {}").unwrap();
    engine.sync().unwrap();
    let outside = temporary.path().join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("source.ts"), "function secret() {}").unwrap();
    fs::remove_dir_all(root.join("nested")).unwrap();
    std::os::unix::fs::symlink(&outside, root.join("nested")).unwrap();
    engine.sync_paths(&[root.join("nested/source.ts")]).unwrap();
    assert!(!contains(&engine, "secret"));
    assert!(!contains(&engine, "original"));
}
