use prometeu_search::{
    Engine,
    viewer::{ViewerOptions, ViewerServer},
};
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::net::{Shutdown, TcpStream};
use std::path::Path;
use tempfile::TempDir;

fn fixture() -> (TempDir, TempDir, Engine) {
    let root = TempDir::new().unwrap();
    let cache = TempDir::new().unwrap();
    std::fs::write(root.path().join("worker.ts"),"export function finish() { return 42; }\nexport function middle() { return finish(); }\nexport function start() { return middle(); }\n").unwrap();
    std::fs::write(
        root.path().join("consumer.ts"),
        "import { start } from './worker';\nexport function main() { return start(); }\n",
    )
    .unwrap();
    let mut engine = Engine::open(root.path(), Some(&cache.path().join("index.sqlite"))).unwrap();
    engine.sync().unwrap();
    (root, cache, engine)
}

fn http(
    root: &Path,
    database: &Path,
    readonly: bool,
    method: &str,
    target: &str,
    headers: &[(&str, &str)],
    body: &str,
) -> (u16, String, String) {
    let server = ViewerServer::bind(0, readonly).unwrap();
    let address = server.address();
    let mut engine = Engine::open(root, Some(database)).unwrap();
    let handler = std::thread::spawn(move || server.serve_one(&mut engine).unwrap());
    let mut stream = TcpStream::connect(address).unwrap();
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(10)))
        .unwrap();
    let host = headers
        .iter()
        .find(|(name, _)| *name == "Host")
        .map(|(_, value)| value.to_string())
        .unwrap_or_else(|| address.to_string());
    write!(
        stream,
        "{method} {target} HTTP/1.1\r\nHost: {host}\r\nContent-Length: {}\r\n",
        body.len()
    )
    .unwrap();
    for (key, value) in headers.iter().filter(|(name, _)| *name != "Host") {
        write!(stream, "{key}: {value}\r\n").unwrap();
    }
    write!(stream, "\r\n{body}").unwrap();
    stream.shutdown(Shutdown::Write).unwrap();
    let mut output = String::new();
    stream.read_to_string(&mut output).unwrap();
    assert!(handler.join().unwrap());
    let (head, body) = output.split_once("\r\n\r\n").unwrap();
    let status = head.split_whitespace().nth(1).unwrap().parse().unwrap();
    (status, head.into(), body.into())
}

