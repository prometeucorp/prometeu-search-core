use prometeu_search::{Direction, Engine, GraphMode, GraphOptions};
use std::fs;
use tempfile::TempDir;

fn project(files: &[(&str, &str)]) -> (TempDir, Engine) {
    let temporary = TempDir::new().unwrap();
    let root = temporary.path().join("project");
    fs::create_dir(&root).unwrap();
    for (path, source) in files {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source).unwrap();
    }
    let mut engine = Engine::open(&root, Some(&temporary.path().join("index.sqlite"))).unwrap();
    engine.sync().unwrap();
    (temporary, engine)
}

#[test]
fn imported_bindings_reexport_under_aliases_and_anonymous_defaults_resolve() {
    let (_temporary, engine) = project(&[
        ("store.ts", "export function save() {}"),
        (
            "barrel.ts",
            "import { save as persist } from './store'; export { persist as write };",
        ),
        ("default.ts", "export default () => {};"),
        (
            "entry.ts",
            "import { write } from './barrel'; import start from './default'; function run() { write(); start(); }",
        ),
        ("other.ts", "function write() {} function start() {}"),
    ]);
    let result = engine
        .related("run", Direction::Callees, 50, 65536)
        .unwrap();
    assert_eq!(result.relations.len(), 2, "{result:?}");
    for (target, path, name) in [
        ("write", "store.ts", "save"),
        ("start", "default.ts", "default"),
    ] {
        let relation = result
            .relations
            .iter()
            .find(|relation| relation.target == target)
            .unwrap();
        let candidate = relation.candidate.as_ref().unwrap();
        assert_eq!(
            (candidate.path.as_str(), candidate.name.as_str()),
            (path, name)
        );
        assert_eq!(relation.resolution, "import_match");
        assert!(!relation.ambiguous);
    }
}

#[test]
fn typed_parameter_scan_preserves_reference_behavior_for_same_line_siblings() {
    let (_temporary, engine) = project(&[
        ("store.ts", "export class Store { save() {} }"),
        ("other.ts", "export class Other { save() {} }"),
        (
            "entry.ts",
            "import { Store } from './store'; function run(value: Store) { value.save(); } function separate(value) { value.save(); }",
        ),
    ]);
    let typed = engine
        .related("run", Direction::Callees, 50, 65536)
        .unwrap();
    let calls = typed
        .relations
        .iter()
        .filter(|relation| relation.kind == "calls")
        .collect::<Vec<_>>();
    assert_eq!(calls.len(), 1);
    let relation = calls[0];
    assert_eq!(relation.resolution, "typed_receiver_match");
    assert_eq!(
        relation.candidate.as_ref().unwrap().qualified_name,
        "Store::save"
    );
    assert_eq!(relation.candidate.as_ref().unwrap().path, "store.ts");
    assert!(typed.relations.iter().any(|relation| {
        relation.kind == "references"
            && relation.resolution == "import_match"
            && relation.candidate.as_ref().is_some_and(|candidate| {
                candidate.qualified_name == "Store" && candidate.path == "store.ts"
            })
    }));
    let untyped = engine
        .related("separate", Direction::Callees, 50, 65536)
        .unwrap();
    assert_eq!(untyped.relations.len(), 1);
    let relation = &untyped.relations[0];
    assert_eq!(relation.resolution, "typed_receiver_match");
    assert_eq!(relation.confidence, "medium");
    assert_eq!(
        relation.candidate.as_ref().unwrap().qualified_name,
        "Store::save"
    );
}

#[test]
fn ruby_required_classes_and_self_receivers_disambiguate_methods() {
    let (_temporary, engine) = project(&[
        ("store.rb", "class Store\n def save!; end\nend"),
        (
            "other.rb",
            "class Other\n def save!; end\n def finish; end\nend",
        ),
        (
            "entry.rb",
            "require_relative 'store'\nclass Worker\n def run\n  store = Store.new\n  store.save!\n  self.finish\n end\n def finish; end\nend",
        ),
    ]);
    let result = engine
        .related("Worker::run", Direction::Callees, 50, 65536)
        .unwrap();
    for (name, path, qualified) in [
        ("save!", "store.rb", "Store::save!"),
        ("finish", "entry.rb", "Worker::finish"),
    ] {
        let matches: Vec<_> = result
            .relations
            .iter()
            .filter(|relation| relation.target == name)
            .collect();
        assert_eq!(matches.len(), 1, "{result:?}");
        let relation = matches[0];
        let candidate = relation.candidate.as_ref().unwrap();
        assert_eq!(
            (candidate.path.as_str(), candidate.qualified_name.as_str()),
            (path, qualified)
        );
        assert_ne!(relation.resolution, "name_match");
        assert!(!relation.ambiguous);
    }
}

