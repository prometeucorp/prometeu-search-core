use prometeu_search::{Engine, parser};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::Path;

fn sources() -> BTreeMap<String, String> {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut result = BTreeMap::<String, String>::new();
    for (file, language) in [
        ("upstream/cases.json", None),
        ("upstream/additional/cases.json", None),
        ("upstream/supplemental/cases.json", None),
        ("upstream/pascal/cases.json", Some("pascal")),
        ("upstream/components/cases.json", None),
        ("upstream/razor/cases.json", Some("razor")),
        ("upstream/cfml/cases.json", None),
        ("formats/cases.json", None),
    ] {
        let corpus: Value =
            serde_json::from_slice(&std::fs::read(fixtures.join(file)).unwrap()).unwrap();
        for case in corpus["cases"].as_array().unwrap() {
            let Some(language) = language.or_else(|| case["language"].as_str()) else {
                continue;
            };
            if case["clean"] == false {
                continue;
            }
            let source = if file == "upstream/cases.json" {
                std::fs::read_to_string(
                    fixtures
                        .join("upstream")
                        .join(case["input"].as_str().unwrap()),
                )
                .unwrap()
            } else {
                case["source"].as_str().unwrap_or_default().to_owned()
            };
            if source.trim().is_empty() {
                continue;
            }
            if !parser::LANGUAGES.contains(&language) {
                continue;
            }
            if case["expected"]["errors"]
                .as_array()
                .is_some_and(|errors| !errors.is_empty())
            {
                continue;
            }
            if result
                .get(language)
                .is_none_or(|old| source.len() < old.len())
            {
                result.insert(language.into(), source);
            }
        }
    }
    result
}

fn normalize(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.remove("updatedAt");
            map.remove("updated_at");
            for value in map.values_mut() {
                normalize(value);
            }
        }
        Value::Array(rows) => {
            for row in rows {
                normalize(row);
            }
        }
        _ => {}
    }
}

fn snapshot(database: &Path) -> Value {
    let db = rusqlite::Connection::open(database).unwrap();
    let files = db
        .prepare("SELECT path, language, analysis FROM files ORDER BY path")
        .unwrap()
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .unwrap()
        .map(|row| {
            let (path, language, analysis) = row.unwrap();
            let mut analysis: Value = serde_json::from_str(&analysis).unwrap();
            normalize(&mut analysis);
            json!({"path":path,"language":language,"analysis":analysis})
        })
        .collect::<Vec<_>>();
    let symbols=db.prepare("SELECT path,name,ordinal,kind,start_line,end_line,code FROM symbols ORDER BY path,ordinal").unwrap().query_map([],|row|Ok(json!([row.get::<_,String>(0)?,row.get::<_,String>(1)?,row.get::<_,i64>(2)?,row.get::<_,String>(3)?,row.get::<_,i64>(4)?,row.get::<_,i64>(5)?,row.get::<_,String>(6)?]))).unwrap().map(Result::unwrap).collect::<Vec<_>>();
    json!({"files":files,"symbols":symbols})
}

fn compare_cold(root: &Path, database: &Path, cache: &Path, label: &str) {
    let cold = cache.join(format!("cold-{label}.sqlite"));
    let mut engine = Engine::open(root, Some(&cold)).unwrap();
    engine.sync().unwrap();
    drop(engine);
    assert_eq!(
        snapshot(database),
        snapshot(&cold),
        "incremental and cold snapshots differ at {label}"
    );
}

#[test]
fn all_41_languages_match_cold_index_after_create_modify_rename_delete_and_configuration_changes() {
    let sources = sources();
    assert_eq!(
        sources
            .keys()
            .map(String::as_str)
            .collect::<std::collections::BTreeSet<_>>(),
        parser::LANGUAGES.iter().copied().collect()
    );
    for (language, source) in sources {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let database = temp.path().join("index.sqlite");
        let config = root.join("prometeu-search.json");
        let settings = json!({"extensions":{"probe":language}});
        std::fs::write(&config, settings.to_string()).unwrap();
        let mut engine = Engine::open(&root, Some(&database)).unwrap();
        engine.sync().unwrap();
        assert_eq!(engine.status().unwrap().files, 0, "{language} empty");
        std::fs::write(root.join("original.probe"), &source).unwrap();
        engine.sync_paths(&["original.probe".into()]).unwrap();
        assert_eq!(engine.status().unwrap().files, 1, "{language} created");
        compare_cold(&root, &database, temp.path(), "create");
        std::fs::write(root.join("original.probe"), format!("\n{source}\n")).unwrap();
        engine.sync_paths(&["original.probe".into()]).unwrap();
        compare_cold(&root, &database, temp.path(), "modify");
        std::fs::rename(root.join("original.probe"), root.join("renamed.probe")).unwrap();
        engine
            .sync_paths(&["original.probe".into(), "renamed.probe".into()])
            .unwrap();
        compare_cold(&root, &database, temp.path(), "rename");
        let stable = snapshot(&database);
        drop(engine);
        let mut engine = Engine::open(&root, Some(&database)).unwrap();
        assert_eq!(snapshot(&database), stable, "{language} reopened");
        std::fs::write(
            &config,
            json!({"extensions":{"probe":language},"exclude":["renamed.probe"]}).to_string(),
        )
        .unwrap();
        engine.sync_paths(&["renamed.probe".into()]).unwrap();
        assert_eq!(engine.status().unwrap().files, 0, "{language} excluded");
        compare_cold(&root, &database, temp.path(), "exclude");
        std::fs::write(&config, settings.to_string()).unwrap();
        engine.sync_paths(&["renamed.probe".into()]).unwrap();
        compare_cold(&root, &database, temp.path(), "restore");
        std::fs::remove_file(root.join("renamed.probe")).unwrap();
        engine.sync_paths(&["renamed.probe".into()]).unwrap();
        assert_eq!(engine.status().unwrap().files, 0, "{language} deleted");
        compare_cold(&root, &database, temp.path(), "delete");
        assert!(
            snapshot(&database)["symbols"]
                .as_array()
                .unwrap()
                .is_empty(),
            "{language} stale symbols"
        );
    }
}
