use prometeu_search::{Engine, GraphOptions};
use std::fs;
#[test]
fn changing_a_lazy_export_refreshes_other_connections_from_committed_sources() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    fs::create_dir_all(root.join("src")).unwrap();
    let database = temp.path().join("index.sqlite");
    fs::write(
        root.join("package.json"),
        r#"{"dependencies":{"react":"18","react-router":"7"}}"#,
    )
    .unwrap();
    fs::write(root.join("src/router.tsx"),"import {lazy} from 'react';\nimport {Route} from 'react-router';\nconst Page=lazy(()=>import('./page'));\nexport function Router(){return <Route path=\"/login\" element={<Page/>}/>;}\n").unwrap();
    let page = root.join("src/page.tsx");
    fs::write(&page, "export default function First(){return null;}\n").unwrap();
    let options = GraphOptions {
        path: Some("src/router.tsx".into()),
        depth: 1,
        limit: 50,
        max_bytes: 65536,
        ..Default::default()
    };
    let targets = |graph: &prometeu_search::GraphResult| {
        let mut names: Vec<_> = graph
            .edges
            .iter()
            .filter(|edge| {
                edge.metadata
                    .as_ref()
                    .is_some_and(|m| m["framework"] == "react")
            })
            .map(|edge| {
                graph
                    .files
                    .iter()
                    .flat_map(|file| &file.symbols)
                    .find(|node| Some(node.id) == edge.to)
                    .unwrap()
                    .name
                    .clone()
            })
            .collect();
        names.sort();
        names.dedup();
        names
    };
    let mut first = Engine::open(&root, Some(&database)).unwrap();
    first.sync().unwrap();
    let before = first.explore("/login", &options).unwrap();
    assert_eq!(targets(&before), ["First"]);
    fs::write(&page, "export default function Second(){return null;}\n").unwrap();
    assert_eq!(
        targets(&first.explore("/login", &options).unwrap()),
        ["First"]
    );
    let mut second = Engine::open(&root, Some(&database)).unwrap();
    second.sync_paths(&["src/page.tsx".into()]).unwrap();
    let after = first.explore("/login", &options).unwrap();
    assert!(after.generation > before.generation);
    assert_eq!(targets(&after), ["Second"]);
    drop(first);
    fs::write(&page, "export default function First(){return null;}\n").unwrap();
    let reopened = Engine::open(&root, Some(&database)).unwrap();
    assert_eq!(
        targets(&reopened.explore("/login", &options).unwrap()),
        ["Second"]
    );
}
