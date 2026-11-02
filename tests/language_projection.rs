use prometeu_search::{Engine, parser};
use serde_json::Value;
use std::{fs, path::Path};

#[test]
fn pascal_and_vbnet_preserve_source_and_canonical_metadata_in_the_index() {
    let temporary = tempfile::TempDir::new().unwrap();
    let root = temporary.path().join("project");
    let database = temporary.path().join("index.sqlite");
    let pascal: Value =
        serde_json::from_str(include_str!("fixtures/upstream/pascal/cases.json")).unwrap();
    let supplemental: Value =
        serde_json::from_str(include_str!("fixtures/upstream/supplemental/cases.json")).unwrap();
    let mut cases = Vec::new();
    for (language, case) in pascal["cases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| ("pascal", c))
        .chain(
            supplemental["cases"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|c| c["language"] == "vbnet")
                .map(|c| ("vbnet", c)),
        )
    {
        let source = case["source"].as_str().unwrap();
        let extension = if language == "vbnet" {
            "vb"
        } else {
            match Path::new(case["path"].as_str().unwrap())
                .extension()
                .and_then(|s| s.to_str())
            {
                Some("dfm") => "dfm",
                Some("fmx") => "fmx",
                _ => "pas",
            }
        };
        let path = format!("{}/source.{extension}", case["id"].as_str().unwrap());
        let parsed =
            parser::parse(Path::new(&path), source).unwrap_or_else(|e| panic!("{path}: {e:#}"));
        for symbol in &parsed.symbols {
            assert!(
                source.get(symbol.start_byte..symbol.end_byte).is_some(),
                "{path}: {} source bounds",
                symbol.name
            );
        }
        fs::create_dir_all(root.join(&path).parent().unwrap()).unwrap();
        fs::write(root.join(&path), source).unwrap();
        cases.push((
            path,
            language,
            source.to_owned(),
            parsed.extraction.unwrap(),
        ));
    }
    let mut engine = Engine::open(&root, Some(&database)).unwrap();
    assert_eq!(engine.sync().unwrap().indexed, cases.len());
    assert_eq!(engine.sync().unwrap().unchanged, cases.len());
    drop(engine);
    let engine = Engine::open(&root, Some(&database)).unwrap();
    let connection = rusqlite::Connection::open(database).unwrap();
    let mut statement = connection
        .prepare("SELECT id,ordinal FROM symbols WHERE path=?1 ORDER BY ordinal")
        .unwrap();
    for (path, language, source, expected) in cases {
        let file = engine.file(&path, 1, None, 65536).unwrap().file.unwrap();
        assert_eq!(file.source, source, "{path}");
        assert_eq!(file.language, language, "{path}");
        for row in statement
            .query_map([&path], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)? as usize))
            })
            .unwrap()
        {
            let (id, ordinal) = row.unwrap();
            let node = engine.node(id, 65536).unwrap().node.unwrap();
            let mut actual = node.metadata.unwrap();
            actual.updated_at = expected.nodes[ordinal].updated_at;
            assert_eq!(actual, expected.nodes[ordinal], "{path}: {}", node.name);
        }
    }
}
