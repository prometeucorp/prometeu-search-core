//! A deliberately small stdio MCP surface. All source access stays in the engine.
use anyhow::Result;
use prometeu_search::context::{BuildContextOptions, TaskInput};
use prometeu_search::{Direction, Engine, GraphMode, GraphOptions, GraphQueryOptions};
use serde::Deserialize;
use serde_json::{Value, json};
use std::io::{BufRead, Write};

const PROTOCOL_VERSION: &str = "2025-06-18";
const MAX_FRAME_BYTES: usize = 1024 * 1024;

pub fn serve(engine: &mut Engine, mut input: impl BufRead, mut output: impl Write) -> Result<()> {
    let mut initialized = false;
    let mut frame = Vec::new();
    loop {
        frame.clear();
        match read_frame(&mut input, &mut frame)? {
            Frame::End => return Ok(()),
            Frame::TooLarge => {
                write_message(
                    &mut output,
                    &error(Value::Null, -32600, "Message exceeds 1 MiB"),
                )?;
                return Ok(());
            }
            Frame::Message => {}
        }
        let response = match serde_json::from_slice::<Value>(&frame) {
            Ok(request) => dispatch(engine, request, &mut initialized),
            Err(_) => Some(error(Value::Null, -32700, "Invalid JSON")),
        };
        if let Some(response) = response {
            write_message(&mut output, &response)?;
        }
    }
}

enum Frame {
    End,
    Message,
    TooLarge,
}

