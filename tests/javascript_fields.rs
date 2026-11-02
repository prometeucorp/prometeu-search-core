use prometeu_search::{Engine, GraphOptions};
use std::fs;

fn calls(engine: &Engine) -> Vec<(String, serde_json::Value)> {
    let graph = engine
        .explore(
            "run",
            &GraphOptions {
                path: Some("app.ts".into()),
                line: Some(4),
                depth: 8,
                limit: 50,
                max_bytes: 65536,
                ..Default::default()
            },
        )
        .unwrap();
    assert!(!graph.truncated);
    graph
        .edges
        .iter()
        .filter(|edge| edge.kind == "calls")
        .filter_map(|edge| {
            edge.to.map(|to| {
                (
                    engine
                        .node(to, 65536)
                        .unwrap()
                        .node
                        .unwrap()
                        .metadata
                        .unwrap()
                        .file_path,
                    edge.metadata.clone().unwrap(),
                )
            })
        })
        .collect()
}
#[test]
fn declared_field_resolution_tracks_imports_removal_and_committed_generations() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("project");
    fs::create_dir(&root).unwrap();
    let database = temporary.path().join("index.sqlite");
    for name in ["a.ts", "b.ts"] {
        fs::write(root.join(name), "export class Sender { send() {} }\n").unwrap();
    }
    let source = |module: &str, field: &str| {
        format!(
            "import {{ Sender as Service }} from './{module}';\nclass App {{\n  {field}\n  run() {{ this.api.send(); }}\n}}\n"
        )
    };
    fs::write(root.join("app.ts"), source("a", "private api: Service;")).unwrap();
    let mut reader = Engine::open(&root, Some(&database)).unwrap();
    reader.sync().unwrap();
    let mut writer = Engine::open(&root, Some(&database)).unwrap();
    let first = calls(&reader);
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].0, "a.ts");
    assert_eq!(first[0].1["resolvedBy"], "instance-method");
    assert_eq!(first[0].1["confidence"], 0.85);
    fs::write(root.join("app.ts"), source("b", "private api: Service;")).unwrap();
    assert_eq!(calls(&reader), first);
    writer.sync_paths(&["app.ts".into()]).unwrap();
    let second = calls(&reader);
    assert_eq!(second.len(), 1);
    assert_eq!(second[0].0, "b.ts");
    assert_eq!(second[0].1, first[0].1);
    fs::write(root.join("app.ts"), source("b", "")).unwrap();
    writer.sync_paths(&["app.ts".into()]).unwrap();
    assert!(calls(&reader).is_empty());
    drop(reader);
    fs::write(
        root.join("app.ts"),
        source("b", "private api = new Service();"),
    )
    .unwrap();
    let reader = Engine::open(&root, Some(&database)).unwrap();
    assert!(calls(&reader).is_empty());
    writer.sync_paths(&["app.ts".into()]).unwrap();
    let restored = calls(&reader);
    assert_eq!(restored.len(), 1);
    assert_eq!(restored[0].0, "b.ts");
    assert_eq!(restored[0].1, first[0].1);
}

#[test]
fn inherited_member_values_follow_committed_ancestors_without_unrelated_fallback() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("project");
    fs::create_dir(&root).unwrap();
    let database = temporary.path().join("index.sqlite");
    fs::write(root.join("base.ts"), "export class Base { handle() {} }\n").unwrap();
    fs::write(root.join("app.ts"), "import { Base } from './base';\nclass Other { handle() {} }\nclass App extends Base { bind() { listen(this.handle); } }\n").unwrap();
    let mut reader = Engine::open(&root, Some(&database)).unwrap();
    reader.sync().unwrap();
    let read = |engine: &Engine| {
        engine
            .explore(
                "bind",
                &GraphOptions {
                    path: Some("app.ts".into()),
                    depth: 1,
                    limit: 50,
                    max_bytes: 65536,
                    ..Default::default()
                },
            )
            .unwrap()
            .edges
            .into_iter()
            .filter(|edge| {
                edge.metadata
                    .as_ref()
                    .is_some_and(|metadata| metadata["fnRef"] == true)
            })
            .collect::<Vec<_>>()
    };
    let first = read(&reader);
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].kind, "references");
    assert_eq!(first[0].metadata.as_ref().unwrap()["confidence"], 0.85);
    assert_eq!(
        first[0].metadata.as_ref().unwrap()["resolvedBy"],
        "function-ref"
    );
    assert_eq!(
        reader
            .node(first[0].to.unwrap(), 65536)
            .unwrap()
            .node
            .unwrap()
            .metadata
            .unwrap()
            .file_path,
        "base.ts"
    );
    fs::write(root.join("base.ts"), "export class Base { other() {} }\n").unwrap();
    assert_eq!(
        serde_json::to_value(read(&reader)).unwrap(),
        serde_json::to_value(&first).unwrap()
    );
    let mut writer = Engine::open(&root, Some(&database)).unwrap();
    writer.sync_paths(&["base.ts".into()]).unwrap();
    assert!(read(&reader).is_empty());
    drop(reader);
    fs::write(root.join("base.ts"), "export class Base { handle() {} }\n").unwrap();
    let reader = Engine::open(&root, Some(&database)).unwrap();
    assert!(read(&reader).is_empty());
    writer.sync_paths(&["base.ts".into()]).unwrap();
    assert_eq!(read(&reader).len(), 1);
}

