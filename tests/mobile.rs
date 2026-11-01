use prometeu_search::{Engine, GraphOptions};
use std::fs;
#[test]
fn bridge_aliases_refresh_only_after_commit_and_survive_reopening() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("project");
    fs::create_dir_all(&root).unwrap();
    let database = temporary.path().join("index.sqlite");
    fs::write(
        root.join("package.json"),
        r#"{"dependencies":{"react-native":"0.73"}}"#,
    )
    .unwrap();
    for module in ["Alpha", "Beta"] {
        fs::write(root.join(format!("{module}.m")),format!("@implementation {module}\nRCT_EXPORT_MODULE({module})\nRCT_EXPORT_METHOD(ping)\n@end\n")).unwrap();
    }
    let caller = |module: &str| {
        format!(
            "import {{NativeModules}} from 'react-native';\nconst bridge=NativeModules.{module};\nexport function run(){{bridge.ping();}}\n"
        )
    };
    let source = root.join("index.ts");
    fs::write(&source, caller("Alpha")).unwrap();
    let options = GraphOptions {
        path: Some("index.ts".into()),
        depth: 1,
        limit: 30,
        max_bytes: 65536,
        ..Default::default()
    };
    let modules = |result: &prometeu_search::GraphResult| {
        let mut modules: Vec<_> = result
            .edges
            .iter()
            .filter_map(|edge| {
                edge.metadata
                    .as_ref()
                    .filter(|m| m["framework"] == "react-native-bridge")
                    .map(|m| m["module"].as_str().unwrap().to_owned())
            })
            .collect();
        modules.sort();
        modules.dedup();
        modules
    };
    let mut first = Engine::open(&root, Some(&database)).unwrap();
    first.sync().unwrap();
    let before = first.explore("run", &options).unwrap();
    assert_eq!(modules(&before), ["Alpha"]);
    fs::write(&source, caller("Beta")).unwrap();
    assert_eq!(modules(&first.explore("run", &options).unwrap()), ["Alpha"]);
    let mut second = Engine::open(&root, Some(&database)).unwrap();
    second.sync_paths(&["index.ts".into()]).unwrap();
    let after = first.explore("run", &options).unwrap();
    assert!(after.generation > before.generation);
    assert_eq!(modules(&after), ["Beta"]);
    drop(first);
    fs::write(&source, caller("Alpha")).unwrap();
    let reopened = Engine::open(&root, Some(&database)).unwrap();
    assert_eq!(
        modules(&reopened.explore("run", &options).unwrap()),
        ["Beta"]
    );
    second.sync_paths(&["index.ts".into()]).unwrap();
    assert_eq!(
        modules(&reopened.explore("run", &options).unwrap()),
        ["Alpha"]
    );
}