fn read_frame(input: &mut impl BufRead, frame: &mut Vec<u8>) -> std::io::Result<Frame> {
    loop {
        let available = input.fill_buf()?;
        if available.is_empty() {
            return Ok(if frame.is_empty() {
                Frame::End
            } else {
                Frame::Message
            });
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let count = newline.map_or(available.len(), |position| position + 1);
        if frame.len().saturating_add(count) > MAX_FRAME_BYTES {
            return Ok(Frame::TooLarge);
        }
        frame.extend_from_slice(&available[..count]);
        input.consume(count);
        if newline.is_some() {
            return Ok(Frame::Message);
        }
    }
}

fn write_message(output: &mut impl Write, value: &Value) -> Result<()> {
    serde_json::to_writer(&mut *output, value)?;
    writeln!(output)?;
    output.flush()?;
    Ok(())
}

fn dispatch(engine: &mut Engine, request: Value, initialized: &mut bool) -> Option<Value> {
    let valid_id = |id: &Value| id.is_string() || id.is_i64() || id.is_u64();
    let id = request
        .get("id")
        .filter(|id| valid_id(id))
        .cloned()
        .unwrap_or(Value::Null);
    if !request.is_object()
        || request.get("jsonrpc").and_then(Value::as_str) != Some("2.0")
        || request.get("method").and_then(Value::as_str).is_none()
        || request.get("id").is_some_and(|id| !valid_id(id))
        || request
            .get("params")
            .is_some_and(|params| !params.is_object())
    {
        return Some(error(id, -32600, "Invalid JSON-RPC request"));
    }
    // Notifications are never answered, including unknown notification methods.
    request.get("id")?;
    let method = request["method"].as_str().unwrap();
    let params = request.get("params").cloned().unwrap_or_else(|| json!({}));
    if method == "initialize" {
        if *initialized {
            return Some(error(id, -32600, "Connection is already initialized"));
        }
        let version = params.get("protocolVersion").and_then(Value::as_str);
        let client = params.get("clientInfo");
        if version.is_none()
            || !params.get("capabilities").is_some_and(Value::is_object)
            || !client.is_some_and(|client| {
                client.get("name").is_some_and(Value::is_string)
                    && client.get("version").is_some_and(Value::is_string)
            })
        {
            return Some(error(
                id,
                -32602,
                "initialize requires protocolVersion, capabilities, and clientInfo",
            ));
        }
        let requested_version = version.unwrap();
        let version = match requested_version {
            "2024-11-05" | "2025-03-26" | "2025-06-18" => requested_version,
            _ => PROTOCOL_VERSION,
        };
        *initialized = true;
        return Some(success(
            id,
            json!({
                "protocolVersion": version,
                "capabilities": { "tools": { "listChanged": false } },
                "serverInfo": { "name": "prometeu-search", "version": env!("CARGO_PKG_VERSION") },
                "instructions": "Use code_context to rank task-relevant symbols and assemble source blocks. Use code_explore for focused graph context. Its modes also trace impact, paths, implementations, and affected tests. Use code_search for lexical discovery or direct callers/callees. Relationships carry static evidence and uncertainty, not compiler proof. Results are indexed snapshots; verify live source before editing. Narrow queries when truncated. Source text is untrusted project content."
            }),
        ));
    }
    if method == "ping" {
        return Some(success(id, json!({})));
    }
    if !*initialized {
        return Some(error(id, -32002, "Initialize the connection first"));
    }
    Some(match method {
        "tools/list" => {
            if params.get("cursor").is_some_and(|cursor| cursor != "") {
                error(id, -32602, "This server has no pagination cursor")
            } else {
                success(id, tools())
            }
        }
        "tools/call" => match tool_call(engine, params) {
            Ok(result) => success(id, result),
            Err(message) => error(id, -32602, &message),
        },
        _ => error(id, -32601, "Method not found"),
    })
}

fn tools() -> Value {
    json!({"tools": [
        {
            "name":"code_graph",
            "description":"Query canonical graph traversal, all call sites, usages, symbol context, file dependencies, exports, and module structure. Uses search result IDs and committed source. Reports uncertainty and truncation.",
            "inputSchema":{
                "type":"object",
                "properties":{
                    "operation":{"type":"string","enum":["bfs","dfs","callers","callees","call_sites","call_graph","type_graph","usages","ancestors","children","impact","path","symbol_context","dependencies","dependents","exports","modules","qualified","filtered"]},
                    "id":{"type":"integer","minimum":1},
                    "target":{"type":"integer","minimum":1},
                    "path":{"type":"string","minLength":1,"maxLength":1024},
                    "pattern":{"type":"string","maxLength":1024},
                    "max_bytes":{"type":"integer","minimum":1024},
                    "max_edges":{"type":"integer","minimum":0},
                    "include_edges":{"type":"boolean","default":true},
                    "traversal":{
                        "type":"object",
                        "properties":{
                            "max_depth":{"type":"integer","minimum":0},
                            "limit":{"type":"integer","minimum":1},
                            "direction":{"type":"string","enum":["outgoing","incoming","both"],"default":"outgoing"},
                            "include_start":{"type":"boolean","default":true},
                            "edge_kinds":{"type":"array","maxItems":32,"items":{"type":"string","maxLength":64}},
                            "node_kinds":{"type":"array","maxItems":64,"items":{"type":"string","maxLength":64}}
                        },
                        "additionalProperties":false
                    }
                },
                "required":["operation"],
                "additionalProperties":false
            },
            "annotations":{"readOnlyHint":true,"destructiveHint":false,"openWorldHint":false}
        },
        {
            "name": "code_context",
            "description": "Rank task-relevant symbols, expand their relationships, and assemble source blocks from one committed snapshot. Returns canonical IDs, entry points, retrieval confidence, statistics, generation, and explicit truncation.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {"type":"string", "minLength":1, "maxLength":10000},
                    "options": {"type":"object", "additionalProperties":false, "properties": {
                        "maxNodes":{"type":"integer","minimum":1,"default":20},
                        "maxCodeBlocks":{"type":"integer","minimum":0,"default":5},
                        "maxCodeBlockSize":{"type":"integer","minimum":0,"default":1500},
                        "includeCode":{"type":"boolean","default":true},
                        "searchLimit":{"type":"integer","minimum":1,"default":3},
                        "traversalDepth":{"type":"integer","minimum":0,"default":1},
                        "minScore":{"type":"number","minimum":0,"maximum":1,"default":0.3}
                    }},
                    "max_bytes":{"type":"integer","minimum":1024}
                },
                "required":["query"],
                "additionalProperties":false
            },
            "annotations":{"readOnlyHint":true,"destructiveHint":false,"openWorldHint":false}
        },
        {
            "name": "code_search",
            "description": "Search indexed source in this worktree. Search mode matches words and symbol names. Callers/callees modes return static candidates with resolution evidence and ambiguity. Results include bounded source and locations; refine queries when truncated.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {"type": "string", "minLength": 1, "maxLength": 10000},
                    "mode": {"type": "string", "enum": ["search", "callers", "callees"], "default": "search"},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 100, "default": 10},
                    "max_bytes": {"type": "integer", "minimum": 1024}
                },
                "required": ["query"],
                "additionalProperties": false
            },
            "annotations": {"readOnlyHint": true, "destructiveHint": false, "openWorldHint": false}
        },
        {
            "name": "code_explore",
            "description": "Assemble relevant indexed source grouped by file, static relationships, paths, and impact within a byte budget. Use mode impact for reverse dependencies, path with target for a directed connection, implementations for type hierarchies, or affected with a file query for candidate tests. Resolution is heuristic; ambiguity and truncated analysis are explicit.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {"type": "string", "minLength": 1, "maxLength": 10000},
                    "mode": {"type": "string", "enum": ["explore", "impact", "path", "implementations", "affected"], "default": "explore"},
                    "path": {"type": "string", "minLength": 1, "maxLength": 4096, "description": "Exact worktree-relative file for symbol selection."},
                    "line": {"type": "integer", "minimum": 1, "description": "Select the symbol containing this line; requires path."},
                    "target": {"type": "string", "minLength": 1, "maxLength": 10000, "description": "Destination symbol; required only for path mode."},
                    "depth": {"type": "integer", "minimum": 1, "default": 2},
                    "limit": {"type": "integer", "minimum": 1},
                    "max_bytes": {"type": "integer", "minimum": 1024}
                },
                "required": ["query"],
                "additionalProperties": false
            },
            "annotations": {"readOnlyHint": true, "destructiveHint": false, "openWorldHint": false}
        },
        {
            "name": "code_files",
            "description": "List files indexed in this worktree, optionally filtering paths. Returns language, symbol counts, syntax status, and snapshot generation within a byte budget.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {"type": "string", "maxLength": 10000, "default": ""},
                    "limit": {"type": "integer", "minimum": 1},
                    "max_bytes": {"type": "integer", "minimum": 1024}
                },
                "additionalProperties": false
            },
            "annotations": {"readOnlyHint": true, "destructiveHint": false, "openWorldHint": false}
        },
        {
            "name": "code_hierarchy",
            "description": "Read a type's ancestors, subtypes, synthesized inheritance, and name-based member override matches using its search result ID. Reports dispatch fan size, uncertainty, and explicit truncation.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": {"type": "integer", "minimum": 1},
                    "max_bytes": {"type": "integer", "minimum": 1024}
                },
                "required": ["id"],
                "additionalProperties": false
            },
            "annotations": {"readOnlyHint": true, "destructiveHint": false, "openWorldHint": false}
        },
        {
            "name": "code_node",
            "description": "Read indexed source and extraction metadata for a search result ID. IDs may change after reindexing. Source and metadata truncation are explicit.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": {"type": "integer", "minimum": 1},
                    "max_bytes": {"type": "integer", "minimum": 1024}
                },
                "required": ["id"],
                "additionalProperties": false
            },
            "annotations": {"readOnlyHint": true, "destructiveHint": false, "openWorldHint": false}
        },
        {
            "name": "code_file",
            "description": "Read inclusive lines from an indexed file in this worktree. Source comes from the committed index snapshot. Verify live source before editing.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": {"type": "string", "minLength": 1, "maxLength": 4096},
                    "start_line": {"type": "integer", "minimum": 1, "default": 1},
                    "end_line": {"type": "integer", "minimum": 1},
                    "max_bytes": {"type": "integer", "minimum": 1024}
                },
                "required": ["path"],
                "additionalProperties": false
            },
            "annotations": {"readOnlyHint": true, "destructiveHint": false, "openWorldHint": false}
        },
        {
            "name": "code_search_status",
            "description": "Reconcile files and report the committed worktree index state.",
            "inputSchema": {"type": "object", "properties": {}, "additionalProperties": false},
            "annotations": {"readOnlyHint": true, "destructiveHint": false, "openWorldHint": false}
        }
    ]})
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Call {
    name: String,
    #[serde(default = "empty_object")]
    arguments: Value,
    #[serde(rename = "_meta")]
    _meta: Option<Value>,
}

