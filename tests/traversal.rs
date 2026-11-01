use prometeu_search::Engine;
use serde::Deserialize;
use std::{collections::BTreeMap, fs, path::Path};
use tempfile::TempDir;

#[derive(Deserialize)]
struct Corpus {
    revision: String,
    cases: Vec<Case>,
}
#[derive(Deserialize)]
struct Case {
    id: String,
    project: String,
    files: BTreeMap<String, Fixture>,
    expected: Vec<String>,
    #[serde(default)]
    environment: BTreeMap<String, String>,
}
#[derive(Deserialize)]
struct Fixture {
    text: Option<String>,
    hex: Option<String>,
}

fn corpus() -> Corpus {
    serde_json::from_str(include_str!("fixtures/upstream/traversal/cases.json")).unwrap()
}
fn materialize(case: &Case, workspace: &Path) {
    for (relative, fixture) in &case.files {
        let path = workspace.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let bytes = fixture
            .text
            .as_ref()
            .map(|text| {
                text.replace("<WORKSPACE>", workspace.to_str().unwrap())
                    .into_bytes()
            })
            .unwrap_or_else(|| {
                fixture
                    .hex
                    .as_ref()
                    .unwrap()
                    .as_bytes()
                    .chunks_exact(2)
                    .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
                    .collect()
            });
        fs::write(path, bytes).unwrap();
    }
}
fn paths(engine: &Engine) -> Vec<String> {
    let result = engine.files("", 50, 65536).unwrap();
    assert!(!result.truncated);
    result.files.into_iter().map(|file| file.path).collect()
}

#[test]
fn native_traversal_matches_pinned_reference_git_and_filesystem_projects() {
    let corpus = corpus();
    assert_eq!(corpus.revision, "6aa358ccbae30fbf27318f79ee183212f726f9ba");
    assert!(corpus.cases.len() >= 20);
    for case in corpus
        .cases
        .into_iter()
        .filter(|case| case.environment.is_empty())
    {
        let temporary = TempDir::new().unwrap();
        let workspace = temporary.path().join("workspace");
        materialize(&case, &workspace);
        let mut engine = Engine::open(
            &workspace.join(&case.project),
            Some(&temporary.path().join("index.sqlite")),
        )
        .unwrap();
        engine
            .sync()
            .unwrap_or_else(|error| panic!("{}: {error:#}", case.id));
        assert_eq!(paths(&engine), case.expected, "{}", case.id);
        assert_eq!(
            engine.sync().unwrap().indexed,
            0,
            "{} must remain incremental",
            case.id
        );
    }
}

#[test]
fn git_environment_configuration_matches_reference_without_external_runtime() {
    for case in corpus()
        .cases
        .into_iter()
        .filter(|case| !case.environment.is_empty())
    {
        let temp = TempDir::new().unwrap();
        let workspace = temp.path().join("workspace");
        materialize(&case, &workspace);
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_prometeu-search"))
            .env("PATH", "")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "")
            .env_remove("GIT_CONFIG_COUNT")
            .env_remove("GIT_CONFIG_PARAMETERS")
            .envs(case.environment.iter().map(|(key, value)| {
                (
                    key,
                    value.replace("<WORKSPACE>", workspace.to_str().unwrap()),
                )
            }))
            .arg("--root")
            .arg(workspace.join(&case.project))
            .arg("--database")
            .arg(temp.path().join("index.sqlite"))
            .args(["files", "--limit", "50", "--max-bytes", "65536"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}: {}",
            case.id,
            String::from_utf8_lossy(&output.stderr)
        );
        let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        let files = result["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["path"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(files, case.expected, "{}", case.id);
    }
}

