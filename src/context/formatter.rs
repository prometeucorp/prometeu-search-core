use super::*;
use serde_json::{Map, Value};
pub(super) fn markdown(context: &TaskContext, generated: &PathPredicate<'_>) -> String {
    let mut lines = vec![
        "## Code Context\n".to_string(),
        format!("**Query:** {}\n", context.query),
    ];
    let mut entries: Vec<_> = context.entry_points.iter().collect();
    entries.sort_by_key(|node| generated(&node.file_path));
    if !entries.is_empty() {
        lines.push("### Entry Points\n".to_string());
        for node in entries {
            let location = if node.start_line > 0 {
                format!(":{}", node.start_line)
            } else {
                String::new()
            };
            lines.push(format!(
                "- **{}** ({}) - {}{location}",
                node.name, node.kind, node.file_path
            ));
            if let Some(signature) = node.signature.as_deref().filter(|s| !s.is_empty()) {
                lines.push(format!("  `{signature}`"));
            }
        }
        lines.push(String::new());
    }
    let others: Vec<_> = context
        .subgraph
        .nodes
        .iter()
        .filter(|node| {
            !context.entry_points.iter().any(|entry| entry.id == node.id)
                && !generated(&node.file_path)
        })
        .take(10)
        .collect();
    if !others.is_empty() {
        lines.push("### Related Symbols\n".to_string());
        let mut files: Vec<(&str, Vec<&Node>)> = Vec::new();
        for node in others {
            if let Some((_, nodes)) = files.iter_mut().find(|(path, _)| *path == node.file_path) {
                nodes.push(node);
            } else {
                files.push((&node.file_path, vec![node]));
            }
        }
        for (path, nodes) in files {
            lines.push(format!(
                "- {path}: {}",
                nodes
                    .iter()
                    .map(|n| format!("{}:{}", n.name, n.start_line))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        lines.push(String::new());
    }
    if !context.code_blocks.is_empty() {
        let mut blocks: Vec<_> = context.code_blocks.iter().collect();
        blocks.sort_by_key(|block| generated(&block.file_path));
        lines.push("### Code\n".to_string());
        for block in blocks {
            lines.push(format!(
                "#### {} ({}:{})\n",
                block.node.name, block.file_path, block.start_line
            ));
            lines.push(format!("```{}", block.language));
            lines.push(block.content.clone());
            lines.push("```\n".to_string());
        }
    }
    lines.join("\n")
}
fn selected(value: Value, keys: &[&str]) -> Value {
    let mut out = Map::new();
    for key in keys {
        if let Some(value) = value.get(key) {
            out.insert((*key).to_string(), value.clone());
        }
    }
    Value::Object(out)
}
fn node_value(node: &Node) -> Result<Value> {
    let mut value = serde_json::to_value(node)?;
    if value.get("visibility").is_none() {
        value["visibility"] = Value::Null;
    }
    Ok(selected(
        value,
        &[
            "id",
            "kind",
            "name",
            "qualifiedName",
            "filePath",
            "language",
            "startLine",
            "endLine",
            "signature",
            "docstring",
            "visibility",
            "isExported",
            "isAsync",
            "isStatic",
        ],
    ))
}

pub fn format_context_as_json(context: &TaskContext) -> Result<String> {
    let mut value = Map::new();
    value.insert("query".to_string(), Value::String(context.query.clone()));
    value.insert(
        "summary".to_string(),
        Value::String(context.summary.clone()),
    );
    value.insert(
        "entryPoints".to_string(),
        Value::Array(
            context
                .entry_points
                .iter()
                .map(node_value)
                .collect::<Result<_>>()?,
        ),
    );
    value.insert(
        "nodes".to_string(),
        Value::Array(
            context
                .subgraph
                .nodes
                .iter()
                .map(node_value)
                .collect::<Result<_>>()?,
        ),
    );
    value.insert(
        "edges".to_string(),
        Value::Array(
            context
                .subgraph
                .edges
                .iter()
                .map(|edge| {
                    Ok(selected(
                        serde_json::to_value(edge)?,
                        &["source", "target", "kind", "line", "column"],
                    ))
                })
                .collect::<Result<_>>()?,
        ),
    );
    let mut blocks = Vec::new();
    for block in &context.code_blocks {
        let mut value = selected(
            serde_json::to_value(block)?,
            &["filePath", "startLine", "endLine", "language", "content"],
        );
        let object = value.as_object_mut().unwrap();
        object.insert("nodeName".to_string(), block.node.name.clone().into());
        object.insert("nodeKind".to_string(), block.node.kind.clone().into());
        blocks.push(value);
    }
    value.insert("codeBlocks".to_string(), Value::Array(blocks));
    value.insert(
        "relatedFiles".to_string(),
        serde_json::to_value(&context.related_files)?,
    );
    value.insert("stats".to_string(), serde_json::to_value(&context.stats)?);
    Ok(serde_json::to_string_pretty(&value)?)
}
pub fn format_bytes(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{bytes} bytes")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}
pub fn format_subgraph_tree(graph: &ContextSubgraph, entries: &[Node]) -> String {
    let mut lines = Vec::new();
    let mut printed = HashSet::new();
    for entry in entries {
        tree(entry, graph, &mut printed, &mut lines, 0, "");
        lines.push(String::new());
    }
    let remaining: Vec<_> = graph
        .nodes
        .iter()
        .filter(|node| !printed.contains(node.id.as_str()))
        .collect();
    if !remaining.is_empty() && remaining.len() <= 10 {
        lines.push("Other relevant symbols:".to_string());
        for node in remaining {
            let location = if node.start_line > 0 {
                format!(":{}", node.start_line)
            } else {
                String::new()
            };
            lines.push(format!(
                "  {}: {} ({}{location})",
                node.kind, node.name, node.file_path
            ));
        }
    } else if remaining.len() > 10 {
        lines.push(format!("... and {} more related symbols", remaining.len()));
    }
    lines
        .join("\n")
        .trim_matches(crate::search::query_whitespace)
        .to_string()
}
fn tree<'a>(
    node: &'a Node,
    graph: &'a ContextSubgraph,
    printed: &mut HashSet<&'a str>,
    lines: &mut Vec<String>,
    depth: usize,
    prefix: &str,
) {
    if !printed.insert(&node.id) {
        return;
    }
    let location = if node.start_line > 0 {
        format!(":{}", node.start_line)
    } else {
        String::new()
    };
    let signature = node
        .signature
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(|s| {
            format!(
                " - {}",
                if utf16_len(s) > 50 {
                    utf16_prefix(s, 47) + "..."
                } else {
                    s.to_string()
                }
            )
        })
        .unwrap_or_default();
    lines.push(format!(
        "{prefix}{}: {} ({}{location}){signature}",
        node.kind, node.name, node.file_path
    ));
    let edges: Vec<_> = graph
        .edges
        .iter()
        .filter(|e| {
            e.source == node.id
                && matches!(
                    e.kind.as_str(),
                    "calls" | "extends" | "implements" | "imports" | "references"
                )
        })
        .collect();
    let mut groups: Vec<(&str, Vec<&Edge>)> = Vec::new();
    for edge in &edges {
        if let Some((_, edges)) = groups.iter_mut().find(|(kind, _)| *kind == edge.kind) {
            edges.push(edge);
        } else {
            groups.push((&edge.kind, vec![edge]));
        }
    }
    let next_prefix = format!("{prefix}  ");
    let name = |edge: &Edge| {
        graph
            .nodes
            .iter()
            .find(|n| n.id == edge.target)
            .map(|n| n.name.as_str())
            .unwrap_or("unknown")
    };
    for (kind, edges) in groups {
        if edges.len() > 3 {
            lines.push(format!(
                "{next_prefix}├── {kind}: {} and {} more",
                edges
                    .iter()
                    .take(3)
                    .map(|e| name(e))
                    .collect::<Vec<_>>()
                    .join(", "),
                edges.len() - 3
            ));
        } else {
            for (i, edge) in edges.iter().enumerate() {
                lines.push(format!(
                    "{next_prefix}{} {kind} → {}",
                    if i + 1 == edges.len() {
                        "└──"
                    } else {
                        "├──"
                    },
                    name(edge)
                ));
            }
        }
    }
    if depth < 1 {
        for edge in edges.into_iter().take(3) {
            if let Some(target) = graph.nodes.iter().find(|n| n.id == edge.target) {
                tree(target, graph, printed, lines, depth + 1, &next_prefix);
            }
        }
    }
}
pub(super) fn low_confidence(entries: &[Node]) -> String {
    let mut dirs = Vec::new();
    for node in entries {
        let dir = node
            .file_path
            .rfind('/')
            .filter(|i| *i > 0)
            .map(|i| &node.file_path[..i])
            .unwrap_or(&node.file_path);
        if !dirs.contains(&dir) {
            dirs.push(dir);
        }
        if dirs.len() == 4 {
            break;
        }
    }
    let tail = if dirs.is_empty() {
        String::new()
    } else {
        format!(
            "\n- `code_files` a likely area: {}",
            dirs.iter()
                .map(|dir| format!("`{dir}`"))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    format!(
        "\n\n{LOW_CONFIDENCE_MARKER}\n\nThis query matched mostly on common words, so the entry points above may be off-target — treat them as a starting point, not a complete answer. For a reliable result:\n- `code_explore` with the **exact symbol names** you are after (class / function / method names), or\n- `code_search <name>` for one specific symbol{tail}\n\nDo not assume the list above is comprehensive."
    )
}
pub(super) fn call_paths(graph: &ContextSubgraph) -> String {
    let ids: HashSet<_> = graph.nodes.iter().map(|n| n.id.as_str()).collect();
    let mut adjacent: Vec<(&str, Vec<&str>)> = Vec::new();
    for edge in &graph.edges {
        if edge.kind != "calls"
            || !ids.contains(edge.source.as_str())
            || !ids.contains(edge.target.as_str())
        {
            continue;
        }
        if let Some((_, targets)) = adjacent.iter_mut().find(|(id, _)| *id == edge.source) {
            targets.push(&edge.target);
        } else {
            adjacent.push((&edge.source, vec![&edge.target]));
        }
    }
    if adjacent.is_empty() {
        return String::new();
    }
    fn dfs<'a>(
        id: &'a str,
        path: &mut Vec<&'a str>,
        adjacent: &[(&'a str, Vec<&'a str>)],
        budget: &mut usize,
        chains: &mut Vec<Vec<&'a str>>,
    ) {
        if *budget == 0 {
            return;
        }
        *budget -= 1;
        let next: Vec<_> = adjacent
            .iter()
            .find(|(from, _)| *from == id)
            .into_iter()
            .flat_map(|(_, targets)| targets)
            .copied()
            .filter(|target| !path.contains(target))
            .collect();
        if next.is_empty() || path.len() >= 6 {
            if path.len() >= 3 {
                chains.push(path.clone());
            }
            return;
        }
        for target in next {
            path.push(target);
            dfs(target, path, adjacent, budget, chains);
            path.pop();
        }
    }
    let starts: Vec<_> = if graph.roots.is_empty() {
        adjacent.iter().map(|(id, _)| *id).take(5).collect()
    } else {
        graph
            .roots
            .iter()
            .map(String::as_str)
            .filter(|id| adjacent.iter().any(|(from, _)| from == id))
            .take(5)
            .collect()
    };
    let mut budget = 2000;
    let mut chains = Vec::new();
    for start in starts {
        dfs(start, &mut vec![start], &adjacent, &mut budget, &mut chains);
    }
    let roots: HashSet<_> = graph.roots.iter().map(String::as_str).collect();
    let count = |chain: &Vec<&str>| chain.iter().filter(|id| roots.contains(**id)).count();
    chains.retain(|chain| count(chain) >= 2);
    chains.sort_by_key(|chain| std::cmp::Reverse((count(chain), chain.len())));
    let mut kept: Vec<Vec<&str>> = Vec::new();
    for chain in chains {
        let key = chain.join(">");
        if kept.iter().any(|kept| kept.join(">").contains(&key)) {
            continue;
        }
        kept.push(chain);
        if kept.len() == 3 {
            break;
        }
    }
    if kept.is_empty() {
        return String::new();
    }
    let mut descriptions = HashMap::new();
    for edge in &graph.edges {
        if edge.kind == "calls"
            && let Some(description) = hop(edge)
        {
            descriptions.insert((edge.source.as_str(), edge.target.as_str()), description);
        }
    }
    let name = |id: &str| {
        graph
            .nodes
            .iter()
            .find(|n| n.id == id)
            .map(|n| n.name.clone())
            .unwrap_or_else(|| id.to_string())
    };
    let mut synthesized = false;
    let mut lines = vec![
        String::new(),
        "## Call paths".to_string(),
        String::new(),
        "Execution flow among the key symbols (traced through the call graph):".to_string(),
        String::new(),
    ];
    for chain in kept {
        let mut line = format!("- {}", name(chain[0]));
        for pair in chain.windows(2) {
            if let Some(description) = descriptions.get(&(pair[0], pair[1])) {
                synthesized = true;
                line.push_str(&format!(" →[{description}] {}", name(pair[1])));
            } else {
                line.push_str(&format!(" → {}", name(pair[1])));
            }
        }
        lines.push(line);
    }
    lines.push(String::new());
    lines.push(if synthesized{"_Hops marked `[…]` are dynamic dispatch bridged by Prometeu (with the registration site); the rest are direct calls. code_node any symbol for its body._"}else{"_code_node any symbol above for its source + its own callers/callees._"}.to_string());
    format!("\n{}\n", lines.join("\n"))
}
fn js(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Null => "null".to_string(),
        Value::Array(values) => values
            .iter()
            .map(|v| if v.is_null() { String::new() } else { js(v) })
            .collect::<Vec<_>>()
            .join(","),
        Value::Object(_) => "[object Object]".to_string(),
        other => other.to_string(),
    }
}
fn truth(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64() != Some(0.0),
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}
pub(super) fn hop(edge: &Edge) -> Option<String> {
    if edge.provenance.as_deref() != Some("heuristic") {
        return None;
    }
    let meta = edge.metadata.as_ref()?;
    let by = meta.get("synthesizedBy")?.as_str()?;
    let value = |key: &str| meta.get(key).unwrap_or(&Value::Null);
    let quoted = |key: &str, fallback: &str| {
        if truth(value(key)) {
            format!("`{}`", js(value(key)))
        } else {
            fallback.to_string()
        }
    };
    let text = |key: &str| {
        if truth(value(key)) {
            js(value(key))
        } else {
            String::new()
        }
    };
    let mut description = match by {
        "callback" => format!("callback via {}", quoted("via", "a registrar")),
        "http-client" => format!(
            "HTTP {} — the client's call onto its own route",
            format!(
                "{} {}",
                meta.get("method")
                    .filter(|v| !v.is_null())
                    .map(js)
                    .unwrap_or_else(|| "GET".to_string()),
                meta.get("href")
                    .filter(|v| !v.is_null())
                    .map(js)
                    .unwrap_or_default()
            )
            .trim()
        ),
        "queue-job" => format!(
            "queue job {}{}",
            quoted("event", "a job"),
            if truth(value("queue")) {
                format!(" on `{}`", text("queue"))
            } else {
                String::new()
            }
        ),
        "event-bus" => format!(
            "{} {}{}",
            if value("channel") == "socket" {
                "socket message"
            } else {
                "bus event"
            },
            quoted("event", "an event"),
            if value("tier") == "client→server" {
                " → server"
            } else if value("tier") == "server→client" {
                " → client"
            } else {
                ""
            }
        ),
        "event-emitter" => format!("event {}", quoted("event", "an event")),
        "tauri-command" => format!(
            "Tauri command {} — the frontend's invoke onto its Rust handler",
            quoted("command", "a command")
        ),
        "tauri-event" => format!(
            "Tauri event {}{}",
            quoted("event", "an event"),
            if value("tier") == "server→client" {
                " → frontend"
            } else if value("tier") == "client→server" {
                " → Rust"
            } else {
                ""
            }
        ),
        "react-render" => "React re-render via setState".to_string(),
        "jsx-render" => format!(
            "renders {}",
            if truth(value("via")) {
                format!("<{}>", text("via"))
            } else {
                "a child component".to_string()
            }
        ),
        "vue-handler" => format!(
            "Vue {} handler",
            if truth(value("event")) {
                format!("@{}", text("event"))
            } else {
                "a template event".to_string()
            }
        ),
        "interface-impl" => value("promotedInto")
            .as_str()
            .map(|s| format!("interface → method promoted into {s}"))
            .unwrap_or_else(|| "interface → impl".to_string()),
        "closure-collection" => format!("runs {} handlers", quoted("field", "a collection")),
        "fn-pointer-dispatch" => format!("fn-pointer {}", text("via")),
        "goframe-route" => format!("GoFrame route {}", text("route")),
        "next-proxy" => {
            if truth(value("matcher")) {
                format!("Next proxy, matcher {}", quoted("matcher", ""))
            } else {
                "Next proxy, on every request".to_string()
            }
        }
        "const-alias" => format!("const alias of {}", quoted("via", "a function")),
        "go-func-field" => format!(
            "func field {}{}",
            quoted("field", "a func field"),
            if truth(value("via")) {
                format!(" (set from `{}`)", text("via"))
            } else {
                String::new()
            }
        ),
        _ => by.replace('-', " "),
    };
    if let Some(at) = value("registeredAt").as_str().filter(|s| !s.is_empty()) {
        description.push_str(&format!(" @{at}"));
    }
    Some(description)
}
