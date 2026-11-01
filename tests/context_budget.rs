use prometeu_search::{Engine, GraphMode, GraphOptions};
use std::{collections::HashSet, fs};
use tempfile::TempDir;

fn project(sources: &[(&str, &str)]) -> (TempDir, Engine) {
    let temp = TempDir::new().unwrap();
    let root = temp.path().join("project");
    fs::create_dir(&root).unwrap();
    for (name, source) in sources {
        fs::write(root.join(name), source).unwrap();
    }
    let mut engine = Engine::open(&root, Some(&temp.path().join("index.sqlite"))).unwrap();
    engine.sync().unwrap();
    (temp, engine)
}

fn options(limit: usize) -> GraphOptions {
    GraphOptions {
        limit,
        max_bytes: 65_536,
        depth: 1,
        ..GraphOptions::default()
    }
}

#[test]
fn lexical_exploration_keeps_ranked_seeds_and_connected_context_within_the_limit() {
    let sources: Vec<_> = (0..8).flat_map(|index| [
        (format!("entry{index}.ts"), format!("import {{ helper{index} }} from './leaf{index}'; export function feature_{index}() {{ helper{index}(); }}")),
        (format!("leaf{index}.ts"), format!("export function helper{index}() {{}}")),
    ]).collect();
    let (_temp, engine) = project(
        &sources
            .iter()
            .map(|(name, source)| (name.as_str(), source.as_str()))
            .collect::<Vec<_>>(),
    );
    let ranked = engine.search("feature", 50, 65_536).unwrap();
    let result = engine.explore("feature", &options(6)).unwrap();
    assert!(result.truncated);
    assert_eq!(result.roots.len(), 3);
    assert_eq!(
        result.roots.iter().map(|root| root.id).collect::<Vec<_>>(),
        ranked
            .results
            .iter()
            .take(3)
            .map(|hit| hit.id)
            .collect::<Vec<_>>()
    );
    let symbols: HashSet<_> = result
        .files
        .iter()
        .flat_map(|file| &file.symbols)
        .map(|symbol| symbol.id)
        .collect();
    assert_eq!(symbols.len(), 6);
    assert!(result.roots.iter().all(|root| symbols.contains(&root.id)));
    assert_eq!(result.edges.len(), 3);
    assert!(
        result
            .edges
            .iter()
            .all(|edge| edge.resolution == "import_match"
                && symbols.contains(&edge.from)
                && edge.to.is_some_and(|to| symbols.contains(&to)))
    );
    assert!(serde_json::to_vec(&result).unwrap().len() <= 65_536);
    let tiny = engine
        .explore(
            "feature",
            &GraphOptions {
                max_bytes: 1024,
                ..options(6)
            },
        )
        .unwrap();
    assert!(tiny.truncated);
    assert!(serde_json::to_vec(&tiny).unwrap().len() <= 1024);
    let returned: HashSet<_> = tiny
        .files
        .iter()
        .flat_map(|file| &file.symbols)
        .map(|symbol| symbol.id)
        .collect();
    assert!(tiny.roots.iter().all(|root| returned.contains(&root.id)));
}

#[test]
fn unused_context_slots_backfill_ranked_matches_and_disconnected_queries_keep_the_budget() {
    let functions: String = (0..8)
        .map(|index| format!("export function feature_{index}() {{ helper(); }}\n"))
        .collect();
    let source = format!("import {{ helper }} from './leaf';\n{functions}");
    let (_temp, engine) = project(&[
        ("entry.ts", &source),
        ("leaf.ts", "export function helper() {}"),
    ]);
    let ranked = engine.search("feature", 50, 65_536).unwrap();
    let result = engine.explore("feature", &options(6)).unwrap();
    assert_eq!(
        result.roots.len(),
        5,
        "one context symbol leaves five match slots"
    );
    assert_eq!(
        result
            .files
            .iter()
            .map(|file| file.symbols.len())
            .sum::<usize>(),
        6
    );
    assert_eq!(
        result.roots.iter().map(|root| root.id).collect::<Vec<_>>(),
        ranked
            .results
            .iter()
            .take(5)
            .map(|hit| hit.id)
            .collect::<Vec<_>>()
    );
    assert!(result.truncated);

    let declarations: String = (0..6)
        .map(|index| format!("function feature_{index}() {{}}\n"))
        .collect();
    let (_temp, engine) = project(&[("entry.ts", &declarations)]);
    let result = engine.explore("feature", &options(6)).unwrap();
    assert_eq!(result.roots.len(), 6);
    assert_eq!(result.files[0].symbols.len(), 6);
    assert!(!result.truncated);
    assert!(result.edges.is_empty());
}

#[test]
fn exact_names_files_lines_and_analysis_keep_their_seed_selection() {
    let sources: Vec<_> = (0..6)
        .map(|index| (format!("part{index}.ts"), "function run() {}".to_owned()))
        .collect();
    let (_temp, engine) = project(
        &sources
            .iter()
            .map(|(name, source)| (name.as_str(), source.as_str()))
            .collect::<Vec<_>>(),
    );
    assert_eq!(engine.explore("run", &options(6)).unwrap().roots.len(), 6);
    assert_eq!(
        engine
            .analyze("run", GraphMode::Impact, &options(6))
            .unwrap()
            .roots
            .len(),
        6
    );
    let selected = engine
        .explore(
            "run",
            &GraphOptions {
                path: Some("part3.ts".into()),
                line: Some(1),
                ..options(6)
            },
        )
        .unwrap();
    assert_eq!(selected.roots.len(), 1);
    assert_eq!(selected.roots[0].path, "part3.ts");
    let declarations: String = (0..6)
        .map(|index| format!("export function feature_{index}() {{}}\n"))
        .collect();
    let (_temp, engine) = project(&[("entry.ts", &declarations)]);
    assert_eq!(
        engine.explore("entry.ts", &options(6)).unwrap().roots.len(),
        6
    );
}