#[test]
fn rust_nested_use_aliases_and_trait_implementations_resolve() {
    let (_temporary, engine) = project(&[
        (
            "src/lib.rs",
            "mod disk; use crate::disk::{nested::{write as persist}, Runnable}; struct Worker; impl Runnable for Worker { fn run(&self) { persist(); } }",
        ),
        (
            "src/disk.rs",
            "pub mod nested; pub trait Runnable { fn run(&self); }",
        ),
        ("src/disk/nested.rs", "pub fn write() {}"),
        ("src/other.rs", "pub fn persist() {} pub fn write() {}"),
    ]);
    let result = engine
        .related("Worker::run", Direction::Callees, 50, 65536)
        .unwrap();
    assert_eq!(result.relations.len(), 1, "{result:?}");
    let relation = &result.relations[0];
    let candidate = relation.candidate.as_ref().unwrap();
    assert_eq!(
        (candidate.path.as_str(), candidate.name.as_str()),
        ("src/disk/nested.rs", "write")
    );
    assert_eq!(relation.resolution, "import_match");
    let hierarchy = engine
        .analyze(
            "Runnable",
            GraphMode::Implementations,
            &GraphOptions {
                limit: 50,
                max_bytes: 65536,
                ..GraphOptions::default()
            },
        )
        .unwrap();
    assert!(
        hierarchy
            .files
            .iter()
            .flat_map(|file| &file.symbols)
            .any(|symbol| symbol.name == "Worker"),
        "{hierarchy:?}"
    );
}

#[test]
fn rust_function_imports_shadow_parent_declarations_and_private_imports_stay_unresolved() {
    let (_temporary, engine) = project(&[
        (
            "src/lib.rs",
            "mod disk; fn save() {} fn run() { use crate::disk::save; save(); } use crate::disk::hidden; fn private_call() { hidden(); }",
        ),
        ("src/disk.rs", "pub fn save() {} fn hidden() {}"),
        ("src/other.rs", "pub fn hidden() {}"),
    ]);
    let scoped = engine
        .related("run", Direction::Callees, 50, 65536)
        .unwrap();
    assert_eq!(scoped.relations.len(), 1, "{scoped:?}");
    assert_eq!(
        scoped.relations[0].candidate.as_ref().unwrap().path,
        "src/disk.rs"
    );
    assert_eq!(scoped.relations[0].resolution, "import_match");
    let private = engine
        .related("private_call", Direction::Callees, 50, 65536)
        .unwrap();
    assert_eq!(private.relations.len(), 1, "{private:?}");
    assert!(private.relations[0].candidate.is_none());
    assert_eq!(private.relations[0].resolution, "unresolved");
}

#[test]
fn same_line_nested_receiver_scan_matches_the_reference_heuristic() {
    let (_temporary, engine) = project(&[
        ("store.ts", "export class Store { save() {} }"),
        ("other.ts", "export class Other { save() {} }"),
        (
            "entry.ts",
            "import { Store } from './store'; function outer(value: Store) { function nested(value) { value.save(); } }",
        ),
    ]);
    let result = engine
        .related("outer::nested", Direction::Callees, 50, 65536)
        .unwrap();
    assert_eq!(result.relations.len(), 1, "{result:?}");
    let relation = &result.relations[0];
    assert_eq!(relation.resolution, "typed_receiver_match");
    assert_eq!(relation.confidence, "medium");
    assert_eq!(
        relation.candidate.as_ref().unwrap().qualified_name,
        "Store::save"
    );
}

#[test]
fn parameters_local_values_and_temporal_dead_zones_hide_imported_calls() {
    for extension in ["ts", "js"] {
        let parameter = if extension == "ts" {
            "save: () => void"
        } else {
            "save"
        };
        let source = format!(
            "import {{ save }} from './store';\nfunction parameter({parameter}) {{ save(); }}\nfunction localValue() {{ const save = unknown(); save(); }}\nfunction beforeBinding() {{ save(); const save = unknown(); }}\nfunction localArrow() {{ const save = () => {{}}; save(); }}"
        );
        let (_temporary, engine) = project(&[
            ("store.ts", "export function save() {}"),
            (&format!("entry.{extension}"), &source),
        ]);
        for caller in ["parameter", "localValue", "beforeBinding"] {
            let result = engine
                .related(caller, Direction::Callees, 50, 65536)
                .unwrap();
            let calls: Vec<_> = result
                .relations
                .iter()
                .filter(|relation| relation.target == "save")
                .collect();
            assert_eq!(calls.len(), 1, "{extension} {caller}: {result:?}");
            assert_eq!(
                calls[0].resolution, "unresolved",
                "{extension} {caller}: {result:?}"
            );
            assert!(calls[0].candidate.is_none());
        }
        let arrow = engine
            .related("localArrow", Direction::Callees, 50, 65536)
            .unwrap();
        assert_eq!(arrow.relations.len(), 1, "{arrow:?}");
        assert_eq!(arrow.relations[0].resolution, "scope_match");
        assert_eq!(
            arrow.relations[0]
                .candidate
                .as_ref()
                .unwrap()
                .qualified_name,
            "localArrow::save"
        );
    }
}