#[test]
fn annotated_local_receivers_refresh_and_array_calls_stay_unresolved() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("project");
    fs::create_dir(&root).unwrap();
    let database = temporary.path().join("index.sqlite");
    fs::write(root.join("a.ts"), "export class A { push() {} }\n").unwrap();
    fs::write(root.join("b.ts"), "export class B { push() {} }\n").unwrap();
    let source = |annotation: &str| {
        format!(
            "import {{ A }} from './a';\nimport {{ B }} from './b';\nexport function run(service: {annotation}) {{\n  service.push();\n}}\n"
        )
    };
    fs::write(root.join("app.ts"), source("A")).unwrap();
    let mut reader = Engine::open(&root, Some(&database)).unwrap();
    reader.sync().unwrap();
    let mut writer = Engine::open(&root, Some(&database)).unwrap();
    let first = calls(&reader);
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].0, "a.ts");
    assert_eq!(first[0].1["confidence"], 0.9);
    assert_eq!(first[0].1["resolvedBy"], "instance-method");
    fs::write(root.join("app.ts"), source("B")).unwrap();
    assert_eq!(calls(&reader), first);
    writer.sync_paths(&["app.ts".into()]).unwrap();
    let second = calls(&reader);
    assert_eq!(second.len(), 1);
    assert_eq!(second[0].0, "b.ts");
    fs::write(root.join("app.ts"), source("B[]")).unwrap();
    writer.sync_paths(&["app.ts".into()]).unwrap();
    assert!(calls(&reader).is_empty());
    drop(reader);
    let reader = Engine::open(&root, Some(&database)).unwrap();
    assert!(calls(&reader).is_empty());
    fs::write(root.join("app.ts"), source("B")).unwrap();
    assert!(calls(&reader).is_empty());
    writer.sync_paths(&["app.ts".into()]).unwrap();
    assert_eq!(calls(&reader), second);
}

#[test]
fn ambient_type_exports_follow_creation_alias_changes_rename_and_removal() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("project");
    fs::create_dir(&root).unwrap();
    let database = temporary.path().join("index.sqlite");
    fs::write(
        root.join("package.json"),
        r#"{"dependencies":{"remote":"1"}}"#,
    )
    .unwrap();
    fs::write(
        root.join("app.ts"),
        "import { Service } from 'remote';\nclass App { service: Service; }\n",
    )
    .unwrap();
    let mut reader = Engine::open(&root, Some(&database)).unwrap();
    reader.sync().unwrap();
    let read = |engine: &Engine| {
        engine
            .explore(
                "service",
                &GraphOptions {
                    path: Some("app.ts".into()),
                    depth: 1,
                    limit: 50,
                    max_bytes: 65536,
                    ..Default::default()
                },
            )
            .unwrap()
            .edges
            .into_iter()
            .filter(|edge| {
                edge.kind == "references"
                    && edge
                        .metadata
                        .as_ref()
                        .is_some_and(|metadata| metadata["refName"] == "Service")
            })
            .filter_map(|edge| {
                edge.to.map(|to| {
                    let node = engine
                        .node(to, 65536)
                        .unwrap()
                        .node
                        .unwrap()
                        .metadata
                        .unwrap();
                    (node.file_path, node.name, edge.metadata.unwrap())
                })
            })
            .collect::<Vec<_>>()
    };
    assert!(read(&reader).is_empty());
    let mut writer = Engine::open(&root, Some(&database)).unwrap();
    fs::write(
        root.join("api.d.ts"),
        "declare module 'remote' { interface Local {} export { Local as Service }; }\n",
    )
    .unwrap();
    assert!(read(&reader).is_empty());
    writer.sync_paths(&["api.d.ts".into()]).unwrap();
    let first = read(&reader);
    assert_eq!(first.len(), 1);
    assert_eq!((&*first[0].0, &*first[0].1), ("api.d.ts", "Local"));
    assert_eq!(first[0].2["resolvedBy"], "import");
    fs::rename(root.join("api.d.ts"), root.join("renamed.d.ts")).unwrap();
    assert_eq!(read(&reader), first);
    writer
        .sync_paths(&["api.d.ts".into(), "renamed.d.ts".into()])
        .unwrap();
    assert_eq!(read(&reader)[0].0, "renamed.d.ts");
    fs::write(
        root.join("renamed.d.ts"),
        "declare module 'remote' { interface Local {} export { Local as Other }; }\n",
    )
    .unwrap();
    writer.sync_paths(&["renamed.d.ts".into()]).unwrap();
    assert!(read(&reader).is_empty());
    drop(reader);
    let reader = Engine::open(&root, Some(&database)).unwrap();
    assert!(read(&reader).is_empty());
    fs::write(
        root.join("renamed.d.ts"),
        "declare module 'remote' { export interface Service {} }\n",
    )
    .unwrap();
    writer.sync_paths(&["renamed.d.ts".into()]).unwrap();
    assert_eq!(read(&reader)[0].1, "Service");
    fs::remove_file(root.join("renamed.d.ts")).unwrap();
    writer.sync_paths(&["renamed.d.ts".into()]).unwrap();
    assert!(read(&reader).is_empty());
}
