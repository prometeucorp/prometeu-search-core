use prometeu_search::{
    Engine,
    context::{BuildContextOptions, TaskInput},
};
use std::{collections::HashSet, fs};

#[test]
fn node_limits_remove_roots_without_a_retained_node() {
    let temporary = tempfile::tempdir().unwrap();
    fs::write(
        temporary.path().join("session.ts"),
        "export function sessionAlpha() {}\nexport function sessionBeta() {}\nexport function sessionGamma() {}\n",
    )
    .unwrap();
    let mut engine = Engine::open(
        temporary.path(),
        Some(&temporary.path().join("index.sqlite")),
    )
    .unwrap();
    engine.sync().unwrap();
    let query = TaskInput::Query("session".into());
    let options = BuildContextOptions {
        search_limit: 3,
        max_nodes: 3,
        ..Default::default()
    };
    let complete = engine.context(&query, &options, 65536).unwrap();
    assert_eq!(complete.context.subgraph.roots.len(), 3);
    let limited = engine
        .context(
            &query,
            &BuildContextOptions {
                max_nodes: 1,
                ..options
            },
            65536,
        )
        .unwrap();
    assert!(!limited.truncated);
    assert_eq!(limited.context.subgraph.nodes.len(), 1);
    assert_eq!(
        limited.context.subgraph.roots,
        vec![limited.context.subgraph.nodes[0].id.clone()]
    );
}

#[test]
fn context_uses_committed_source_and_preserves_bounds_across_connections() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("project with spaces");
    fs::create_dir(&root).unwrap();
    let database = temporary.path().join("index.sqlite");
    fs::write(
        root.join("auth.ts"),
        "export function validateToken() { return 'old token'; }\n",
    )
    .unwrap();
    fs::write(root.join("entry.ts"), "import { validateToken } from './auth';\nexport function login() { return validateToken(); }\n").unwrap();
    let mut writer = Engine::open(&root, Some(&database)).unwrap();
    writer.sync().unwrap();
    let observer = Engine::open(&root, Some(&database)).unwrap();
    let query = TaskInput::Query("validateToken".into());
    let options = BuildContextOptions::default();
    let read = |engine: &Engine, budget| engine.context(&query, &options, budget).unwrap();
    let before = read(&observer, 65536);
    assert!(
        before
            .context
            .code_blocks
            .iter()
            .any(|block| block.content.contains("old token"))
    );
    assert!(before.markdown().contains("validateToken"));
    fs::write(
        root.join("auth.ts"),
        "export function validateToken() { return 'new token'; }\n",
    )
    .unwrap();
    assert!(
        read(&observer, 65536)
            .context
            .code_blocks
            .iter()
            .any(|block| block.content.contains("old token"))
    );
    writer.sync_paths(&["auth.ts".into()]).unwrap();
    let after = read(&observer, 65536);
    assert!(after.generation > before.generation);
    assert!(
        after
            .context
            .code_blocks
            .iter()
            .any(|block| block.content.contains("new token"))
    );
    let bounded = read(&observer, 1024);
    assert!(bounded.truncated);
    assert!(serde_json::to_vec(&bounded).unwrap().len() <= 1024);
    assert!(bounded.markdown().len() <= 1024);
    let ids = bounded
        .context
        .subgraph
        .nodes
        .iter()
        .map(|node| node.id.as_str())
        .collect::<HashSet<_>>();
    assert!(
        bounded
            .context
            .subgraph
            .roots
            .iter()
            .all(|id| ids.contains(id.as_str()))
    );
    assert!(
        bounded
            .context
            .subgraph
            .edges
            .iter()
            .all(|edge| ids.contains(edge.source.as_str()) && ids.contains(edge.target.as_str()))
    );
    assert_eq!(bounded.context.stats.node_count, ids.len());
    drop(observer);
    let reopened = Engine::open(&root, Some(&database)).unwrap();
    assert_eq!(
        read(&reopened, 65536).context.code_blocks[0].content,
        after.context.code_blocks[0].content
    );
    assert!(
        reopened
            .context(&TaskInput::Query(" ".into()), &options, 1024)
            .is_err()
    );
    assert!(
        reopened
            .context(
                &query,
                &BuildContextOptions {
                    search_limit: usize::MAX,
                    ..options.clone()
                },
                usize::MAX
            )
            .is_err()
    );
    assert!(
        reopened
            .context(
                &query,
                &BuildContextOptions {
                    traversal_depth: 9,
                    ..options.clone()
                },
                1024
            )
            .is_ok()
    );
    assert!(
        reopened
            .context(
                &query,
                &BuildContextOptions {
                    min_score: f64::NAN,
                    ..options
                },
                1024
            )
            .is_err()
    );
}