#[test]
fn corrupt_git_metadata_rolls_back_the_complete_source_snapshot() {
    let case = corpus()
        .cases
        .into_iter()
        .find(|case| case.id == "index-version-four")
        .unwrap();
    let temporary = TempDir::new().unwrap();
    let workspace = temporary.path().join("workspace");
    materialize(&case, &workspace);
    let mut engine =
        Engine::open(&workspace, Some(&temporary.path().join("index.sqlite"))).unwrap();
    let first = engine.sync().unwrap();
    fs::write(workspace.join("new.ts"), "export function newSymbol() {}\n").unwrap();
    let index = workspace.join(".git/index");
    let original = fs::read(&index).unwrap();
    for invalid in [&original[..10], &original[..original.len() - 1]] {
        fs::write(&index, invalid).unwrap();
        assert!(engine.sync().is_err());
        assert_eq!(paths(&engine), case.expected);
        assert_eq!(
            engine.files("", 50, 65536).unwrap().generation,
            first.generation
        );
    }
    fs::write(&index, original).unwrap();
    let recovered = engine.sync().unwrap();
    assert_eq!(recovered.indexed, 1);
    assert!(recovered.generation > first.generation);
}

#[test]
fn changed_git_index_forces_targeted_updates_to_reconcile_visibility() {
    let corpus = corpus();
    let case = corpus
        .cases
        .iter()
        .find(|case| case.id == "index-version-four")
        .unwrap();
    let temporary = TempDir::new().unwrap();
    let workspace = temporary.path().join("workspace");
    materialize(case, &workspace);
    let mut engine =
        Engine::open(&workspace, Some(&temporary.path().join("index.sqlite"))).unwrap();
    let first = engine.sync().unwrap();
    let replacement = corpus
        .cases
        .iter()
        .find(|case| case.id == "repository-exclude-files")
        .unwrap();
    let bytes: Vec<_> = replacement.files[".git/index"]
        .hex
        .as_ref()
        .unwrap()
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect();
    fs::write(workspace.join(".git/index"), bytes).unwrap();
    fs::write(workspace.join("new.ts"), "export function changed() {}\n").unwrap();
    let report = engine.sync_paths(&["new.ts".into()]).unwrap();
    assert_eq!(report.removed, 3);
    assert_eq!(paths(&engine), ["new.ts"]);
    assert!(report.generation > first.generation);
}

#[test]
fn ignored_source_exceptions_remain_visible_to_the_watcher() {
    let temporary = TempDir::new().unwrap();
    let workspace = temporary.path().join("workspace");
    fs::create_dir_all(workspace.join("vendor")).unwrap();
    fs::write(workspace.join(".gitignore"), "!vendor/\n").unwrap();
    fs::write(workspace.join("vendor/first.ts"), "function first() {}\n").unwrap();
    let database = temporary.path().join("index.sqlite");
    let mut engine = Engine::open(&workspace, Some(&database)).unwrap();
    engine.sync().unwrap();
    let _watcher = engine.watch().unwrap();
    fs::write(workspace.join("vendor/second.ts"), "function second() {}\n").unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        if paths(&engine).len() == 2 {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "watcher missed an opted-in default directory"
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert_eq!(paths(&engine), ["vendor/first.ts", "vendor/second.ts"]);
}

#[cfg(unix)]
#[test]
fn metadata_and_opted_in_repositories_cannot_follow_source_symlinks() {
    use std::os::unix::fs::symlink;
    let case = corpus()
        .cases
        .into_iter()
        .find(|case| case.id == "embedded-ignored-opt-in")
        .unwrap();
    let temporary = TempDir::new().unwrap();
    let workspace = temporary.path().join("workspace");
    materialize(&case, &workspace);
    let outside = temporary.path().join("outside");
    fs::create_dir_all(outside.join(".git")).unwrap();
    fs::write(outside.join("secret.ts"), "function secret() {}\n").unwrap();
    symlink(&outside, workspace.join("repos/linked")).unwrap();
    let mut engine =
        Engine::open(&workspace, Some(&temporary.path().join("index.sqlite"))).unwrap();
    engine.sync().unwrap();
    assert_eq!(paths(&engine), case.expected);
    let before = engine.files("", 50, 65536).unwrap().generation;
    symlink(outside.join("secret.ts"), workspace.join(".git/index")).unwrap();
    assert!(engine.sync().is_err());
    assert_eq!(paths(&engine), case.expected);
    assert_eq!(engine.files("", 50, 65536).unwrap().generation, before);
}