fn empty_object() -> Value {
    json!({})
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Mode {
    #[default]
    Search,
    Callers,
    Callees,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Search {
    query: String,
    #[serde(default)]
    mode: Mode,
    #[serde(default = "default_limit")]
    limit: usize,
    #[serde(default = "default_max_bytes")]
    max_bytes: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Context {
    query: String,
    #[serde(default)]
    options: BuildContextOptions,
    #[serde(default = "default_max_bytes")]
    max_bytes: usize,
}

fn default_limit() -> usize {
    10
}
fn default_max_bytes() -> usize {
    usize::MAX
}

#[derive(Default, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
enum ExploreMode {
    #[default]
    Explore,
    Impact,
    Path,
    Implementations,
    Affected,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Explore {
    query: String,
    #[serde(default)]
    mode: ExploreMode,
    path: Option<String>,
    line: Option<usize>,
    target: Option<String>,
    #[serde(default = "default_depth")]
    depth: usize,
    #[serde(default = "default_limit")]
    limit: usize,
    #[serde(default = "default_max_bytes")]
    max_bytes: usize,
}

fn default_depth() -> usize {
    2
}

enum Request {
    Graph(GraphQueryOptions),
    Context(Context),
    Search(Search),
    Explore(Explore),
    Files(Files),
    Node(NodeRead),
    Hierarchy(NodeRead),
    File(FileRead),
    Status,
}

impl Request {
    fn byte_budget_mut(&mut self) -> Option<&mut usize> {
        Some(match self {
            Self::Graph(query) => &mut query.max_bytes,
            Self::Context(query) => &mut query.max_bytes,
            Self::Search(query) => &mut query.max_bytes,
            Self::Explore(query) => &mut query.max_bytes,
            Self::Files(query) => &mut query.max_bytes,
            Self::Node(query) | Self::Hierarchy(query) => &mut query.max_bytes,
            Self::File(query) => &mut query.max_bytes,
            Self::Status => return None,
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Files {
    #[serde(default)]
    query: String,
    #[serde(default = "default_max_bytes")]
    limit: usize,
    #[serde(default = "default_max_bytes")]
    max_bytes: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NodeRead {
    id: i64,
    #[serde(default = "default_max_bytes")]
    max_bytes: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileRead {
    path: String,
    #[serde(default = "first_line")]
    start_line: usize,
    end_line: Option<usize>,
    #[serde(default = "default_max_bytes")]
    max_bytes: usize,
}

fn first_line() -> usize {
    1
}

fn validate_query(query: &str, limit: usize, max_bytes: usize) -> std::result::Result<(), String> {
    if query.trim().is_empty() || query.encode_utf16().count() > 10_000 {
        return Err("query must contain 1 to 10000 UTF-16 code units".into());
    }
    validate_budget(limit, max_bytes)
}

fn validate_budget(limit: usize, max_bytes: usize) -> std::result::Result<(), String> {
    if limit == 0 || max_bytes < 1024 {
        return Err("limit must be positive and max_bytes must be at least 1024".into());
    }
    Ok(())
}

fn tool_call(engine: &mut Engine, params: Value) -> std::result::Result<Value, String> {
    let call: Call = serde_json::from_value(params).map_err(|error| error.to_string())?;
    let mut request = match call.name.as_str() {
        "code_graph" => {
            if call.arguments.get("operation").is_none() {
                return Err("operation is required".into());
            }
            let query: GraphQueryOptions =
                serde_json::from_value(call.arguments).map_err(|error| error.to_string())?;
            query.validate().map_err(|error| error.to_string())?;
            Request::Graph(query)
        }
        "code_context" => {
            let query: Context =
                serde_json::from_value(call.arguments).map_err(|error| error.to_string())?;
            if query.query.trim().is_empty() || query.query.encode_utf16().count() > 10_000 {
                return Err("query must contain 1 to 10000 UTF-16 code units".into());
            }
            validate_budget(1, query.max_bytes)?;
            let options = &query.options;
            if options.max_nodes == 0
                || options.search_limit == 0
                || !options.min_score.is_finite()
                || !(0.0..=1.0).contains(&options.min_score)
            {
                return Err("context options exceed the supported bounds".into());
            }
            Request::Context(query)
        }
        "code_search" => {
            let explicit_limit = call
                .arguments
                .get("limit")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let mut query: Search =
                serde_json::from_value(call.arguments).map_err(|error| error.to_string())?;
            query.limit = if explicit_limit == 0 {
                if matches!(query.mode, Mode::Search) {
                    10
                } else {
                    20
                }
            } else {
                explicit_limit.clamp(1, 100) as usize
            };
            validate_query(&query.query, query.limit, query.max_bytes)?;
            Request::Search(query)
        }
        "code_explore" => {
            let explicit_limit = call.arguments.get("limit").is_some();
            let mut query: Explore =
                serde_json::from_value(call.arguments).map_err(|error| error.to_string())?;
            if !explicit_limit {
                query.limit = if query.mode == ExploreMode::Impact {
                    10_000
                } else {
                    200
                };
            }
            validate_query(&query.query, query.limit, query.max_bytes)?;
            if query.mode == ExploreMode::Impact {
                query.depth = query.depth.clamp(1, 10);
            }
            if query.depth == 0
                || query.line == Some(0)
                || (query.line.is_some() && query.path.is_none())
            {
                return Err(
                    "depth must be positive; line must be positive and requires path".into(),
                );
            }
            if (query.mode == ExploreMode::Path) != query.target.is_some() {
                return Err("target is required only for path mode".into());
            }
            if let Some(target) = &query.target {
                validate_query(target, query.limit, query.max_bytes)?;
            }
            if let Some(path) = &query.path
                && (path.is_empty()
                    || path.encode_utf16().count() > 4096
                    || path.contains('\\')
                    || !std::path::Path::new(path).components().all(|component| {
                        matches!(
                            component,
                            std::path::Component::Normal(_) | std::path::Component::CurDir
                        )
                    }))
            {
                return Err(
                    "path must be a worktree-relative file without parent traversal".into(),
                );
            }
            Request::Explore(query)
        }
        "code_files" => {
            let query: Files =
                serde_json::from_value(call.arguments).map_err(|error| error.to_string())?;
            validate_budget(query.limit, query.max_bytes)?;
            if query.query.encode_utf16().count() > 10_000 {
                return Err("query must be at most 10000 UTF-16 code units".into());
            }
            Request::Files(query)
        }
        "code_node" | "code_hierarchy" => {
            let query: NodeRead =
                serde_json::from_value(call.arguments).map_err(|error| error.to_string())?;
            validate_budget(1, query.max_bytes)?;
            if query.id <= 0 {
                return Err("id must be positive".into());
            }
            if call.name == "code_hierarchy" {
                Request::Hierarchy(query)
            } else {
                Request::Node(query)
            }
        }
        "code_file" => {
            let query: FileRead =
                serde_json::from_value(call.arguments).map_err(|error| error.to_string())?;
            validate_budget(1, query.max_bytes)?;
            if query.path.is_empty()
                || query.path.encode_utf16().count() > 4096
                || query.path.contains('\\')
                || !std::path::Path::new(&query.path).components().all(|part| {
                    matches!(
                        part,
                        std::path::Component::Normal(_) | std::path::Component::CurDir
                    )
                })
            {
                return Err(
                    "path must be a worktree-relative file without parent traversal".into(),
                );
            }
            if query.start_line == 0 || query.end_line.is_some_and(|end| end < query.start_line) {
                return Err("line range must be positive and ordered".into());
            }
            Request::File(query)
        }
        "code_search_status" => {
            if !call
                .arguments
                .as_object()
                .is_some_and(|args| args.is_empty())
            {
                return Err("code_search_status accepts only an empty arguments object".into());
            }
            Request::Status
        }
        _ => return Err("Unknown tool".into()),
    };
    let char_limit = match &request {
        Request::File(_) => 38_000,
        Request::Explore(query) if query.mode == ExploreMode::Explore => 25_000,
        Request::Context(_) => 25_000,
        _ => 15_000,
    };
    if let Some(budget) = request.byte_budget_mut() {
        *budget = (*budget).min(char_limit * 3);
    }
    let result: Result<Value> = (|| {
        engine.refresh()?;
        loop {
            let value = execute_request(engine, &request, char_limit)?;
            let text = serde_json::to_string(&value)?;
            let chars = text.encode_utf16().count();
            if chars <= char_limit {
                return Ok(value);
            }
            let budget = request
                .byte_budget_mut()
                .ok_or_else(|| anyhow::anyhow!("response exceeds character budget"))?;
            anyhow::ensure!(*budget > 1024, "response metadata exceeds character budget");
            *budget = (*budget - 1)
                .min(text.len().saturating_sub(chars - char_limit))
                .max(1024);
        }
    })();
    Ok(match result {
        Ok(value) => {
            json!({"content": [{"type": "text", "text": serde_json::to_string(&value).unwrap()}], "isError": false})
        }
        Err(error) => {
            let detail: String = format!("{error:#}").chars().take(2048).collect();
            json!({"content": [{"type": "text", "text": detail}], "isError": true})
        }
    })
}

fn execute_request(engine: &Engine, request: &Request, char_limit: usize) -> Result<Value> {
    match request {
        Request::Graph(query) => {
            let mut result = engine.graph_query(query)?;
            result.limit_utf16(char_limit)?;
            Ok(serde_json::to_value(result)?)
        }
        Request::Context(query) => Ok(serde_json::to_value(engine.context(
            &TaskInput::Query(query.query.clone()),
            &query.options,
            query.max_bytes,
        )?)?),
        Request::Search(search) => match search.mode {
            Mode::Search => Ok(serde_json::to_value(engine.search(
                &search.query,
                search.limit,
                search.max_bytes,
            )?)?),
            Mode::Callers => Ok(serde_json::to_value(engine.related(
                &search.query,
                Direction::Callers,
                search.limit,
                search.max_bytes,
            )?)?),
            Mode::Callees => Ok(serde_json::to_value(engine.related(
                &search.query,
                Direction::Callees,
                search.limit,
                search.max_bytes,
            )?)?),
        },
        Request::Explore(query) => {
            let options = GraphOptions {
                path: query.path.clone(),
                line: query.line,
                target: query.target.clone(),
                depth: query.depth,
                limit: query.limit,
                max_bytes: query.max_bytes,
            };
            let mode = match query.mode {
                ExploreMode::Explore => None,
                ExploreMode::Impact => Some(GraphMode::Impact),
                ExploreMode::Path => Some(GraphMode::Path),
                ExploreMode::Implementations => Some(GraphMode::Implementations),
                ExploreMode::Affected => Some(GraphMode::Affected),
            };
            Ok(serde_json::to_value(match mode {
                Some(mode) => engine.analyze(&query.query, mode, &options)?,
                None => engine.explore(&query.query, &options)?,
            })?)
        }
        Request::Status => Ok(serde_json::to_value(engine.status()?)?),
        Request::Files(query) => Ok(serde_json::to_value(engine.files(
            &query.query,
            query.limit,
            query.max_bytes,
        )?)?),
        Request::Node(query) => Ok(serde_json::to_value(
            engine.node(query.id, query.max_bytes)?,
        )?),
        Request::Hierarchy(query) => Ok(serde_json::to_value(
            engine.hierarchy(query.id, query.max_bytes)?,
        )?),
        Request::File(query) => Ok(serde_json::to_value(
            engine.file(
                &query.path,
                query.start_line,
                Some(
                    query
                        .end_line
                        .unwrap_or_else(|| query.start_line.saturating_add(1999)),
                ),
                query.max_bytes,
            )?,
        )?),
    }
}

fn success(id: Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}
fn error(id: Value, code: i32, message: &str) -> Value {
    let message: String = message.chars().take(2048).collect();
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}
