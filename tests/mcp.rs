use serde_json::{Value, json};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

fn initialize(id: u32) -> Value {
    json!({"jsonrpc":"2.0", "id":id, "method":"initialize", "params":{
        "protocolVersion":"2025-06-18", "capabilities":{},
        "clientInfo":{"name":"test", "version":"1"}
    }})
}

fn run(root: &Path, database: &Path, messages: &[Value]) -> Vec<Value> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_prometeu-search"))
        .args([
            "--root",
            root.to_str().unwrap(),
            "--database",
            database.to_str().unwrap(),
            "serve",
            "--no-watch",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    for message in messages {
        writeln!(input, "{message}").unwrap();
    }
    drop(input);
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).expect("stdout contains only JSON-RPC"))
        .collect()
}

#[test]
fn linked_server_matches_standalone_protocol_and_search() {
    let root = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("main.rs"), "fn greeting() {}\n").unwrap();
    let database = cache.path().join("index.sqlite");
    let messages = [
        initialize(1),
        json!({"jsonrpc":"2.0", "method":"notifications/initialized"}),
        json!({"jsonrpc":"2.0", "id":2, "method":"tools/list"}),
        json!({"jsonrpc":"2.0", "id":3, "method":"tools/call", "params":{"name":"code_search", "arguments":{"query":"greeting", "max_bytes":1024}}}),
    ];
    let standalone = run(root.path(), &database, &messages);
    let mut engine = prometeu_search::Engine::open(root.path(), Some(&database)).unwrap();
    let mut input = Vec::new();
    for message in messages {
        writeln!(input, "{message}").unwrap();
    }
    let mut output = Vec::new();
    prometeu_search::mcp::serve(&mut engine, input.as_slice(), &mut output).unwrap();
    let linked: Vec<Value> = String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(linked, standalone);
    assert_eq!(linked.len(), 3, "notifications have no response");
    assert_eq!(linked[2]["result"]["isError"], false);
    assert!(
        linked[2]["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("greeting")
    );
}