#[test]
fn javascript_side_effect_imports_keep_dependencies_without_inventing_bindings() {
    let (_temporary, engine) = project(&[
        ("store.ts", "export function save() {}"),
        ("other.ts", "export function save() {}"),
        ("entry.ts", "import './store'; function run() { save(); }"),
        ("entry.test.ts", "import './entry';"),
    ]);
    let result = engine
        .related("run", Direction::Callees, 50, 65536)
        .unwrap();
    assert_eq!(result.relations.len(), 2, "{result:?}");
    assert!(
        result
            .relations
            .iter()
            .all(|relation| relation.resolution == "name_match" && relation.ambiguous)
    );
    let affected = engine
        .analyze(
            "store.ts",
            GraphMode::Affected,
            &GraphOptions {
                depth: 4,
                limit: 50,
                max_bytes: 65536,
                ..GraphOptions::default()
            },
        )
        .unwrap();
    assert!(
        affected
            .affected_tests
            .contains(&"entry.test.ts".to_string()),
        "{affected:?}"
    );
}

#[test]
fn ruby_explicit_calls_are_methods_even_with_same_named_local_values() {
    let (_temporary, engine) = project(&[(
        "entry.rb",
        "class Worker\n def run\n  save = 1\n  save()\n end\n def save; end\nend",
    )]);
    let result = engine
        .related("Worker::run", Direction::Callees, 50, 65536)
        .unwrap();
    assert_eq!(result.relations.len(), 1, "{result:?}");
    assert_eq!(
        result.relations[0]
            .candidate
            .as_ref()
            .unwrap()
            .qualified_name,
        "Worker::save"
    );
}

#[test]
fn rust_trait_impls_attach_to_imported_aliased_owners() {
    let (_temporary, engine) = project(&[
        (
            "src/lib.rs",
            "mod implementation; pub struct Store; pub trait Runnable { fn run(&self); }",
        ),
        (
            "src/implementation.rs",
            "use crate::{Store as LocalStore, Runnable as LocalTrait}; impl LocalTrait for LocalStore { fn run(&self) {} }",
        ),
    ]);
    let hierarchy = engine
        .analyze(
            "Runnable",
            GraphMode::Implementations,
            &GraphOptions {
                depth: 4,
                limit: 50,
                max_bytes: 65536,
                ..GraphOptions::default()
            },
        )
        .unwrap();
    let symbols: Vec<_> = hierarchy
        .files
        .iter()
        .flat_map(|file| &file.symbols)
        .collect();
    let store = symbols
        .iter()
        .find(|symbol| symbol.name == "Store")
        .unwrap_or_else(|| panic!("{hierarchy:?}"));
    let runnable = symbols
        .iter()
        .find(|symbol| symbol.name == "Runnable")
        .unwrap();
    assert!(
        hierarchy.edges.iter().any(|edge| edge.from == store.id
            && edge.to == Some(runnable.id)
            && edge.kind == "implements"),
        "{hierarchy:?}"
    );
}

