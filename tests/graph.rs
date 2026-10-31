use prometeu_search::{Direction, Engine, GraphMode, GraphOptions};
use std::fs;
use tempfile::TempDir;

fn project(files: &[(&str, &str)]) -> (TempDir, Engine) {
    let temp = TempDir::new().unwrap();
    let root = temp.path().join("project");
    fs::create_dir(&root).unwrap();
    for (path, source) in files {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source).unwrap();
    }
    let mut engine = Engine::open(&root, Some(&temp.path().join("index.sqlite"))).unwrap();
    engine.sync().unwrap();
    (temp, engine)
}

#[test]
fn imports_aliases_reexports_and_receivers_disambiguate_calls() {
    let (_temp, engine) = project(&[
        (
            "store.ts",
            "export function save() {} export class Store { save() {} } export default class DefaultStore { save() {} }",
        ),
        (
            "other.ts",
            "export function save() {} export class Other { save() {} }",
        ),
        ("barrel.ts", "export { save as persist } from './store';"),
        (
            "entry.ts",
            "import { persist as write } from './barrel'; import * as storage from './store'; import DefaultStore, { Store } from './store'; function start() { write(); storage.save(); const store = new Store(); store.save(); const defaultStore = new DefaultStore(); defaultStore.save(); }",
        ),
    ]);
    let result = engine
        .related("start", Direction::Callees, 50, 65536)
        .unwrap();
    assert!(!result.relations.is_empty());
    for relation in &result.relations {
        if matches!(relation.target.as_str(), "write" | "save") {
            let candidate = relation
                .candidate
                .as_ref()
                .unwrap_or_else(|| panic!("unresolved {}", relation.target));
            assert_eq!(candidate.path, "store.ts");
            assert_ne!(relation.resolution, "name_match");
            assert!(!relation.ambiguous);
        }
    }
    let names: Vec<_> = result
        .relations
        .iter()
        .filter_map(|relation| relation.candidate.as_ref())
        .map(|candidate| candidate.qualified_name.as_str())
        .collect();
    for name in ["save", "Store::save", "DefaultStore::save"] {
        assert!(names.contains(&name), "{names:?}");
    }
}

#[test]
fn known_receivers_and_missing_imports_never_resolve_to_unrelated_methods() {
    let (_temp, engine) = project(&[
        (
            "entry.ts",
            "import { absent } from './missing'; class Empty {} function run(value: Empty) { value.save(); absent(); }",
        ),
        (
            "other.ts",
            "export function absent() {} class Other { save() {} }",
        ),
    ]);
    let result = engine
        .related("run", Direction::Callees, 50, 65536)
        .unwrap();
    assert_eq!(result.relations.len(), 2);
    assert!(
        result
            .relations
            .iter()
            .all(|relation| relation.candidate.is_none() && relation.resolution == "unresolved")
    );
}

#[test]
fn rust_modules_aliases_and_impl_receivers_resolve() {
    let (_temp, engine) = project(&[
        (
            "src/lib.rs",
            "mod disk; use crate::disk::save as persist; struct Store; impl Store { fn save(&self) {} fn run(&self) { self.save(); persist(); disk::save(); } }",
        ),
        ("src/disk.rs", "pub fn save() {}"),
        ("src/other.rs", "pub fn save() {}"),
    ]);
    let result = engine
        .related("Store::run", Direction::Callees, 50, 65536)
        .unwrap();
    assert_eq!(result.relations.len(), 3, "{result:?}");
    assert!(
        result
            .relations
            .iter()
            .all(|relation| relation.candidate.is_some() && relation.resolution != "name_match")
    );
    let candidates: Vec<_> = result
        .relations
        .iter()
        .map(|relation| relation.candidate.as_ref().unwrap().path.as_str())
        .collect();
    assert_eq!(
        candidates
            .iter()
            .filter(|&&path| path == "src/disk.rs")
            .count(),
        2
    );
}

