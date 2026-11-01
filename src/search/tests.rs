use super::*;
use serde_json::{Value as Json, json};
fn fixture() -> Json {
    serde_json::from_str(include_str!(
        "../../tests/fixtures/upstream/search/cases.json"
    ))
    .unwrap()
}
#[test]
fn query_helpers_match_independent_reference() {
    let fixture = fixture();
    for case in fixture["helpers"].as_array().unwrap() {
        let text = case["text"].as_str().unwrap();
        let actual = json!({"text":text,"parsed":parse_query(text),"terms":extract_search_terms(text,true),"baseTerms":extract_search_terms(text,false),"cjk":has_cjk(text),"switches":split_at_cjk_switches(text),"floor":meets_query_word_floor(text),"name":is_query_name_token(text),"blank":blank_non_ascii_words(text),"distinctive":is_distinctive_identifier(text),"stem":stem_variants(text),"segments":split_identifier_segments(text),"normalized":normalize_prose_word(text),"prose":extract_prose_candidates(text),"segmentWords":extract_segment_search_words(text),"variants":segment_lookup_variants(text)});
        assert_eq!(&actual, case, "helper {text:?}");
    }
    for case in fixture["scores"].as_array().unwrap() {
        assert_eq!(
            name_match_bonus(
                case["name"].as_str().unwrap(),
                case["query"].as_str().unwrap()
            ),
            case["bonus"].as_f64().unwrap(),
            "bonus {case}"
        );
    }
    for case in fixture["pathScores"].as_array().unwrap() {
        assert_eq!(
            score_path_relevance(
                case["path"].as_str().unwrap(),
                case["query"].as_str().unwrap(),
                &HashSet::from(["superbizagent".into()]),
                case["deprioritized"].as_bool().unwrap()
            ),
            case["expected"].as_f64().unwrap(),
            "path {case}"
        );
    }
    for case in fixture["distances"].as_array().unwrap() {
        assert_eq!(
            bounded_edit_distance(
                case["a"].as_str().unwrap(),
                case["b"].as_str().unwrap(),
                case["bound"].as_u64().unwrap() as usize
            ),
            case["expected"].as_u64().unwrap() as usize,
            "distance {case}"
        );
    }
}
#[test]
fn sqlite_search_order_and_scores_match_independent_reference() {
    let fixture = fixture();
    let project = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let engine =
        crate::Engine::open(project.path(), Some(&cache.path().join("search.sqlite"))).unwrap();
    let db = &engine.connection;
    db.execute(
        "INSERT INTO metadata VALUES('project_manifests',?)",
        [json!({"package.json":"{\"name\":\"@scope/SuperBizAgent\"}"}).to_string()],
    )
    .unwrap();
    for node in fixture["nodes"].as_array().unwrap() {
        let path = node["filePath"].as_str().unwrap();
        db.execute("INSERT OR IGNORE INTO files(path,hash,language,syntax_errors,analysis,deprioritized) VALUES(?,'fixture','typescript',0,'{}',?)",rusqlite::params![path,path.starts_with("vendor/")||path.starts_with("examples/")]).unwrap();
        db.execute("INSERT INTO symbols(path,name,ordinal,terms,kind,start_line,end_line,code) VALUES(?,?,0,'',?,1,2,'fixture source')",rusqlite::params![path,node["name"].as_str(),node["kind"].as_str()]).unwrap();
        let id = db.last_insert_rowid();
        db.execute("INSERT INTO canonical_search(symbol_id,id,name,qualified_name,docstring,signature,kind,language,path) VALUES(?,?,?,?,?,?,?,?,?)",rusqlite::params![id,node["id"].as_str(),node["name"].as_str(),node["qualifiedName"].as_str(),node["docstring"].as_str(),node["signature"].as_str(),node["kind"].as_str(),node["language"].as_str(),path]).unwrap();
    }
    for case in fixture["ranked"].as_array().unwrap() {
        let query = case["query"].as_str().unwrap();
        let limit = case["limit"].as_u64().unwrap() as usize;
        let actual = engine
            .search_with_options(
                query,
                &SearchOptions {
                    limit,
                    offset: case["offset"].as_u64().unwrap_or(0) as usize,
                    max_bytes: None,
                },
            )
            .unwrap();
        let reference = case["expected"].as_array().unwrap();
        let expected = reference;
        let ids: Vec<String> = actual
            .results
            .iter()
            .map(|hit| {
                db.query_row(
                    "SELECT id FROM canonical_search WHERE symbol_id=?",
                    [hit.id],
                    |r| r.get(0),
                )
                .unwrap()
            })
            .collect();
        assert_eq!(
            ids,
            expected
                .iter()
                .map(|n| n["id"].as_str().unwrap())
                .collect::<Vec<_>>(),
            "ranking query={query:?} limit={limit}"
        );
        for (hit, reference) in actual.results.iter().zip(expected) {
            assert!(
                (hit.score - reference["score"].as_f64().unwrap()).abs() < 1e-10,
                "score query={query:?} limit={limit} got={} expected={}",
                hit.score,
                reference["score"]
            );
        }
    }
}
#[test]
fn query_paths_match_independent_reference() {
    let fixture = fixture();
    for case in fixture["pathCases"].as_array().unwrap() {
        let query = case["query"].as_str().unwrap();
        let indexed: Vec<String> = serde_json::from_value(case["indexed"].clone()).unwrap();
        let symbol_map: std::collections::HashMap<String, Vec<String>> =
            serde_json::from_value(case["symbolMap"].clone()).unwrap();
        let symbols = |name: &str| symbol_map.get(name).cloned().unwrap_or_default();
        let disk = |name: &str| name == "scripts/deploy";
        let options = match case["mode"].as_str().unwrap() {
            "symbols" => QueryPathOptions {
                symbol_files: Some(&symbols),
                ..Default::default()
            },
            "disk" => QueryPathOptions {
                exists_on_disk: Some(&disk),
                ..Default::default()
            },
            "budget" => QueryPathOptions {
                max_pins: Some(1),
                max_matches_per_span: Some(1),
                ..Default::default()
            },
            _ => QueryPathOptions::default(),
        };
        assert_eq!(
            query_might_contain_paths(query),
            case["might"].as_bool().unwrap(),
            "path gate {query:?}"
        );
        assert_eq!(
            serde_json::to_value(extract_query_paths(query, &indexed, options)).unwrap(),
            case["expected"],
            "path query={query:?} mode={}",
            case["mode"]
        );
    }
}
