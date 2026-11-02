use prometeu_search::Engine;
use serde_json::Value;
use std::fs;

#[test]
fn search_pages_apply_offsets_after_ranking_exact_and_typo_matches() {
    use prometeu_search::search::SearchOptions;
    let temporary = tempfile::tempdir().unwrap();
    let mut source =
        "export function save() {}\nexport function safe() {}\nexport function saver() {}\n"
            .to_owned();
    for index in 0..60 {
        source.push_str(&format!("export function getter{index:02}() {{}}\n"));
    }
    fs::write(temporary.path().join("actions.ts"), source).unwrap();
    let mut engine = Engine::open(
        temporary.path(),
        Some(&temporary.path().join("search.sqlite")),
    )
    .unwrap();
    engine.sync().unwrap();
    let page = |query: &str, offset, limit| {
        engine
            .search_with_options(
                query,
                &SearchOptions {
                    offset,
                    limit,
                    max_bytes: None,
                },
            )
            .unwrap()
            .results
    };
    let all = page("sawe", 0, 10);
    assert_eq!(all.len(), 2);
    assert_eq!(page("sawe", 0, 1)[0].id, all[0].id);
    assert_eq!(page("sawe", 1, 1)[0].id, all[1].id);
    assert!(page("sawe", 2, 10).is_empty());
    assert!(page("sawe", 10, 10).is_empty());
    assert_eq!(page("sawe path:actions", 1, 1)[0].id, all[1].id);
    assert_eq!(page("getterxy", 50, 1).len(), 1);
    assert!(page("getterxy", 60, 1).is_empty());
    assert_eq!(
        page("save", 0, 10)
            .iter()
            .map(|hit| hit.name.as_str())
            .collect::<Vec<_>>(),
        ["save", "saver"]
    );
    assert_eq!(page("save", 0, 1)[0].name, "save");
    assert_eq!(page("save", 1, 1)[0].name, "saver");
    assert!(page("save", 2, 1).is_empty());
}

#[test]
fn library_search_pages_candidates_without_transport_ceiling() {
    use prometeu_search::search::SearchOptions;
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("project");
    fs::create_dir(&root).unwrap();
    let source = (0..130)
        .map(|i| {
            format!(
                "export function entry{i:03}() {{ return \"{}\"; }}\n",
                "value ".repeat(160)
            )
        })
        .collect::<String>();
    fs::write(root.join("entries.ts"), source).unwrap();
    let database = temporary.path().join("search.sqlite");
    let mut engine = Engine::open(&root, Some(&database)).unwrap();
    engine.sync().unwrap();
    let all = engine
        .search_with_options(
            "kind:function",
            &SearchOptions {
                limit: 130,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(all.results.len(), 130);
    assert!(serde_json::to_vec(&all).unwrap().len() > 65_536);
    assert_eq!(
        engine
            .search_with_options("kind:function", &SearchOptions::default())
            .unwrap()
            .results
            .len(),
        100
    );
    let page = engine
        .search_with_options(
            "kind:function",
            &SearchOptions {
                limit: 20,
                offset: 110,
                max_bytes: None,
            },
        )
        .unwrap();
    assert_eq!(
        page.results.iter().map(|hit| &hit.name).collect::<Vec<_>>(),
        all.results[110..]
            .iter()
            .map(|hit| &hit.name)
            .collect::<Vec<_>>()
    );
    let bounded = engine
        .search_with_options(
            "kind:function",
            &SearchOptions {
                limit: 130,
                max_bytes: Some(1024),
                ..Default::default()
            },
        )
        .unwrap();
    assert!(serde_json::to_vec(&bounded).unwrap().len() <= 1024);
    assert!(bounded.truncated);
    drop(engine);
    let reopened = Engine::open(&root, Some(&database)).unwrap();
    assert_eq!(
        reopened
            .search_with_options(
                "kind:function",
                &SearchOptions {
                    limit: 20,
                    offset: 110,
                    max_bytes: None
                }
            )
            .unwrap()
            .results
            .len(),
        20
    );
}

#[test]
fn indexed_search_matches_reference_and_reads_committed_source() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/upstream/search/projects.json")).unwrap();
    for project in fixture["cases"].as_array().unwrap() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("project");
        fs::create_dir(&root).unwrap();
        let database = temporary.path().join("search.sqlite");
        for (file, source) in project["files"].as_object().unwrap() {
            let path = root.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, source.as_str().unwrap()).unwrap();
        }
        let mut engine = Engine::open(&root, Some(&database)).unwrap();
        engine.sync().unwrap();
        let check = |engine: &Engine| {
            for case in project["queries"].as_array().unwrap() {
                let query = case["query"].as_str().unwrap();
                let limit = case["limit"].as_u64().unwrap() as usize;
                let actual = engine.search(query, limit, 65536).unwrap();
                let expected = case["expected"].as_array().unwrap();
                let identities: Vec<_> = actual
                    .results
                    .iter()
                    .map(|h| (&h.name, &h.kind, &h.path, h.start_line))
                    .collect();
                let reference: Vec<_> = expected
                    .iter()
                    .map(|r| {
                        (
                            r["name"].as_str().unwrap(),
                            r["kind"].as_str().unwrap(),
                            r["path"].as_str().unwrap(),
                            r["startLine"].as_u64().unwrap() as usize,
                        )
                    })
                    .collect();
                assert_eq!(
                    identities
                        .iter()
                        .map(|(n, k, p, l)| (n.as_str(), k.as_str(), p.as_str(), *l))
                        .collect::<Vec<_>>(),
                    reference,
                    "project={} query={query:?} limit={limit}",
                    project["name"]
                );
                for (hit, reference) in actual.results.iter().zip(expected) {
                    assert!(
                        (hit.score - reference["score"].as_f64().unwrap()).abs() < 1e-10,
                        "project={} query={query:?} score got {} expected {}",
                        project["name"],
                        hit.score,
                        reference["score"]
                    );
                    assert!(
                        project["files"][&hit.path]
                            .as_str()
                            .unwrap()
                            .contains(&hit.snippet),
                        "source slice {}",
                        hit.path
                    );
                    assert!(!hit.snippet.contains("changed_on_disk"));
                }
            }
        };
        check(&engine);
        for file in project["files"].as_object().unwrap().keys() {
            fs::write(root.join(file), "changed_on_disk\n").unwrap();
        }
        drop(engine);
        let engine = Engine::open(&root, Some(&database)).unwrap();
        check(&engine);
    }
}

