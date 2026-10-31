use prometeu_search::{Direction, Engine};
use std::fs;
use std::process::Command;
use tempfile::TempDir;

#[test]
fn existing_index_adds_ruby_and_reconciles_edits_and_deletions() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().join("project");
    fs::create_dir(&root).unwrap();
    let database = temp.path().join("index.sqlite");
    let mut engine = Engine::open(&root, Some(&database)).unwrap();
    fs::write(root.join("existing.ts"), "function existing() {}").unwrap();
    engine.sync().unwrap();
    let generation = engine.status().unwrap().generation;
    drop(engine);

    fs::write(
        root.join("store.rb"),
        "class Store\n  def save!\n    persist()\n  end\n  def persist\n  end\nend\n",
    )
    .unwrap();
    fs::write(
        root.join("Gemfile"),
        "source 'https://rubygems.org'\ngem 'rails'\n",
    )
    .unwrap();
    let mut engine = Engine::open(&root, Some(&database)).unwrap();
    assert_eq!(engine.sync().unwrap().indexed, 2);
    assert!(engine.status().unwrap().generation > generation);
    assert!(
        engine
            .search("rails", 10, 12000)
            .unwrap()
            .results
            .iter()
            .any(|hit| hit.path == "Gemfile")
    );
    let callers = engine
        .related("persist", Direction::Callers, 10, 12000)
        .unwrap();
    assert_eq!(callers.relations[0].caller.name, "save!");
    assert_eq!(
        callers.relations[0].candidate.as_ref().unwrap().path,
        "store.rb"
    );
    fs::write(
        root.join("store.rb"),
        "class Store\n  def load\n    fetch()\n  end\nend\n",
    )
    .unwrap();
    engine.sync().unwrap();
    assert!(
        engine
            .related("persist", Direction::Callers, 10, 12000)
            .unwrap()
            .relations
            .is_empty()
    );
    fs::remove_file(root.join("store.rb")).unwrap();
    assert_eq!(engine.sync().unwrap().removed, 1);
    assert!(
        engine
            .search("Store", 10, 12000)
            .unwrap()
            .results
            .is_empty()
    );
    assert!(
        !engine
            .search("existing", 10, 12000)
            .unwrap()
            .results
            .is_empty()
    );
}

#[test]
fn standalone_cli_returns_ruby_source_and_method_calls() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().join("project");
    fs::create_dir(&root).unwrap();
    fs::write(
        root.join("billing.rb"),
        "def charge!\n  authorize()\nend\ndef authorize\n  true\nend\n",
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_prometeu-search"))
        .arg("--root")
        .arg(&root)
        .arg("--database")
        .arg(temp.path().join("index.sqlite"))
        .args(["callers", "authorize"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["relations"][0]["caller"]["name"], "charge!");
    assert_eq!(result["relations"][0]["candidate"]["path"], "billing.rb");
}