#[test]
fn stdio_handshake_search_validation_and_eof() {
    let root = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("main.rs"),
        "fn greet() { println!(\"hello\"); }\nfn main() { greet(); }\n",
    )
    .unwrap();
    let messages = vec![
        json!({"jsonrpc":"2.0", "id":0, "method":"tools/list"}),
        initialize(1),
        json!({"jsonrpc":"2.0", "method":"notifications/initialized"}),
        json!({"jsonrpc":"2.0", "id":2, "method":"tools/list"}),
        json!({"jsonrpc":"2.0", "id":3, "method":"tools/call", "params":{"name":"code_search", "arguments":{"query":"greet", "max_bytes":1024}}}),
        json!({"jsonrpc":"2.0", "id":4, "method":"tools/call", "params":{"name":"code_search", "arguments":{"query":"greet", "root":"/"}}}),
        json!({"jsonrpc":"2.0", "id":5, "method":"tools/call", "params":{"name":"code_search", "arguments":{"query":"greet", "limit":0}}}),
        json!({"jsonrpc":"2.0", "id":6, "method":"tools/call", "params":{"name":"code_search_status"}}),
        json!({"jsonrpc":"2.0", "id":7, "method":"ping"}),
    ];
    let responses = run(root.path(), &cache.path().join("index.sqlite"), &messages);
    assert_eq!(responses.len(), 8, "notifications have no response");
    assert_eq!(responses[0]["error"]["code"], -32002);
    assert_eq!(responses[1]["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(responses[2]["result"]["tools"].as_array().unwrap().len(), 3);
    let search = &responses[3]["result"];
    assert_eq!(search["isError"], false);
    let content = search["content"][0]["text"].as_str().unwrap();
    assert!(content.contains("greet"));
    assert!(content.len() <= 1024);
    assert_eq!(responses[4]["error"]["code"], -32602);
    assert_eq!(responses[5]["error"]["code"], -32602);
    let status: Value = serde_json::from_str(
        responses[6]["result"]["content"][0]["text"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(status["files"], 1);
    assert_eq!(responses[7]["result"], json!({}));
}

#[test]
fn protocol_negotiates_supported_versions_and_reports_invalid_requests() {
    let root = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let mut init = initialize(1);
    init["params"]["protocolVersion"] = json!("2099-01-01");
    let responses = run(
        root.path(),
        &cache.path().join("index.sqlite"),
        &[
            init,
            json!({"jsonrpc":"2.0", "id":true, "method":"ping"}),
            json!({"jsonrpc":"2.0", "id":2, "method":"not_a_method"}),
            json!({"jsonrpc":"2.0", "method":"notifications/unknown"}),
        ],
    );
    assert_eq!(responses.len(), 3);
    assert_eq!(responses[0]["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(responses[1]["error"]["code"], -32600);
    assert!(responses[1]["id"].is_null());
    assert_eq!(responses[2]["error"]["code"], -32601);
}

#[test]
fn malformed_and_oversized_messages_are_bounded() {
    let root = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_prometeu-search"))
        .arg("--root")
        .arg(root.path())
        .arg("--database")
        .arg(cache.path().join("index.sqlite"))
        .args(["serve", "--no-watch"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    writeln!(input, "{{broken").unwrap();
    // The server closes once the bound is exceeded; a broken pipe is expected.
    let _ = input.write_all(&vec![b'x'; 1024 * 1024 + 1]);
    drop(input);
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let responses: Vec<Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(responses.len(), 2);
    assert_eq!(responses[0]["error"]["code"], -32700);
    assert_eq!(responses[1]["error"]["code"], -32600);
}

#[test]
fn standalone_commands_reconcile_and_return_graph_candidates() {
    let root = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let source = root.path().join("main.ts");
    std::fs::write(
        &source,
        "function greet() {}\nfunction start() { greet(); }\n",
    )
    .unwrap();
    let command = |args: &[&str]| -> Value {
        let output = Command::new(env!("CARGO_BIN_EXE_prometeu-search"))
            .arg("--root")
            .arg(root.path())
            .arg("--database")
            .arg(cache.path().join("index.sqlite"))
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    };
    let callers = command(&["callers", "greet"]);
    assert_eq!(callers["relations"][0]["caller"]["name"], "start");
    assert_eq!(callers["relations"][0]["resolution"], "scope_match");
    let callees = command(&["callees", "start"]);
    assert_eq!(callees["relations"][0]["target"], "greet");
    std::fs::write(&source, "function replacement() {}\n").unwrap();
    let search = command(&["search", "replacement"]);
    assert_eq!(search["results"][0]["name"], "replacement");
    assert!(
        command(&["search", "greet"])["results"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn exploration_and_graph_modes_work_over_stdio_and_cli() {
    let root = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("store.ts"),
        "export function save() { return 'saved'; }\n",
    )
    .unwrap();
    std::fs::write(
        root.path().join("main.ts"),
        "import { save as persist } from './store';\nexport function start() { persist(); }\n",
    )
    .unwrap();
    std::fs::write(
        root.path().join("main.test.ts"),
        "import { start } from './main';\nfunction testStart() { start(); }\n",
    )
    .unwrap();
    let database = cache.path().join("index.sqlite");
    let requests = [
        json!({"query":"start", "path":"main.ts"}),
        json!({"query":"save", "mode":"impact", "depth":3}),
        json!({"query":"testStart", "mode":"path", "target":"save", "depth":3}),
        json!({"query":"store.ts", "mode":"affected", "depth":3}),
        json!({"query":"start", "max_bytes":1024}),
    ];
    let mut messages = vec![initialize(0)];
    messages.extend(requests.iter().enumerate().map(|(index, arguments)| {
        json!({"jsonrpc":"2.0", "id":index+1, "method":"tools/call", "params":{"name":"code_explore", "arguments":arguments}})
    }));
    let responses = run(root.path(), &database, &messages);
    let results: Vec<Value> = responses
        .iter()
        .skip(1)
        .map(|response| {
            assert_eq!(response["result"]["isError"], false, "{response}");
            serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap())
                .unwrap()
        })
        .collect();
    assert!(
        results[0]["files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|file| file["path"] == "store.ts")
    );
    assert!(
        results[0]["edges"]
            .as_array()
            .unwrap()
            .iter()
            .any(|edge| edge["resolution"] == "import_match")
    );
    assert!(
        results[1]["impact"]
            .as_array()
            .unwrap()
            .iter()
            .any(|node| node["name"] == "testStart")
    );
    assert_eq!(results[2]["path"].as_array().unwrap().len(), 3);
    assert_eq!(results[3]["affected_tests"], json!(["main.test.ts"]));
    assert!(
        responses[5]["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .len()
            <= 1024
    );

    for arguments in [
        vec!["explore", "start", "--path", "main.ts", "--line", "2"],
        vec!["impact", "save", "--depth", "3"],
        vec!["path", "testStart", "save", "--depth", "3"],
        vec!["implementations", "Store"],
        vec!["affected", "store.ts", "--depth", "3"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_prometeu-search"))
            .arg("--root")
            .arg(root.path())
            .arg("--database")
            .arg(&database)
            .args(&arguments)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{arguments:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let result: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(result["generation"], results[0]["generation"]);
        assert!(result["files"].is_array());
    }
}

#[test]
fn graph_arguments_reject_invalid_modes_limits_and_path_escape() {
    let root = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let invalid = [
        json!({"query":"save", "mode":"unknown"}),
        json!({"query":"save", "depth":0}),
        json!({"query":"save", "depth":9}),
        json!({"query":"save", "line":1}),
        json!({"query":"save", "path":"../outside.ts"}),
        json!({"query":"save", "path":"/outside.ts"}),
        json!({"query":"save", "mode":"path"}),
        json!({"query":"save", "target":"start"}),
        json!({"query":"save", "mode":"path", "target":""}),
        json!({"query":"save", "root":"/"}),
        json!({"query":"save", "max_bytes":100}),
    ];
    let mut messages = vec![initialize(0)];
    messages.extend(invalid.iter().enumerate().map(|(index, arguments)| {
        json!({"jsonrpc":"2.0", "id":index+1, "method":"tools/call", "params":{"name":"code_explore", "arguments":arguments}})
    }));
    let responses = run(root.path(), &cache.path().join("index.sqlite"), &messages);
    for response in responses.iter().skip(1) {
        assert_eq!(response["error"]["code"], -32602, "{response}");
    }
}