#[test]
fn impact_paths_hierarchy_and_affected_tests_are_transitive() {
    let (_temp, engine) = project(&[
        (
            "leaf.ts",
            "export function leaf() {} export interface Base { save(): void }",
        ),
        (
            "middle.ts",
            "import { leaf, Base } from './leaf'; export function middle() { leaf(); } export class Store implements Base { save() { leaf(); } }",
        ),
        (
            "entry.ts",
            "import { middle } from './middle'; export function entry() { middle(); }",
        ),
        (
            "entry.test.ts",
            "import { entry } from './entry'; function check() { entry(); }",
        ),
    ]);
    let options = GraphOptions {
        depth: 4,
        limit: 50,
        max_bytes: 65536,
        ..GraphOptions::default()
    };
    let impact = engine.analyze("leaf", GraphMode::Impact, &options).unwrap();
    let impacted: Vec<_> = impact
        .impact
        .iter()
        .map(|node| node.name.as_str())
        .collect();
    for name in ["middle", "entry", "check", "save"] {
        assert!(impacted.contains(&name), "{impacted:?}");
    }
    let path = engine
        .analyze(
            "entry",
            GraphMode::Path,
            &GraphOptions {
                target: Some("leaf".into()),
                ..options.clone()
            },
        )
        .unwrap();
    assert_eq!(path.path.len(), 3);
    assert_eq!(path.edges.len(), 2);
    let hierarchy = engine
        .analyze("Base", GraphMode::Implementations, &options)
        .unwrap();
    assert!(hierarchy.impact.iter().any(|node| node.name == "Store"));
    let affected = engine
        .analyze("leaf.ts", GraphMode::Affected, &options)
        .unwrap();
    assert_eq!(affected.affected_tests, ["entry.test.ts"]);
    let bounded = engine
        .analyze(
            "leaf",
            GraphMode::Impact,
            &GraphOptions {
                depth: 1,
                ..options
            },
        )
        .unwrap();
    assert!(bounded.truncated);
}

#[test]
fn explore_combines_source_and_edges_and_preserves_snapshot_and_byte_limits() {
    let (temp, mut engine) = project(&[(
        "entry.ts",
        "function leaf() { return 'café'; } function entry() { leaf(); }",
    )]);
    let options = GraphOptions {
        max_bytes: 65536,
        ..GraphOptions::default()
    };
    let explored = engine.explore("entry", &options).unwrap();
    assert!(
        explored
            .files
            .iter()
            .flat_map(|file| &file.symbols)
            .any(|symbol| symbol.name == "leaf" && symbol.snippet.contains("café"))
    );
    assert!(explored.edges.iter().any(|edge| edge.kind == "calls"));
    fs::write(
        temp.path().join("project/entry.ts"),
        "function replacement() {}",
    )
    .unwrap();
    assert!(
        engine
            .explore("entry", &options)
            .unwrap()
            .files
            .iter()
            .flat_map(|file| &file.symbols)
            .any(|symbol| symbol.snippet.contains("leaf"))
    );
    engine.sync().unwrap();
    assert!(
        engine
            .explore("entry", &options)
            .unwrap()
            .roots
            .iter()
            .all(|node| node.name != "entry")
    );
    fs::write(
        temp.path().join("project/entry.ts"),
        format!("function quoted() {{ return '{}'; }}", "\\\"é".repeat(3000)),
    )
    .unwrap();
    engine.sync().unwrap();
    let bounded = engine
        .explore(
            "quoted",
            &GraphOptions {
                max_bytes: 1024,
                ..options
            },
        )
        .unwrap();
    assert!(serde_json::to_vec(&bounded).unwrap().len() <= 1024);
    assert!(bounded.truncated);
    assert!(bounded.files[0].symbols[0].snippet_truncated);
}

#[test]
fn selectors_and_invalid_bounds_are_explicit() {
    let (_temp, engine) = project(&[("a.ts", "function run() {}"), ("b.ts", "function run() {}")]);
    let selected = engine
        .explore(
            "run",
            &GraphOptions {
                path: Some("b.ts".into()),
                line: Some(1),
                ..GraphOptions::default()
            },
        )
        .unwrap();
    assert_eq!(selected.roots.len(), 1);
    assert_eq!(selected.roots[0].path, "b.ts");
    for options in [
        GraphOptions {
            depth: 0,
            ..GraphOptions::default()
        },
        GraphOptions {
            depth: 9,
            ..GraphOptions::default()
        },
        GraphOptions {
            path: Some("../a.ts".into()),
            ..GraphOptions::default()
        },
        GraphOptions {
            line: Some(1),
            ..GraphOptions::default()
        },
        GraphOptions {
            target: Some("run".into()),
            ..GraphOptions::default()
        },
    ] {
        assert!(engine.explore("run", &options).is_err());
    }
    assert!(
        engine
            .analyze("run", GraphMode::Path, &GraphOptions::default())
            .is_err()
    );
}

