use prometeu_search::{Engine, GraphOptions, GraphResult};
use std::{fs, path::Path};
fn write(root: &Path, path: &str, source: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, source).unwrap();
}
fn handler(result: &GraphResult) -> String {
    let edge = result
        .edges
        .iter()
        .find(|edge| {
            edge.metadata
                .as_ref()
                .is_some_and(|m| m["framework"] == "vapor" && m["confidence"] == 0.9)
        })
        .unwrap();
    let id = edge.to.unwrap();
    result
        .files
        .iter()
        .find(|file| file.symbols.iter().any(|symbol| symbol.id == id))
        .unwrap()
        .path
        .clone()
}
#[test]
fn vapor_route_handler_changes_refresh_only_after_committed_source() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("project");
    fs::create_dir(&root).unwrap();
    let database = temporary.path().join("index.sqlite");
    write(
        &root,
        "Package.swift",
        "import PackageDescription\nlet package = Package(name: \"App\", dependencies: [.package(url: \"https://github.com/vapor/vapor.git\", from: \"4.0.0\")])\n",
    );
    for name in ["FirstController", "SecondController"] {
        write(
            &root,
            &format!("Sources/App/{name}.swift"),
            &format!(
                "import Vapor\nenum {name} {{ static func show(req: Request) -> String {{ \"ok\" }} }}\n"
            ),
        );
    }
    let routes = |name: &str| {
        format!(
            "import Vapor\nfunc routes(_ app: Application) {{\n app.get(\"item\", use: {name}.show)\n}}\n"
        )
    };
    write(
        &root,
        "Sources/App/routes.swift",
        &routes("FirstController"),
    );
    let options = GraphOptions {
        limit: 30,
        max_bytes: 65536,
        depth: 1,
        ..Default::default()
    };
    let mut first = Engine::open(&root, Some(&database)).unwrap();
    first.sync().unwrap();
    let before = first.explore("GET /item", &options).unwrap();
    assert!(handler(&before).ends_with("FirstController.swift"));
    write(
        &root,
        "Sources/App/routes.swift",
        &routes("SecondController"),
    );
    assert!(
        handler(&first.explore("GET /item", &options).unwrap()).ends_with("FirstController.swift")
    );
    let mut second = Engine::open(&root, Some(&database)).unwrap();
    second
        .sync_paths(&["Sources/App/routes.swift".into()])
        .unwrap();
    let after = first.explore("GET /item", &options).unwrap();
    assert!(after.generation > before.generation);
    assert!(handler(&after).ends_with("SecondController.swift"));
    drop(first);
    write(
        &root,
        "Sources/App/routes.swift",
        &routes("FirstController"),
    );
    let reopened = Engine::open(&root, Some(&database)).unwrap();
    assert!(
        handler(&reopened.explore("GET /item", &options).unwrap())
            .ends_with("SecondController.swift")
    );
    fs::remove_file(root.join("Sources/App/SecondController.swift")).unwrap();
    second
        .sync_paths(&["Sources/App/SecondController.swift".into()])
        .unwrap();
    assert!(
        reopened
            .explore("GET /item", &options)
            .unwrap()
            .edges
            .iter()
            .all(|edge| edge
                .metadata
                .as_ref()
                .is_none_or(|m| m["framework"] != "vapor"))
    );
}
