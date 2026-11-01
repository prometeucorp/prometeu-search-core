use prometeu_search::Engine;
use rusqlite::Connection;
use serde_json::json;
use std::{
    fs,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};
use tempfile::TempDir;

fn project() -> (TempDir, PathBuf, PathBuf, Engine) {
    let temporary = TempDir::new().unwrap();
    let root = temporary.path().join("project");
    fs::create_dir(&root).unwrap();
    let database = temporary.path().join("index.sqlite");
    let engine = Engine::open(&root, Some(&database)).unwrap();
    (temporary, root, database, engine)
}

fn write(root: &Path, path: &str, content: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

fn configure(root: &Path, settings: serde_json::Value) {
    write(root, "prometeu-search.json", &settings.to_string());
}

fn paths(engine: &Engine) -> Vec<String> {
    engine
        .files("", 50, 65536)
        .unwrap()
        .files
        .into_iter()
        .map(|file| file.path)
        .collect()
}

#[test]
fn extension_changes_reparse_unchanged_files_and_publish_one_snapshot() {
    let (_temporary, root, database, mut engine) = project();
    write(&root, "custom.TPL", "export function hello() {}\n");
    write(&root, "plain.ts", "export function plain() {}\n");
    engine.sync().unwrap();
    assert_eq!(paths(&engine), ["plain.ts"]);
    configure(&root, json!({"extensions":{" TPL ":"javascript"}}));
    // A stale targeted event must still discover configuration and reconcile every file.
    let changed = engine.sync_paths(&[PathBuf::from("plain.ts")]).unwrap();
    assert_eq!((changed.indexed, changed.hashed), (2, 2));
    assert_eq!(
        engine
            .file("custom.TPL", 1, None, 8192)
            .unwrap()
            .file
            .unwrap()
            .language,
        "javascript"
    );
    let old_id = engine.search("hello", 10, 8192).unwrap().results[0].id;
    configure(&root, json!({"extensions":{".tpl":"typescript"}}));
    assert_eq!(engine.refresh().unwrap().indexed, 2);
    assert!(engine.node(old_id, 8192).is_err());
    assert_eq!(
        engine
            .file("custom.TPL", 1, None, 8192)
            .unwrap()
            .file
            .unwrap()
            .language,
        "typescript"
    );
    let conn = Connection::open(&database).unwrap();
    let fingerprint: String = conn
        .query_row(
            "SELECT value FROM metadata WHERE key='project_config_fingerprint'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let unchanged = engine.sync().unwrap();
    assert_eq!(unchanged.indexed, 0);
    fs::write(
        root.join("prometeu-search.json"),
        "{ \"extensions\": {\".tpl\":\"typescript\"} }\n",
    )
    .unwrap();
    let whitespace = engine.refresh().unwrap();
    assert_eq!(whitespace.indexed, 2);
    assert!(whitespace.generation > unchanged.generation);
    let effective: String = conn
        .query_row(
            "SELECT value FROM metadata WHERE key='project_config_fingerprint'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(effective, fingerprint);
    fs::remove_file(root.join("prometeu-search.json")).unwrap();
    assert_eq!(engine.refresh().unwrap().removed, 1);
    assert_eq!(paths(&engine), ["plain.ts"]);
}

#[test]
fn include_and_exclude_follow_reference_precedence_in_full_and_targeted_scans() {
    let (_temporary, root, _database, mut engine) = project();
    write(&root, ".gitignore", "Tools/\nignored.ts\n");
    for path in [
        "plain.ts",
        "ignored.ts",
        "Tools/main.ts",
        "Tools/generated/skip.ts",
        "Tools/hidden.ts",
        "node_modules/dependency.ts",
    ] {
        write(&root, path, "function hello() {}\n");
    }
    configure(
        &root,
        json!({"include":["Tools/","ignored.ts","node_modules/"],"exclude":["Tools/generated/","Tools/hidden.ts"]}),
    );
    engine.sync().unwrap();
    assert_eq!(paths(&engine), ["Tools/main.ts", "ignored.ts", "plain.ts"]);
    write(&root, "Tools/main.ts", "function revised() {}\n");
    let update = engine
        .sync_paths(&[PathBuf::from("Tools/main.ts")])
        .unwrap();
    assert_eq!((update.indexed, update.hashed), (1, 1));
    assert_eq!(
        engine.search("revised", 10, 8192).unwrap().results[0].path,
        "Tools/main.ts"
    );
    configure(
        &root,
        json!({"include":["Tools/"],"exclude":["Tools/","!Tools/main.ts"]}),
    );
    engine.sync().unwrap();
    assert_eq!(paths(&engine), ["plain.ts"]);
}

#[test]
fn include_ignored_restores_embedded_repositories_with_their_own_ignores() {
    let (_temporary, root, _database, mut engine) = project();
    write(&root, ".git/HEAD", "ref: refs/heads/main\n");
    write(&root, ".gitignore", "repos/\n");
    write(&root, "repos/plain.ts", "function ordinary() {}\n");
    write(&root, "repos/clone/.git/HEAD", "ref: refs/heads/main\n");
    write(&root, "repos/clone/.gitignore", "private.ts\n");
    write(&root, "repos/clone/source.ts", "function cloned() {}\n");
    write(
        &root,
        "repos/clone/private.ts",
        "function privateSource() {}\n",
    );
    write(
        &root,
        "repos/submodule/.git",
        "gitdir: ../../.git/modules/submodule\n",
    );
    write(
        &root,
        "repos/submodule/source.ts",
        "function submodule() {}\n",
    );
    write(
        &root,
        "repos/worktree/.git",
        "gitdir: ../../.git/worktrees/second\n",
    );
    write(
        &root,
        "repos/worktree/source.ts",
        "function duplicate() {}\n",
    );
    write(
        &root,
        "repos/module-worktree/.git",
        "gitdir: C:\\root\\.git\\modules\\mod\\worktrees\\copy\n",
    );
    write(
        &root,
        "repos/module-worktree/source.ts",
        "function duplicateModule() {}\n",
    );
    engine.sync().unwrap();
    assert!(paths(&engine).is_empty());
    configure(&root, json!({"includeIgnored":["repos/"]}));
    engine.sync().unwrap();
    assert_eq!(
        paths(&engine),
        ["repos/clone/source.ts", "repos/submodule/source.ts"]
    );
    configure(&root, json!({"includeIgnored":["repos/clone/"]}));
    engine.sync().unwrap();
    assert_eq!(paths(&engine), ["repos/clone/source.ts"]);
    configure(
        &root,
        json!({"includeIgnored":["repos/"],"exclude":["repos/clone/"]}),
    );
    engine.sync().unwrap();
    assert_eq!(paths(&engine), ["repos/submodule/source.ts"]);
}

#[test]
fn malformed_config_defaults_while_read_failures_roll_back() {
    let (_temporary, root, _database, mut engine) = project();
    write(&root, "plain.ts", "function original() {}\n");
    write(&root, "extra.custom", "function custom() {}\n");
    configure(&root, json!({"extensions":{"custom":"javascript"}}));
    engine.sync().unwrap();
    write(&root, "prometeu-search.json", "{ broken JSON");
    assert_eq!(engine.refresh().unwrap().removed, 1);
    let generation = engine.status().unwrap().generation;
    write(&root, "plain.ts", "function pending() {}\n");
    fs::remove_file(root.join("prometeu-search.json")).unwrap();
    fs::create_dir(root.join("prometeu-search.json")).unwrap();
    assert!(engine.refresh().is_err());
    assert_eq!(engine.status().unwrap().generation, generation);
    assert!(
        engine
            .file("plain.ts", 1, None, 8192)
            .unwrap()
            .file
            .unwrap()
            .source
            .contains("original")
    );
}

#[test]
fn deprioritized_files_remain_searchable_and_configuration_is_worktree_local() {
    let (_temporary, root, _database, mut engine) = project();
    write(&root, "aaa/demo.ts", "export function shared() {}\n");
    write(&root, "src/real.ts", "export function shared() {}\n");
    configure(&root, json!({"deprioritize":["aaa/"]}));
    engine.sync().unwrap();
    assert_eq!(
        engine.search("shared", 1, 8192).unwrap().results[0].path,
        "src/real.ts"
    );
    assert!(paths(&engine).contains(&"aaa/demo.ts".to_string()));
    let (_other, second_root, _db, mut second) = project();
    write(&second_root, "aaa/demo.ts", "export function shared() {}\n");
    write(&second_root, "src/real.ts", "export function shared() {}\n");
    second.sync().unwrap();
    assert_eq!(
        second.search("shared", 1, 8192).unwrap().results[0].path,
        "aaa/demo.ts"
    );
}

#[test]
fn go_module_snapshots_change_generation_without_reparsing_source() {
    let (_temporary, root, database, mut engine) = project();
    write(&root, "go.mod", "module example.org/original\n");
    write(&root, "nested/go.mod", "module example.org/nested\n");
    write(&root, "entry.go", "package main\nfunc Hello() {}\n");
    engine.sync().unwrap();
    let generation = engine.status().unwrap().generation;
    write(&root, "go.mod", "module example.org/updated\n");
    let report = engine.sync_paths(&[PathBuf::from("go.mod")]).unwrap();
    assert_eq!(report.indexed, 0);
    assert!(report.generation > generation);
    let conn = Connection::open(&database).unwrap();
    let modules: String = conn
        .query_row(
            "SELECT value FROM metadata WHERE key='resolution_go_modules'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let modules: serde_json::Value = serde_json::from_str(&modules).unwrap();
    assert_eq!(modules["go.mod"], "module example.org/updated\n");
    assert_eq!(modules["nested/go.mod"], "module example.org/nested\n");
    fs::remove_file(root.join("nested/go.mod")).unwrap();
    assert!(engine.refresh().unwrap().generation > report.generation);
}

#[cfg(unix)]
#[test]
fn configuration_and_force_include_cannot_read_through_symlinks() {
    use std::os::unix::fs::symlink;
    let (temporary, root, _database, mut engine) = project();
    write(&root, "entry.ts", "function original() {}\n");
    engine.sync().unwrap();
    let generation = engine.status().unwrap().generation;
    let outside = temporary.path().join("outside.json");
    fs::write(&outside, "{}").unwrap();
    symlink(&outside, root.join("prometeu-search.json")).unwrap();
    assert!(engine.sync().is_err());
    assert_eq!(engine.status().unwrap().generation, generation);
    fs::remove_file(root.join("prometeu-search.json")).unwrap();
    fs::create_dir(temporary.path().join("outside")).unwrap();
    fs::write(
        temporary.path().join("outside/secret.ts"),
        "function secret() {}\n",
    )
    .unwrap();
    symlink(temporary.path().join("outside"), root.join("linked")).unwrap();
    configure(&root, json!({"include":["linked/"]}));
    engine.sync().unwrap();
    assert_eq!(paths(&engine), ["entry.ts"]);
}

#[test]
fn watcher_reloads_config_and_observes_custom_extension_edits() {
    let (_temporary, root, _database, mut engine) = project();
    write(&root, "entry.custom", "function original() {}\n");
    engine.sync().unwrap();
    let _watcher = engine.watch().unwrap();
    configure(&root, json!({"extensions":{"custom":"javascript"}}));
    let wait = |name: &str| {
        let deadline = Instant::now() + Duration::from_secs(8);
        while !engine
            .search(name, 10, 8192)
            .unwrap()
            .results
            .iter()
            .any(|hit| hit.name == name)
        {
            assert!(Instant::now() < deadline, "watcher did not index {name}");
            thread::sleep(Duration::from_millis(50));
        }
    };
    wait("original");
    write(&root, "entry.custom", "function modified() {}\n");
    wait("modified");
}