#[test]
fn reexport_namespace_boundaries_and_default_exports_stay_exact() {
    let (_temp, engine) = project(&[
        (
            "store.ts",
            "export function save() {} export default function hidden() {}",
        ),
        ("star.ts", "export * from './store';"),
        ("namespace.ts", "export * as storage from './store';"),
        (
            "entry.ts",
            "import hidden from './star'; import { save, storage } from './namespace'; function invalid() { hidden(); save(); } function valid() { storage.save(); }",
        ),
    ]);
    let invalid = engine
        .related("invalid", Direction::Callees, 50, 65536)
        .unwrap();
    assert_eq!(invalid.relations.len(), 2);
    assert!(
        invalid
            .relations
            .iter()
            .all(|relation| relation.candidate.is_none()),
        "{invalid:?}"
    );
    let valid = engine
        .related("valid", Direction::Callees, 50, 65536)
        .unwrap();
    assert_eq!(valid.relations.len(), 1);
    assert_eq!(
        valid.relations[0].candidate.as_ref().unwrap().path,
        "store.ts"
    );
}

#[test]
fn exact_exploration_marks_ambiguous_roots_and_resolution_caps_mark_truncation() {
    let (temp, mut engine) = project(&[
        ("a.ts", "export function run() {}"),
        ("b.ts", "export function run() {}"),
    ]);
    let explored = engine.explore("run", &GraphOptions::default()).unwrap();
    assert_eq!(explored.roots.len(), 2);
    assert!(explored.ambiguous);
    let root = temp.path().join("project");
    for index in 0..34 {
        fs::write(
            root.join(format!("barrel{index}.ts")),
            format!("export {{ run }} from './barrel{}';", index + 1),
        )
        .unwrap();
    }
    fs::write(root.join("barrel34.ts"), "export function run() {}").unwrap();
    fs::write(
        root.join("entry.ts"),
        "import { run } from './barrel0'; function start() { run(); }",
    )
    .unwrap();
    engine.sync().unwrap();
    let capped = engine
        .related("start", Direction::Callees, 10, 12000)
        .unwrap();
    assert!(capped.truncated);
    assert!(capped.relations[0].candidate.is_none());
}

#[test]
fn rust_public_use_binds_locally_and_module_aliases_resolve() {
    let (_temp, engine) = project(&[
        (
            "src/lib.rs",
            "mod disk; mod entry; pub use crate::disk::save as persist; fn run() { persist(); }",
        ),
        ("src/disk.rs", "pub fn save() {}"),
        (
            "src/entry.rs",
            "use crate::disk as storage; fn start() { storage::save(); }",
        ),
    ]);
    for name in ["run", "start"] {
        let result = engine.related(name, Direction::Callees, 10, 12000).unwrap();
        assert_eq!(result.relations.len(), 1, "{result:?}");
        assert_eq!(
            result.relations[0].candidate.as_ref().unwrap().path,
            "src/disk.rs"
        );
        assert_eq!(result.relations[0].resolution, "import_match");
    }
}

#[test]
fn rust_impls_follow_imported_type_identity_across_files() {
    let (_temp, engine) = project(&[
        (
            "src/lib.rs",
            "mod model; mod methods; mod other; pub use model::Store; fn run() { let s = Store::new(); s.save(); }",
        ),
        ("src/model.rs", "pub struct Store;"),
        (
            "src/methods.rs",
            "use crate::Store; impl Store { pub fn new() -> Self { Store } pub fn save(&self) {} pub fn run(&self) { self.save(); } }",
        ),
        (
            "src/other.rs",
            "pub struct Store; impl Store { pub fn save(&self) {} }",
        ),
    ]);
    for name in ["run", "Store::run"] {
        let result = engine.related(name, Direction::Callees, 50, 65536).unwrap();
        assert!(!result.relations.is_empty());
        for relation in result.relations {
            assert_eq!(
                relation.candidate.as_ref().unwrap().path,
                "src/methods.rs",
                "{relation:?}"
            );
            assert_ne!(relation.resolution, "name_match");
        }
    }
}
