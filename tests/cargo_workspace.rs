use prometeu_search::{Engine, GraphOptions, GraphResult};
use std::{fs, path::Path};
fn write(root: &Path, path: &str, source: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, source).unwrap();
}
fn manifest(member: &str) -> String {
    format!("[workspace]\nmembers=[\"{member}\"]\n")
}
fn target(result: &GraphResult) -> &str {
    let edge = result
        .edges
        .iter()
        .find(|edge| {
            edge.metadata.as_ref().is_some_and(|metadata| {
                metadata["framework"] == "rust" && metadata["confidence"] == 0.95
            })
        })
        .unwrap();
    let id = edge.to.unwrap();
    &result
        .files
        .iter()
        .find(|file| file.symbols.iter().any(|symbol| symbol.id == id))
        .unwrap()
        .path
}
#[test]
fn cargo_member_changes_commit_atomically_and_survive_cache_reopening() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("project");
    fs::create_dir(&root).unwrap();
    let database = temporary.path().join("index.sqlite");
    let old = "a/b/c/d/e/f/g/old";
    let new = "a/b/c/d/e/f/g/new";
    write(&root, "Cargo.toml", &manifest(old));
    write(
        &root,
        "src/main.rs",
        "use core_api::build;\npub fn run(){build();}\n",
    );
    for member in [old, new] {
        write(
            &root,
            &format!("{member}/Cargo.toml"),
            "[package]\nname=\"core-api\"\n",
        );
        write(&root, &format!("{member}/src/lib.rs"), "pub fn build(){}\n");
    }
    let options = GraphOptions {
        limit: 30,
        max_bytes: 65536,
        depth: 1,
        ..Default::default()
    };
    let mut first = Engine::open(&root, Some(&database)).unwrap();
    first.sync().unwrap();
    let before = first.explore("src/main.rs", &options).unwrap();
    assert_eq!(target(&before), format!("{old}/src/lib.rs"));
    write(&root, "Cargo.toml", &manifest(new));
    assert_eq!(
        target(&first.explore("src/main.rs", &options).unwrap()),
        format!("{old}/src/lib.rs")
    );
    let mut second = Engine::open(&root, Some(&database)).unwrap();
    let report = second.sync_paths(&["Cargo.toml".into()]).unwrap();
    assert_eq!(report.indexed, 0);
    let after = first.explore("src/main.rs", &options).unwrap();
    assert!(after.generation > before.generation);
    assert_eq!(target(&after), format!("{new}/src/lib.rs"));
    drop(first);
    write(&root, "Cargo.toml", &manifest(old));
    let reopened = Engine::open(&root, Some(&database)).unwrap();
    assert_eq!(
        target(&reopened.explore("src/main.rs", &options).unwrap()),
        format!("{new}/src/lib.rs")
    );
    write(&root, "Cargo.toml", &manifest(new));
    write(
        &root,
        &format!("{new}/Cargo.toml"),
        &"x".repeat(1024 * 1024 + 1),
    );
    assert!(
        second
            .sync_paths(&[format!("{new}/Cargo.toml").into()])
            .is_err()
    );
    let unchanged = reopened.explore("src/main.rs", &options).unwrap();
    assert_eq!(unchanged.generation, after.generation);
    assert_eq!(target(&unchanged), format!("{new}/src/lib.rs"));
}
#[cfg(unix)]
#[test]
fn explicit_cargo_members_never_read_symlinked_manifests() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("project");
    fs::create_dir(&root).unwrap();
    let database = temporary.path().join("index.sqlite");
    write(&root, "Cargo.toml", &manifest("deep/a/b/c/d/e/f/member"));
    write(&root, "src/lib.rs", "pub fn own(){}\n");
    let mut engine = Engine::open(&root, Some(&database)).unwrap();
    engine.sync().unwrap();
    let generation = engine.status().unwrap().generation;
    let outside = temporary.path().join("outside.toml");
    fs::write(&outside, "[package]\nname=\"secret\"\n").unwrap();
    let member = root.join("deep/a/b/c/d/e/f/member/Cargo.toml");
    fs::create_dir_all(member.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(outside, member).unwrap();
    assert!(engine.sync().is_err());
    assert_eq!(engine.status().unwrap().generation, generation);
}
