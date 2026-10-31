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
    assert_eq!(responses[2]["result"]["tools"].as_array().unwrap().len(), 2);
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
    assert_eq!(callers["relations"][0]["resolution"], "name_match");
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