#[test]
fn canonical_search_updates_and_deletes_with_source_generation() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("project");
    fs::create_dir(&root).unwrap();
    let database = temporary.path().join("search.sqlite");
    fs::write(
        root.join("item.ts"),
        "/** old_documentation */\nexport function before() {}\n",
    )
    .unwrap();
    let mut first = Engine::open(&root, Some(&database)).unwrap();
    first.sync().unwrap();
    let second = Engine::open(&root, Some(&database)).unwrap();
    assert_eq!(
        second
            .search("old_documentation", 10, 8192)
            .unwrap()
            .results[0]
            .name,
        "before"
    );
    let generation = second.status().unwrap().generation;
    fs::write(
        root.join("item.ts"),
        "/** new_documentation */\nexport function after() {}\n",
    )
    .unwrap();
    first.sync().unwrap();
    assert!(
        second
            .search("old_documentation", 10, 8192)
            .unwrap()
            .results
            .is_empty()
    );
    let changed = second.search("new_documentation", 10, 8192).unwrap();
    assert!(changed.generation > generation);
    assert_eq!(changed.results[0].name, "after");
    fs::remove_file(root.join("item.ts")).unwrap();
    first.sync().unwrap();
    assert!(
        second
            .search("new_documentation", 10, 8192)
            .unwrap()
            .results
            .is_empty()
    );
    assert!(
        second
            .search("kind:file", 10, 8192)
            .unwrap()
            .results
            .is_empty()
    );
}

#[test]
fn reopening_installs_update_triggers_for_in_place_route_finalization() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("project");
    fs::create_dir(&root).unwrap();
    let database = temporary.path().join("search.sqlite");
    fs::write(
        root.join("item.ts"),
        "export function original_marker() {}\n",
    )
    .unwrap();
    let mut engine = Engine::open(&root, Some(&database)).unwrap();
    engine.sync().unwrap();
    drop(engine);
    let connection = rusqlite::Connection::open(&database).unwrap();
    connection
        .execute_batch("DROP TRIGGER symbols_update; DROP TRIGGER canonical_update;")
        .unwrap();
    let engine = Engine::open(&root, Some(&database)).unwrap();
    connection.execute_batch("BEGIN;
        UPDATE symbols SET name='renamed_marker',terms='renamed_marker',code='renamed_marker' WHERE kind='function';
        UPDATE canonical_search SET name='renamed_marker',qualified_name='renamed_marker',signature='renamed_marker' WHERE kind='function';
        COMMIT;").unwrap();
    for table in ["symbols_fts", "canonical_fts"] {
        let count = |query: &str| {
            connection.query_row(&format!("SELECT count(*) FROM {table} WHERE {table} MATCH ?1 AND rowid IN (SELECT id FROM symbols WHERE kind='function')"), [query], |row| row.get::<_,i64>(0)).unwrap()
        };
        assert_eq!(count("original_marker"), 0, "{table}");
        assert_eq!(count("renamed_marker"), 1, "{table}");
    }
    assert_eq!(
        engine.search("renamed_marker", 10, 8192).unwrap().results[0].name,
        "renamed_marker"
    );
}
