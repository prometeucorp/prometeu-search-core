use prometeu_search::{Engine, GraphOptions, GraphResult};
use std::{fs, path::Path};
fn write(root: &Path, path: &str, source: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, source).unwrap();
}
fn resolves_store(result: &GraphResult) -> bool {
    result
        .edges
        .iter()
        .any(|edge| edge.target == "AccountStore" && edge.to.is_some())
}
#[test]
fn go_dot_import_visibility_uses_only_committed_module_configuration() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("project");
    fs::create_dir(&root).unwrap();
    let database = temporary.path().join("index.sqlite");
    write(&root, "go.mod", "module example.org/app\ngo 1.22\n");
    write(
        &root,
        "store.go",
        "package main\ntype AccountStore struct{}\n",
    );
    write(
        &root,
        "external_test.go",
        "package main_test\nimport . \"example.org/app\"\nfunc run(){ _ = AccountStore{} }\n",
    );
    let options = GraphOptions {
        path: Some("external_test.go".into()),
        limit: 30,
        max_bytes: 65536,
        depth: 1,
        ..Default::default()
    };
    let mut first = Engine::open(&root, Some(&database)).unwrap();
    first.sync().unwrap();
    let before = first.explore("run", &options).unwrap();
    assert!(resolves_store(&before));
    write(&root, "go.mod", "module example.org/renamed\ngo 1.22\n");
    assert!(resolves_store(&first.explore("run", &options).unwrap()));
    let mut second = Engine::open(&root, Some(&database)).unwrap();
    let report = second.sync_paths(&["go.mod".into()]).unwrap();
    assert_eq!(report.indexed, 0);
    let after = first.explore("run", &options).unwrap();
    assert!(after.generation > before.generation);
    assert!(!resolves_store(&after));
    drop(first);
    write(&root, "go.mod", "module example.org/app\ngo 1.22\n");
    let reopened = Engine::open(&root, Some(&database)).unwrap();
    assert!(!resolves_store(&reopened.explore("run", &options).unwrap()));
    second.sync_paths(&["go.mod".into()]).unwrap();
    assert!(resolves_store(&reopened.explore("run", &options).unwrap()));
}

#[test]
fn indexed_go_receiver_tracks_imported_types_across_commits_and_reopening() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("project");
    fs::create_dir(&root).unwrap();
    let database = temporary.path().join("index.sqlite");
    write(&root, "go.mod", "module example.org/app\ngo 1.22\n");
    write(
        &root,
        "main.go",
        "package main\nimport m \"example.org/app/model\"\nfunc run(xs []m.Item){xs[0].Save()}\n",
    );
    write(
        &root,
        "model/model.go",
        "package model\ntype Item struct{}\nfunc(x Item) Save() {}\n",
    );
    write(
        &root,
        "other/model.go",
        "package other\ntype Item struct{}\nfunc(x Item) Save() {}\n",
    );
    let options = GraphOptions {
        path: Some("main.go".into()),
        limit: 30,
        max_bytes: 65536,
        depth: 1,
        ..Default::default()
    };
    let selected = |engine: &Engine| {
        let result = engine.explore("run", &options).unwrap();
        result
            .edges
            .iter()
            .filter(|edge| edge.target == "Save")
            .filter_map(|edge| edge.to)
            .filter_map(|id| {
                result
                    .files
                    .iter()
                    .find(|file| file.symbols.iter().any(|symbol| symbol.id == id))
            })
            .map(|file| file.path.clone())
            .collect::<Vec<_>>()
    };
    let mut first = Engine::open(&root, Some(&database)).unwrap();
    first.sync().unwrap();
    assert_eq!(selected(&first), ["model/model.go"]);
    write(
        &root,
        "model/model.go",
        "package model\ntype Item struct{}\n",
    );
    assert_eq!(selected(&first), ["model/model.go"]);
    let mut second = Engine::open(&root, Some(&database)).unwrap();
    second.sync_paths(&["model/model.go".into()]).unwrap();
    assert!(selected(&first).is_empty());
    drop(first);
    let reopened = Engine::open(&root, Some(&database)).unwrap();
    assert!(selected(&reopened).is_empty());
    write(
        &root,
        "model/model.go",
        "package model\ntype Item struct{ Base }\ntype Base struct{}\nfunc(x Base) Save() {}\n",
    );
    second.sync_paths(&["model/model.go".into()]).unwrap();
    assert_eq!(selected(&reopened), ["model/model.go"]);
}

#[test]
fn skipped_go_method_declarations_veto_inherited_targets_from_the_committed_snapshot() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("project");
    fs::create_dir(&root).unwrap();
    let database = temporary.path().join("index.sqlite");
    write(&root, "go.mod", "module example.org/app\ngo 1.22\n");
    write(
        &root,
        "main.go",
        "package main\ntype Base struct{}\nfunc(x Base) Save() {}\ntype Item struct{ Base }\nfunc run(xs []Item){xs[0].Save()}\n",
    );
    let large = format!(
        "package main\nfunc(x Item) Save() {{}}\n{}",
        "// padding\n".repeat(110000)
    );
    write(&root, "generated.pb.go", &large);
    let options = GraphOptions {
        path: Some("main.go".into()),
        limit: 30,
        max_bytes: 65536,
        depth: 1,
        ..Default::default()
    };
    let linked = |engine: &Engine| {
        engine
            .explore("run", &options)
            .unwrap()
            .edges
            .iter()
            .any(|edge| edge.target == "Save" && edge.to.is_some())
    };
    let mut first = Engine::open(&root, Some(&database)).unwrap();
    first.sync().unwrap();
    assert!(!linked(&first));
    fs::remove_file(root.join("generated.pb.go")).unwrap();
    assert!(!linked(&first));
    let mut second = Engine::open(&root, Some(&database)).unwrap();
    second.sync_paths(&["generated.pb.go".into()]).unwrap();
    assert!(linked(&first));
    drop(first);
    write(&root, "generated.pb.go", &large);
    let reopened = Engine::open(&root, Some(&database)).unwrap();
    assert!(linked(&reopened));
    second.sync_paths(&["generated.pb.go".into()]).unwrap();
    assert!(!linked(&reopened));
}