#[test]
fn destructuring_and_catch_bindings_hide_imports_only_in_their_scopes() {
    for extension in ["ts", "js"] {
        let source = r#"import { save } from './store';
function objectParameter({save}) { save(); }
function arrayParameter([save]) { save(); }
function aliasParameter({other: save}) { save(); }
function nestedParameter({nested: {save}}) { save(); }
function defaultParameter({save = fallback}) { save(); }
function restParameter(...save) { save(); }
function arrayRestParameter([...save]) { save(); }
function objectRestParameter({...save}) { save(); }
function localObject() { const {save} = input; save(); }
function localArray() { let [save] = input; save(); }
function beforeBinding() { save(); const {save} = input; }
function functionScoped() { { var {save} = input; } save(); }
function propertyKey({save: other}) { save(); }
function defaultExpression({other = save()}) {}
function catchScope() {
  try { throw null; } catch (save) { save(); }
  save();
}
function catchPattern() {
  try { throw null; } catch ({save}) { save(); }
  save();
}
function blockScope() {
  { const {save} = input; save(); }
  save();
}
"#;
        let (_temporary, engine) = project(&[
            ("store.ts", "export function save() {}"),
            (&format!("entry.{extension}"), source),
        ]);
        for caller in [
            "objectParameter",
            "arrayParameter",
            "aliasParameter",
            "nestedParameter",
            "defaultParameter",
            "restParameter",
            "arrayRestParameter",
            "objectRestParameter",
            "localObject",
            "localArray",
            "beforeBinding",
            "functionScoped",
        ] {
            let result = engine
                .related(caller, Direction::Callees, 50, 65536)
                .unwrap();
            let calls: Vec<_> = result
                .relations
                .iter()
                .filter(|relation| relation.target == "save")
                .collect();
            assert_eq!(calls.len(), 1, "{extension} {caller}: {result:?}");
            assert!(
                calls[0].candidate.is_none(),
                "{extension} {caller}: {result:?}"
            );
            assert_eq!(
                calls[0].resolution, "unresolved",
                "{extension} {caller}: {result:?}"
            );
        }
        for caller in ["propertyKey", "defaultExpression"] {
            let result = engine
                .related(caller, Direction::Callees, 50, 65536)
                .unwrap();
            assert_eq!(
                result.relations.len(),
                1,
                "{extension} {caller}: {result:?}"
            );
            assert_eq!(
                result.relations[0].resolution, "import_match",
                "{extension} {caller}: {result:?}"
            );
        }
        for caller in ["catchScope", "catchPattern", "blockScope"] {
            let result = engine
                .related(caller, Direction::Callees, 50, 65536)
                .unwrap();
            assert_eq!(
                result.relations.len(),
                2,
                "{extension} {caller}: {result:?}"
            );
            assert!(
                result.relations[0].candidate.is_none(),
                "{extension} {caller}: {result:?}"
            );
            assert_eq!(
                result.relations[1].resolution, "import_match",
                "{extension} {caller}: {result:?}"
            );
            assert_eq!(
                result.relations[1].candidate.as_ref().unwrap().path,
                "store.ts"
            );
        }
    }
}

#[test]
fn local_receivers_do_not_reuse_shadowed_namespace_or_class_imports() {
    for extension in ["ts", "js"] {
        let store_parameter = if extension == "ts" {
            "store: any"
        } else {
            "store"
        };
        let class_parameter = if extension == "ts" {
            "Store: any"
        } else {
            "Store"
        };
        let typed = if extension == "ts" {
            "function typed(store: Store) { store.save(); }"
        } else {
            ""
        };
        let source = format!(
            r#"import * as store from './store';
import {{ Store }} from './store';
function namespaceParameter({store_parameter}) {{ store.save(); }}
function classParameter({class_parameter}) {{ Store.save(); }}
function constructorParameter({class_parameter}) {{ new Store().save(); }}
function indirectConstructor({class_parameter}) {{ const value = new Store(); value.save(); }}
function localNamespace() {{ const store = input; store.save(); }}
function destructuredNamespace({{store}}) {{ store.save(); }}
function catchNamespace() {{
  try {{ throw null; }} catch (store) {{ store.save(); }}
  store.save();
}}
function importedNamespace() {{ store.save(); }}
function constructedReceiver() {{ const store = new Store(); store.save(); }}
{typed}
"#
        );
        let (_temporary, engine) = project(&[
            (
                "store.ts",
                "export function save() {} export class Store { save() {} }",
            ),
            (&format!("entry.{extension}"), &source),
        ]);
        for caller in [
            "namespaceParameter",
            "classParameter",
            "constructorParameter",
            "indirectConstructor",
            "localNamespace",
            "destructuredNamespace",
        ] {
            let result = engine
                .related(caller, Direction::Callees, 50, 65536)
                .unwrap();
            assert!(
                !result.relations.is_empty(),
                "{extension} {caller}: {result:?}"
            );
            assert!(
                result
                    .relations
                    .iter()
                    .all(|relation| relation.candidate.is_none()
                        && relation.resolution == "unresolved"),
                "{extension} {caller}: {result:?}"
            );
        }
        let restored = engine
            .related("catchNamespace", Direction::Callees, 50, 65536)
            .unwrap();
        assert_eq!(restored.relations.len(), 2, "{extension}: {restored:?}");
        assert!(
            restored.relations[0].candidate.is_none(),
            "{extension}: {restored:?}"
        );
        assert_eq!(
            restored.relations[1].resolution, "import_match",
            "{extension}: {restored:?}"
        );
        let imported = engine
            .related("importedNamespace", Direction::Callees, 50, 65536)
            .unwrap();
        assert_eq!(imported.relations[0].resolution, "import_match");
        let constructed = engine
            .related("constructedReceiver", Direction::Callees, 50, 65536)
            .unwrap();
        assert!(
            constructed
                .relations
                .iter()
                .any(|relation| relation.target == "save"
                    && relation.resolution == "typed_receiver_match"),
            "{extension}: {constructed:?}"
        );
        if extension == "ts" {
            let typed = engine
                .related("typed", Direction::Callees, 50, 65536)
                .unwrap();
            assert_eq!(
                typed.relations[0].resolution, "typed_receiver_match",
                "{typed:?}"
            );
        }
    }
}
