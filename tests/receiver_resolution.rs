use prometeu_search::{Direction, Engine, GraphMode, GraphOptions};
use std::fs;
use tempfile::TempDir;

fn project(files: &[(&str, &str)]) -> (TempDir, Engine) {
    let temporary = TempDir::new().unwrap();
    let root = temporary.path().join("project");
    fs::create_dir(&root).unwrap();
    for (path, source) in files {
        fs::write(root.join(path), source).unwrap();
    }
    let mut engine = Engine::open(&root, Some(&temporary.path().join("index.sqlite"))).unwrap();
    engine.sync().unwrap();
    (temporary, engine)
}

#[test]
fn interface_receivers_resolve_local_imported_and_inherited_methods() {
    let (_temporary, engine) = project(&[
        (
            "store.ts",
            "export interface Store { save(): void } export interface Child extends Store {}",
        ),
        (
            "entry.ts",
            "import { Store, Child } from './store';
             interface Local { save(): void }
             function local(value: Local) { value.save(); }
             function imported(value: Store) { value.save(); }
             function inherited(value: Child) { value.save(); }",
        ),
    ]);
    for (caller, path, owner) in [
        ("local", "entry.ts", "Local::save"),
        ("imported", "store.ts", "Store::save"),
        ("inherited", "store.ts", "Store::save"),
    ] {
        let result = engine
            .related(caller, Direction::Callees, 50, 65536)
            .unwrap();
        assert_eq!(result.relations.len(), 1, "{caller}: {result:?}");
        let relation = &result.relations[0];
        let candidate = relation.candidate.as_ref().expect("interface method");
        assert_eq!(
            (candidate.path.as_str(), candidate.qualified_name.as_str()),
            (path, owner)
        );
        assert_eq!(relation.resolution, "receiver_match");
        assert_eq!(relation.confidence, "medium");
        assert!(!relation.ambiguous);
    }
    let path = engine
        .analyze(
            "imported",
            GraphMode::Path,
            &GraphOptions {
                target: Some("Store::save".into()),
                ..GraphOptions::default()
            },
        )
        .unwrap();
    assert_eq!(path.path.len(), 2);
    let impact = engine
        .analyze("Store::save", GraphMode::Impact, &GraphOptions::default())
        .unwrap();
    for caller in ["imported", "inherited"] {
        assert!(
            impact.impact.iter().any(|symbol| symbol.name == caller),
            "{impact:?}"
        );
    }
}

#[test]
fn namespace_receiver_types_follow_exports_and_keep_private_types_unresolved() {
    let (_temporary, engine) = project(&[
        (
            "model.ts",
            "export class Store { save() {} } export interface Writer { write(): void }
             class Hidden { hide() {} }",
        ),
        ("barrel.ts", "export * as model from './model';"),
        ("alias.ts", "export { model as storage } from './barrel';"),
        (
            "entry.ts",
            "import * as model from './model'; import { storage } from './alias';
             function direct(value: model.Store) { value.save(); }
             function signature(value: model.Writer) { value.write(); }
             function reexported(value: storage.Store) { value.save(); }
             function privateType(value: model.Hidden) { value.hide(); }
             function missingType(value: model.Missing) { value.save(); }",
        ),
        (
            "other.ts",
            "export class Store { save() {} } export class Hidden { hide() {} }",
        ),
    ]);
    for (caller, owner) in [
        ("direct", "Store::save"),
        ("signature", "Writer::write"),
        ("reexported", "Store::save"),
    ] {
        let result = engine
            .related(caller, Direction::Callees, 50, 65536)
            .unwrap();
        assert_eq!(result.relations.len(), 1, "{caller}: {result:?}");
        let relation = &result.relations[0];
        let candidate = relation
            .candidate
            .as_ref()
            .expect("exported namespace type");
        assert_eq!(
            (candidate.path.as_str(), candidate.qualified_name.as_str()),
            ("model.ts", owner)
        );
        assert_eq!(relation.resolution, "receiver_match");
        assert_eq!(relation.confidence, "medium");
        assert!(!relation.ambiguous);
    }
    for caller in ["privateType", "missingType"] {
        let result = engine
            .related(caller, Direction::Callees, 50, 65536)
            .unwrap();
        assert_eq!(result.relations.len(), 1, "{caller}: {result:?}");
        assert!(result.relations[0].candidate.is_none(), "{result:?}");
        assert_eq!(result.relations[0].resolution, "unresolved");
    }
}
