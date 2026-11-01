use super::*;

#[test]
fn constructor_owners_columns_and_shadowing_survive_older_cached_projections() -> Result<()> {
    let temporary = tempfile::tempdir()?;
    let root = temporary.path().join("project");
    std::fs::create_dir(&root)?;
    let database = temporary.path().join("index.sqlite");
    std::fs::write(root.join("store.ts"), "export class Store {}\n")?;
    let source = "import { Store } from './store';\nimport * as ns from './store';\nclass App {\n  item = new Store();\n  build = () => new ns.Store();\n  run() { const mark = '😀'; Store(); new Store(); new Store(); }\n}\nfunction hidden(Store: any) { new Store(); }\n";
    std::fs::write(root.join("app.ts"), source)?;
    let mut engine = crate::Engine::open(&root, Some(&database))?;
    engine.sync()?;
    let graph = Graph::load(&engine.connection, 1)?;
    let constructors = &graph.files["app.ts"].analysis.references;
    let constructors: Vec<_> = constructors
        .iter()
        .filter(|reference| reference.kind == "instantiates")
        .collect();
    assert_eq!(constructors.len(), 5);
    let expected: Vec<_> = graph.canonical_edges.iter().flatten().cloned().collect();
    let extraction = graph.files["app.ts"].analysis.extraction.as_ref().unwrap();
    for raw in extraction
        .unresolved_references
        .iter()
        .filter(|raw| raw.reference_kind == "instantiates")
    {
        let reference = constructors
            .iter()
            .find(|reference| {
                reference.line == raw.line as usize && reference.column == Some(raw.column as usize)
            })
            .unwrap();
        assert_eq!(
            extraction.nodes[reference.caller.unwrap()].id,
            raw.from_node_id
        );
    }
    assert!(
        constructors
            .iter()
            .any(|reference| reference.qualifier.as_deref() == Some("ns"))
    );
    assert!(
        constructors
            .iter()
            .any(|reference| reference.line == 8 && reference.shadowed)
    );
    assert!(
        graph.files["app.ts"]
            .analysis
            .references
            .iter()
            .any(|reference| reference.line == 6
                && reference.kind == "calls"
                && reference.target == "Store")
    );
    let old: String = engine.connection.query_row(
        "SELECT analysis FROM files WHERE path='app.ts'",
        [],
        |row| row.get(0),
    )?;
    let mut old: serde_json::Value = serde_json::from_str(&old)?;
    let file = old["symbols"]
        .as_array()
        .unwrap()
        .iter()
        .position(|symbol| symbol["kind"] == "file")
        .unwrap();
    for reference in old["references"].as_array_mut().unwrap() {
        if reference["kind"] == "instantiates" {
            reference["kind"] = "calls".into();
            reference.as_object_mut().unwrap().remove("column");
            reference["caller"] = file.into();
        }
    }
    let old = serde_json::to_string(&old)?;
    engine
        .connection
        .execute("UPDATE files SET analysis=?1 WHERE path='app.ts'", [&old])?;
    drop(engine);
    std::fs::write(root.join("app.ts"), "class Changed {}\n")?;
    std::fs::write(root.join("store.ts"), "export class Changed {}\n")?;
    let engine = crate::Engine::open(&root, Some(&database))?;
    let reopened = Graph::load(&engine.connection, 1)?;
    let actual: Vec<_> = reopened.canonical_edges.iter().flatten().cloned().collect();
    assert_eq!(
        serde_json::to_value(actual)?,
        serde_json::to_value(expected)?
    );
    let retained: String = engine.connection.query_row(
        "SELECT analysis FROM files WHERE path='app.ts'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(retained, old);
    Ok(())
}