#[test]
fn viewer_queries_share_graph_and_committed_source_across_reopen_and_changes() {
    let (root, cache, mut engine) = fixture();
    let graph = engine.viewer_graph(&ViewerOptions::default()).unwrap();
    let start = graph
        .nodes
        .iter()
        .find(|node| node["name"] == "start")
        .unwrap();
    let id = start["id"].as_str().unwrap();
    let details = engine.viewer_query(&format!("/api/node/{id}")).unwrap();
    assert_eq!(details["outgoing"]["items"][0]["node"]["name"], "middle");
    assert!(
        details["incoming"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["node"]["name"] == "main")
    );
    let flow = engine
        .viewer_query("/api/flow?from=start&to=finish")
        .unwrap();
    assert_eq!(
        flow["flows"][0]["hops"]
            .as_array()
            .unwrap()
            .iter()
            .map(|hop| hop["node"]["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["start", "middle", "finish"]
    );
    let search = engine
        .viewer_query("/api/search?q=kind%3Afunction%20start")
        .unwrap();
    assert_eq!(search["results"]["items"][0]["name"], "start");
    let file = engine.viewer_query("/api/filecode/worker.ts").unwrap();
    assert!(
        file["source"]["text"]
            .as_str()
            .unwrap()
            .contains("return 42")
    );
    std::fs::write(
        root.path().join("worker.ts"),
        "export function finish() { return 99; }\n",
    )
    .unwrap();
    assert_eq!(
        engine.viewer_query("/api/filecode/worker.ts").unwrap(),
        file
    );
    let reopened = Engine::open(root.path(), Some(&cache.path().join("index.sqlite"))).unwrap();
    assert_eq!(
        reopened.viewer_query("/api/filecode/worker.ts").unwrap(),
        file
    );
    drop(reopened);
    engine.sync().unwrap();
    let fresh = engine.viewer_query("/api/filecode/worker.ts").unwrap();
    assert!(
        fresh["source"]["text"]
            .as_str()
            .unwrap()
            .contains("return 99")
    );
    assert_ne!(fresh["generation"], file["generation"]);
    let limited = engine
        .viewer_graph(&ViewerOptions {
            limit: 1,
            max_bytes: 1024,
            ..ViewerOptions::default()
        })
        .unwrap();
    assert!(limited.truncated);
    assert!(serde_json::to_vec(&limited).unwrap().len() <= 1024);
    assert!(limited.edges.iter().all(|edge| {
        limited.nodes.iter().any(|node| node["id"] == edge["from"])
            && (edge["to"].is_null() || limited.nodes.iter().any(|node| node["id"] == edge["to"]))
    }));
    for target in [
        "/api/source?file=../secret",
        "/api/file/%2e%2e/secret",
        "/api/graph?root=%2Ftmp",
        "/api/map?depth=99",
    ] {
        assert!(engine.viewer_query(target).is_err(), "{target}");
    }
    for target in [
        "/api/stats",
        "/api/routes",
        "/api/entrypoints",
        "/api/screens",
        "/api/map",
        "/api/deadcode",
    ] {
        assert!(engine.viewer_query(target).is_ok(), "{target}");
    }
}

#[test]
fn viewer_http_serves_embedded_assets_checks_origin_and_persists_resolved_trails() {
    let (root, cache, engine) = fixture();
    let database = cache.path().join("index.sqlite");
    let (status, headers, body) = http(root.path(), &database, false, "GET", "/", &[], "");
    assert_eq!(status, 200);
    assert!(body.contains("Prometeu Search"));
    assert!(headers.contains("Content-Security-Policy:"));
    assert!(!headers.contains("Access-Control-Allow-Origin"));
    let (status, _, body) = http(root.path(), &database, false, "GET", "/viewer.js", &[], "");
    assert_eq!(status, 200);
    assert!(body.contains("drawGraph"));
    assert!(!body.contains("innerHTML"));
    let (status, _, body) = http(root.path(), &database, false, "HEAD", "/", &[], "");
    assert_eq!(status, 200);
    assert!(body.is_empty());
    for headers in [
        vec![("Host", "attacker.invalid")],
        vec![("Origin", "https://attacker.invalid")],
        vec![("Sec-Fetch-Site", "cross-site")],
    ] {
        assert_eq!(
            http(
                root.path(),
                &database,
                false,
                "GET",
                "/api/stats",
                &headers,
                ""
            )
            .0,
            403
        );
    }
    assert_eq!(
        http(
            root.path(),
            &database,
            false,
            "GET",
            "/api/source?file=..%2Foutside",
            &[],
            ""
        )
        .0,
        400
    );
    let graph = engine.viewer_graph(&ViewerOptions::default()).unwrap();
    let start = graph
        .nodes
        .iter()
        .find(|node| node["name"] == "start")
        .unwrap();
    let body =
        json!({"name":"Read startup","hops":[start["id"]],"note":"Saved from test"}).to_string();
    assert_eq!(
        http(
            root.path(),
            &database,
            false,
            "POST",
            "/api/trails",
            &[("Content-Type", "application/json")],
            &body
        )
        .0,
        403
    );
    let headers = [
        ("Content-Type", "application/json"),
        ("X-Viewer-Write", "1"),
    ];
    assert_eq!(
        http(
            root.path(),
            &database,
            true,
            "POST",
            "/api/trails",
            &headers,
            &body
        )
        .0,
        403
    );
    let (status, _, saved) = http(
        root.path(),
        &database,
        false,
        "POST",
        "/api/trails",
        &headers,
        &body,
    );
    assert_eq!(status, 200);
    let saved: Value = serde_json::from_str(&saved).unwrap();
    let (status, _, listed) = http(root.path(), &database, true, "GET", "/api/trails", &[], "");
    assert_eq!(status, 200);
    let listed: Value = serde_json::from_str(&listed).unwrap();
    assert_eq!(listed["trails"][0]["hops"][0]["status"], "resolved");
    assert_eq!(listed["readOnly"], true);
    let target = format!("/api/flow?hop=s{}", start["id"].as_str().unwrap());
    let (status, _, flow) = http(root.path(), &database, false, "GET", &target, &[], "");
    assert_eq!(status, 200);
    let flow: Value = serde_json::from_str(&flow).unwrap();
    assert_eq!(flow["flows"][0]["hops"].as_array().unwrap().len(), 1);
    assert_eq!(flow["flows"][0]["hops"][0]["node"]["id"], start["id"]);
    assert!(flow["reason"].is_null());
    std::fs::write(
        root.path().join("worker.ts"),
        "export function finish() {}\n",
    )
    .unwrap();
    let (_, _, listed) = http(root.path(), &database, false, "GET", "/api/trails", &[], "");
    let listed: Value = serde_json::from_str(&listed).unwrap();
    assert_eq!(listed["trails"][0]["hops"][0]["status"], "missing");
    let target = format!("/api/trails/{}", saved["id"].as_str().unwrap());
    assert_eq!(
        http(
            root.path(),
            &database,
            false,
            "DELETE",
            &target,
            &headers,
            ""
        )
        .0,
        200
    );
    let (_, _, events) = http(root.path(), &database, false, "GET", "/api/events", &[], "");
    assert!(events.contains("event: index"));
}

#[test]
fn viewer_steps_preserve_branch_decisions_effects_and_alternative_flows() {
    let root = TempDir::new().unwrap();
    let cache = TempDir::new().unwrap();
    std::fs::write(
        root.path().join("app.ts"),
        r#"export function handler(req, res) {
  if (!req.ok) { return res.status(403).json({error: 'denied'}); }
  const user = db.users.findOne(req.id);
  if (user) { return res.json(user); }
  return res.status(404).send('missing');
}
export function finish() { return 1; }
export function left() { return finish(); }
export function right() { return finish(); }
export function start(flag) { if (flag) { left(); } else { right(); } }
function helper() { fetch('/items'); }
export function guarded(flag) { if (flag) { helper(); } else { helper(); } }
"#,
    )
    .unwrap();
    let mut engine = Engine::open(root.path(), Some(&cache.path().join("index.sqlite"))).unwrap();
    engine.sync().unwrap();
    let steps = engine.viewer_query("/api/steps?symbol=handler").unwrap();
    let nodes = steps["nodes"].as_array().unwrap();
    assert!(
        nodes
            .iter()
            .any(|node| node["effect"]["category"] == "database"
                && node["effect"]["model"] == "users"
                && node["effect"]["access"] == "read")
    );
    assert!(
        nodes
            .iter()
            .any(|node| node["effect"]["category"] == "response" && node["status"] == 403)
    );
    assert!(
        nodes
            .iter()
            .any(|node| node["effect"]["category"] == "response" && node["status"] == 404)
    );
    assert!(
        steps["program"]["root"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["kind"] == "fork")
    );
    assert!(steps["links"].as_array().unwrap().iter().any(|link| {
        link["when"]
            .as_str()
            .is_some_and(|when| when.contains("req.ok"))
    }));
    let alternatives = engine.viewer_query("/api/steps?symbol=guarded").unwrap();
    let folded = alternatives["links"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|link| {
            link["via"]
                .as_array()
                .is_some_and(|via| via.iter().any(|node| node["name"] == "helper"))
        })
        .collect::<Vec<_>>();
    assert_eq!(folded.len(), 2, "{alternatives}");
    let negated = folded
        .iter()
        .map(|link| link["guards"][0]["negated"].as_bool().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(negated, std::collections::BTreeSet::from([false, true]));

    let directed = engine
        .viewer_query("/api/flow?from=start&to=finish&limit=4")
        .unwrap();
    assert_eq!(directed["flows"].as_array().unwrap().len(), 2);
    let named = engine
        .viewer_query("/api/flow?symbols=start%20left%20finish")
        .unwrap();
    assert_eq!(named["query"]["kind"], "symbols");
    assert_eq!(
        named["flows"][0]["hops"]
            .as_array()
            .unwrap()
            .iter()
            .map(|hop| hop["node"]["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["start", "left", "finish"]
    );
    assert!(directed["flows"][0]["hops"][0]["edge"].is_null());
    assert!(
        directed["flows"][0]["hops"][1]["edge"]["when"]
            .as_str()
            .is_some_and(|when| when.contains("flag"))
    );
}

#[test]
fn viewer_limits_follow_each_endpoint_without_a_global_response_ceiling() {
    let root = TempDir::new().unwrap();
    let cache = TempDir::new().unwrap();
    let mut source = String::from("export function target() { return 0; }\n");
    for index in 0..3200 {
        let body = if index < 310 {
            "target()".to_owned()
        } else {
            index.to_string()
        };
        source.push_str(&format!(
            "export function outline_item_{index:04}() {{ return {body}; }}\n"
        ));
    }
    source.push_str(
        &"// More source lines remain available through subsequent windows.\n".repeat(1100),
    );
    std::fs::write(root.path().join("large.ts"), &source).unwrap();
    let suffix = "x".repeat(160);
    for file in 0..8 {
        let content = (0..400)
            .map(|index| {
                format!("export function long_{file}_{index:04}_{suffix}() {{ return {index}; }}\n")
            })
            .collect::<String>();
        std::fs::write(root.path().join(format!("part{file}.ts")), content).unwrap();
    }
    let database = cache.path().join("index.sqlite");
    let mut engine = Engine::open(root.path(), Some(&database)).unwrap();
    engine.sync().unwrap();

    let graph = engine
        .viewer_graph(&ViewerOptions {
            limit: 10000,
            depth: 14,
            ..ViewerOptions::default()
        })
        .unwrap();
    assert_eq!(graph.nodes.len(), graph.total_nodes);
    assert!(graph.nodes.len() > 3000);
    assert!(serde_json::to_vec(&graph).unwrap().len() > 2 * 1024 * 1024);
    let target = graph
        .nodes
        .iter()
        .find(|node| node["name"] == "target")
        .unwrap();
    let detail = engine
        .viewer_query(&format!("/api/node/{}", target["id"].as_str().unwrap()))
        .unwrap();
    assert_eq!(detail["incoming"]["shown"], 300);
    assert!(detail["incoming"]["total"].as_u64().unwrap() >= 310);
    assert_eq!(detail["incoming"]["truncated"], true);

    let source_view = engine.viewer_query("/api/source?file=large.ts").unwrap();
    assert_eq!(source_view["to"], 4000);
    assert_eq!(
        source_view["text"],
        source.lines().take(4000).collect::<Vec<_>>().join("\n")
    );
    assert!(source_view["text"].as_str().unwrap().len() > 128 * 1024);
    assert_eq!(source_view["truncated"], true);
    let remainder = engine
        .viewer_query("/api/source?file=large.ts&from=4001")
        .unwrap();
    assert_eq!(remainder["to"], 4301);
    assert_eq!(
        remainder["text"],
        source.lines().skip(4000).collect::<Vec<_>>().join("\n")
    );
    assert_eq!(remainder["truncated"], false);
    let outline = engine.viewer_query("/api/file/large.ts").unwrap();
    assert_eq!(outline["outline"]["shown"], 3000);
    assert_eq!(outline["outline"]["truncated"], true);
    let default_search = engine
        .viewer_query("/api/search?q=kind%3Afunction")
        .unwrap();
    assert_eq!(default_search["results"]["shown"], 60);
    let search = engine
        .viewer_query("/api/search?q=kind%3Afunction&limit=200")
        .unwrap();
    assert_eq!(search["results"]["shown"], 200);

    for path in [
        "/api/search?q=target&limit=201",
        "/api/routes?limit=2",
        "/api/routes?limit=501",
        "/api/entrypoints?limit=51",
        "/api/entrypoints?routes=301",
        "/api/deadcode?limit=301",
        "/api/steps?symbol=target&limit=19",
        "/api/steps?symbol=target&limit=401",
        "/api/steps?symbol=target&depth=15",
    ] {
        assert!(engine.viewer_query(path).is_err(), "{path}");
    }
    let steps = engine.viewer_query("/api/steps?symbol=target").unwrap();
    assert_eq!(steps["limit"], 120);
    assert_eq!(steps["depth"], 8);

    let (status, _, body) = http(
        root.path(),
        &database,
        false,
        "GET",
        "/api/graph?limit=10000&depth=14",
        &[],
        "",
    );
    assert_eq!(status, 200);
    assert!(body.len() > 2 * 1024 * 1024);
    let served: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(served["nodes"].as_array().unwrap().len(), graph.nodes.len());
}

#[test]
fn viewer_steps_revisit_shared_helpers_reached_at_a_lower_level() {
    let root = TempDir::new().unwrap();
    let cache = TempDir::new().unwrap();
    std::fs::create_dir(root.path().join("stores")).unwrap();
    std::fs::write(
        root.path().join("app.ts"),
        "import { enter, finish } from './stores/state';\n\
         export function start() { enter(); detour(); }\n\
         function detour() { hop(); }\n\
         function hop() { shared(); }\n\
         export function shared() { fetch('/shared'); finish(); }\n",
    )
    .unwrap();
    std::fs::write(
        root.path().join("stores/state.ts"),
        "import { shared } from '../app';\n\
         export function enter() { shared(); }\n\
         export function finish() { fetch('/reachable'); unknownCall(); }\n",
    )
    .unwrap();
    let mut engine = Engine::open(root.path(), Some(&cache.path().join("index.sqlite"))).unwrap();
    engine.sync().unwrap();
    let steps = engine
        .viewer_query("/api/steps?symbol=start&depth=2")
        .unwrap();
    let effects = steps["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|node| node["kind"] == "external")
        .collect::<Vec<_>>();
    assert_eq!(effects.len(), 2, "{steps}");
    assert!(effects.iter().any(|node| node["file"] == "stores/state.ts"));
    assert_eq!(steps["outsideIndex"]["items"].as_array().unwrap().len(), 1);
}

#[test]
fn viewer_steps_can_follow_fourteen_semantic_levels() {
    let root = TempDir::new().unwrap();
    let cache = TempDir::new().unwrap();
    std::fs::create_dir(root.path().join("stores")).unwrap();
    let mut workers = String::new();
    let mut stores = String::new();
    for index in 0..12 {
        workers.push_str(&format!("import {{ state{index} }} from './stores/state';\nexport function run{index}() {{ return state{index}(); }}\n"));
        if index < 11 {
            stores.push_str(&format!("import {{ run{} }} from '../workers';\nexport function state{index}() {{ return run{}(); }}\n", index + 1, index + 1));
        } else {
            stores.push_str("export function state11() { return fetch('/finished'); }\n");
        }
    }
    std::fs::write(root.path().join("workers.ts"), workers).unwrap();
    std::fs::write(root.path().join("stores/state.ts"), stores).unwrap();
    let mut engine = Engine::open(root.path(), Some(&cache.path().join("index.sqlite"))).unwrap();
    engine.sync().unwrap();
    let shallow = engine
        .viewer_query("/api/steps?symbol=run0&depth=8")
        .unwrap();
    let deep = engine
        .viewer_query("/api/steps?symbol=run0&depth=14")
        .unwrap();
    assert!(
        !shallow["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|node| node["name"] == "state11")
    );
    assert!(
        deep["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|node| node["name"] == "state11")
    );
}

#[test]
fn generation_events_read_committed_state_without_reconciling_source() {
    let (root, cache, engine) = fixture();
    let database = cache.path().join("index.sqlite");
    let initial = engine.status().unwrap().generation;
    std::fs::write(
        root.path().join("worker.ts"),
        "export function changed() { return 99; }\n",
    )
    .unwrap();
    for _ in 0..2 {
        let (status, _, events) =
            http(root.path(), &database, false, "GET", "/api/events", &[], "");
        assert_eq!(status, 200);
        assert!(events.contains(&format!("\"generation\":{initial}")));
        assert_eq!(engine.status().unwrap().generation, initial);
    }
    let (status, _, stats) = http(root.path(), &database, false, "GET", "/api/stats", &[], "");
    assert_eq!(status, 200);
    let stats: Value = serde_json::from_str(&stats).unwrap();
    assert!(stats["generation"].as_u64().unwrap() > initial);
    let current = engine.status().unwrap().generation;
    let (_, _, events) = http(root.path(), &database, false, "GET", "/api/events", &[], "");
    assert!(events.contains(&format!("\"generation\":{current}")));
}
