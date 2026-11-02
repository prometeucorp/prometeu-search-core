//! Embedded, loopback-only graph viewer over committed index snapshots.
use super::{Graph, with_graph};
use crate::Engine;
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};
use std::path::{Component, Path};

#[path = "viewer/branches.rs"]
mod branches;
#[path = "viewer/effects.rs"]
mod effects;
#[path = "viewer/http.rs"]
mod http;
#[path = "viewer/program.rs"]
mod program;
#[path = "viewer/views.rs"]
mod views;
pub use http::{ViewerServer, serve, serve_with_options};

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ViewerOptions {
    pub root: String,
    pub query: String,
    pub kind: Option<String>,
    pub language: Option<String>,
    pub focus: Option<String>,
    pub depth: usize,
    pub limit: usize,
    /// Optional host budget; `usize::MAX` (the default) leaves bytes unrestricted.
    pub max_bytes: usize,
}

impl Default for ViewerOptions {
    fn default() -> Self {
        Self {
            root: String::new(),
            query: String::new(),
            kind: None,
            language: None,
            focus: None,
            depth: 2,
            limit: 500,
            max_bytes: usize::MAX,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewerGraph {
    pub generation: u64,
    pub nodes: Vec<Value>,
    pub edges: Vec<Value>,
    pub total_nodes: usize,
    pub total_edges: usize,
    pub truncated: bool,
}

impl Engine {
    /// Export a bounded graph for a host renderer. Reads only committed data.
    pub fn viewer_graph(&self, options: &ViewerOptions) -> Result<ViewerGraph> {
        ensure!(options.limit > 0, "limit must be positive");
        ensure!(options.depth > 0, "depth must be positive");
        ensure!(options.max_bytes >= 1024, "invalid response budget");
        validate_path(&options.root)?;
        ensure!(options.query.len() <= 1024, "query exceeds 1024 bytes");
        with_graph(self, |graph, _| {
            let needle = options.query.to_lowercase();
            let reached = options
                .focus
                .as_deref()
                .map(|id| {
                    let index = lookup(graph, id)?;
                    Ok::<_, anyhow::Error>(walk(graph, index, options.depth, true, usize::MAX).0)
                })
                .transpose()?;
            let candidates = graph
                .nodes
                .iter()
                .enumerate()
                .filter(|(i, node)| {
                    within(&node.path, &options.root)
                        && options
                            .kind
                            .as_ref()
                            .is_none_or(|kind| node.symbol.kind == *kind)
                        && options.language.as_ref().is_none_or(|language| {
                            graph.files[&node.path].analysis.language == *language
                        })
                        && (needle.is_empty()
                            || node.symbol.qualified_name.to_lowercase().contains(&needle)
                            || node.path.to_lowercase().contains(&needle))
                        && reached.as_ref().is_none_or(|indices| indices.contains(i))
                })
                .map(|(i, _)| i)
                .collect::<Vec<_>>();
            let total_nodes = candidates.len();
            let mut selected = candidates
                .into_iter()
                .take(options.limit)
                .collect::<HashSet<_>>();
            let mut result = ViewerGraph {
                generation: graph.generation,
                nodes: vec![],
                edges: vec![],
                total_nodes,
                total_edges: 0,
                truncated: graph.truncated
                    || graph.resolution_truncated.get()
                    || total_nodes > selected.len(),
            };
            let mut ordered = selected.iter().copied().collect::<Vec<_>>();
            ordered.sort_unstable();
            let mut bytes = 256;
            for index in ordered {
                let node = node_ref(graph, index);
                let size = serde_json::to_vec(&node)?.len() + 1;
                if bytes + size > options.max_bytes {
                    selected.remove(&index);
                    result.truncated = true;
                    continue;
                }
                bytes += size;
                result.nodes.push(node);
            }
            for (index, edge) in graph.edges.iter().enumerate() {
                if selected.contains(&edge.from) && edge.to.is_none_or(|to| selected.contains(&to))
                {
                    result.total_edges += 1;
                    let value = edge_ref(graph, index);
                    let size = serde_json::to_vec(&value)?.len() + 1;
                    if bytes + size > options.max_bytes {
                        result.truncated = true;
                        continue;
                    }
                    bytes += size;
                    result.edges.push(value);
                }
            }
            Ok(result)
        })
    }

    /// Read a viewer endpoint without starting a server. Paths and query values
    /// use URL encoding, exactly as they do over HTTP. No implicit refresh.
    pub fn viewer_query(&self, target: &str) -> Result<Value> {
        let (path, query) = parse_target(target)?;
        if path == "/api/deadcode" {
            let options = crate::DeadCodeOptions {
                limit: integer(&query, "limit", 300, 1, 300)?,
                max_bytes: usize::MAX,
                kinds: argument(&query, "kinds")
                    .unwrap_or("")
                    .split(',')
                    .filter(|x| !x.is_empty())
                    .map(str::to_owned)
                    .collect(),
                include_exported: argument(&query, "exported") == Some("true"),
                include_tests: argument(&query, "tests") == Some("true"),
                include_generated: argument(&query, "generated") == Some("true"),
            };
            return Ok(serde_json::to_value(self.dead_code(&options)?)?);
        }
        if path == "/api/graph" {
            return Ok(serde_json::to_value(self.viewer_graph(
                &ViewerOptions {
                    root: argument(&query, "root").unwrap_or("").into(),
                    query: argument(&query, "q").unwrap_or("").into(),
                    kind: argument(&query, "kind").map(str::to_owned),
                    language: argument(&query, "language").map(str::to_owned),
                    focus: argument(&query, "focus").map(str::to_owned),
                    depth: integer(&query, "depth", 2, 1, usize::MAX)?,
                    limit: integer(&query, "limit", 500, 1, usize::MAX)?,
                    ..ViewerOptions::default()
                },
            )?)?);
        }
        let lexical = if path == "/api/search" {
            let raw = argument(&query, "q").context("q is required")?;
            if raw.trim().is_empty() {
                None
            } else {
                Some(self.search(raw, 400, usize::MAX)?)
            }
        } else {
            None
        };
        with_graph(self, |graph, _| {
            ensure!(
                lexical
                    .as_ref()
                    .is_none_or(|result| result.generation == graph.generation),
                "index changed during search; retry the request"
            );
            let mut value = match path.as_str() {
                "/api" => {
                    json!({"name":"Prometeu Search viewer","endpoints":["stats","search","node/<id>","nodes","source","file/<path>","filecode/<path>","graph","map","routes","entrypoints","screens","steps","flow","deadcode","events","trails"]})
                }
                "/api/stats" => stats(graph, self),
                "/api/search" => search(
                    graph,
                    argument(&query, "q").unwrap_or(""),
                    lexical.as_ref(),
                    integer(&query, "limit", 60, 1, 200)?,
                ),
                "/api/nodes" => {
                    let ids = query
                        .iter()
                        .filter(|(key, _)| key == "id")
                        .map(|(_, value)| value)
                        .collect::<Vec<_>>();
                    ensure!(ids.len() <= 60, "too many node IDs; at most 60 per request");
                    let mut missing = vec![];
                    let nodes = ids
                        .into_iter()
                        .filter_map(|id| match lookup(graph, id) {
                            Ok(i) => Some(node_ref(graph, i)),
                            Err(_) => {
                                missing.push(id);
                                None
                            }
                        })
                        .collect::<Vec<_>>();
                    json!({"nodes": nodes, "missing":missing})
                }
                "/api/source" => source(
                    graph,
                    argument(&query, "file").context("file is required")?,
                    integer(&query, "from", 1, 1, usize::MAX)?,
                    integer(&query, "to", 0, 0, usize::MAX)?,
                )?,
                "/api/routes" => views::routes(graph, integer(&query, "limit", 200, 3, 500)?),
                "/api/entrypoints" => views::entrypoints(
                    graph,
                    integer(&query, "limit", 12, 1, 50)?,
                    integer(&query, "routes", 60, 3, 300)?,
                ),
                "/api/map" => views::map(
                    graph,
                    argument(&query, "root").unwrap_or(""),
                    integer(&query, "depth", 1, 1, 4)?,
                )?,
                "/api/screens" => views::screens(graph),
                "/api/steps" => {
                    views::steps(graph, &query, integer(&query, "limit", 120, 20, 400)?)?
                }
                "/api/flow" => views::flow(graph, &query)?,
                _ if path.starts_with("/api/node/") => node(graph, lookup(graph, &path[10..])?)?,
                _ if path.starts_with("/api/filecode/") => file(graph, &path[14..], true)?,
                _ if path.starts_with("/api/file/") => file(graph, &path[10..], false)?,
                _ => bail!("unknown viewer endpoint"),
            };
            if let Some(object) = value.as_object_mut() {
                object.insert("generation".into(), json!(graph.generation));
            }
            Ok(value)
        })
    }
}

fn canonical(graph: &Graph, index: usize) -> Option<&prometeu_extract::Node> {
    let node = &graph.nodes[index];
    graph.files[&node.path]
        .analysis
        .extraction
        .as_ref()?
        .nodes
        .get(node.ordinal)
}

fn node_id(graph: &Graph, index: usize) -> String {
    canonical(graph, index)
        .map(|node| node.id.clone())
        .unwrap_or_else(|| format!("projected:{}", graph.nodes[index].id))
}

fn node_ref(graph: &Graph, index: usize) -> Value {
    let node = &graph.nodes[index];
    let raw = canonical(graph, index);
    json!({
        "id":node_id(graph,index), "indexId":node.id, "kind":node.symbol.kind,
        "name":node.symbol.name,"qualifiedName":node.symbol.qualified_name,
        "file":node.path,"line":node.symbol.start_line,"endLine":node.symbol.end_line,
        "language":graph.files[&node.path].analysis.language,"test":crate::classify::is_test_file(&node.path),
        "signature":raw.and_then(|node|node.signature.as_deref()),
        "exported":raw.and_then(|node|node.is_exported).unwrap_or(false),
    })
}

fn edge_ref(graph: &Graph, index: usize) -> Value {
    let edge = &graph.edges[index];
    let raw = graph.edge(index);
    let metadata = raw.metadata.as_ref();
    let site = if matches!(
        edge.kind.as_str(),
        "calls" | "references" | "instantiates" | "navigates"
    ) {
        branches::for_edge(graph, index)
    } else {
        branches::Site::default()
    };
    let call_text = site
        .call
        .as_ref()
        .and_then(|call| call["callee"].as_str())
        .unwrap_or(&edge.target);
    let args = site.call.as_ref().and_then(|call| call["args"].as_str());
    let effect = effects::classify(&effects::Input {
        text: call_text.into(),
        kind: edge.kind.clone(),
        language: Some(
            graph.files[&graph.nodes[edge.from].path]
                .analysis
                .language
                .clone(),
        ),
        project: Some(project_kind(graph).into()),
        args: args.map(str::to_owned),
        receiver_type: site.receiver_type.clone(),
        project_type: edge.to.is_some(),
    });
    let status = effect
        .as_ref()
        .filter(|effect| effect.category == "response")
        .and_then(|_| {
            effects::response_status(call_text, args)
                .or_else(|| {
                    site.call
                        .as_ref()
                        .and_then(|call| call["status"].as_u64())
                        .map(|status| status as u16)
                })
                .or_else(|| effects::implicit_status(call_text))
        });
    json!({"effect":effect,"status":status,"when":site.when,"guards":site.guards,"loops":site.loops,"call":site.call,"trigger":site.trigger,"conditionsAvailable":site.available,"from":node_id(graph,edge.from),"to":edge.to.map(|to|node_id(graph,to)),
        "kind":edge.kind,"target":edge.target,"line":edge.line,"col":raw.column,
        "confidence":metadata.and_then(|m|m.get("confidence")).cloned().unwrap_or(Value::Null),
        "resolvedBy":metadata.and_then(|m|m.get("resolvedBy")).cloned().unwrap_or_else(||json!(edge.method)),
        "resolution":edge.method,"uncertain":edge.ambiguous || edge.confidence == "low",
        "ambiguous":edge.ambiguous,"provenance":raw.provenance,"metadata":raw.metadata})
}

fn project_kind(graph: &Graph) -> &'static str {
    graph.viewer_project.get_or_init(|| {
        let mut endpoints = false;
        let mut pages = false;
        for node in graph
            .nodes
            .iter()
            .filter(|node| node.symbol.kind == "route")
        {
            if node.symbol.name.split_once(' ').is_some_and(|(method, _)| {
                matches!(
                    method,
                    "GET"
                        | "POST"
                        | "PUT"
                        | "PATCH"
                        | "DELETE"
                        | "HEAD"
                        | "OPTIONS"
                        | "ALL"
                        | "ANY"
                )
            }) {
                endpoints = true;
            } else if node.symbol.name.starts_with('/') {
                pages = true;
                if crate::frameworks::frontend::paths::next(&node.path)
                    .ok()
                    .flatten()
                    .is_some_and(|route| {
                        route.kind == crate::frameworks::frontend::paths::Kind::Page
                    })
                {
                    return "web";
                }
            }
        }
        if !endpoints {
            "app"
        } else if pages || graph.edges.iter().any(|edge| edge.kind == "navigates") {
            "web"
        } else {
            "api"
        }
    })
}

fn lookup(graph: &Graph, id: &str) -> Result<usize> {
    if let Ok(number) = id.parse::<i64>()
        && let Some(&index) = graph.ids.get(&number)
    {
        return Ok(index);
    }
    graph
        .nodes
        .iter()
        .enumerate()
        .find(|(i, _)| node_id(graph, *i) == id)
        .map(|(i, _)| i)
        .context("symbol ID is not present in this indexed snapshot")
}

fn wire_list(mut items: Vec<Value>, limit: usize) -> Value {
    let total = items.len();
    items.truncate(limit);
    json!({"total":total,"shown":items.len(),"truncated":items.len()<total,"items":items})
}

fn relations(graph: &Graph, indices: &[usize], incoming: bool) -> Vec<Value> {
    let mut groups = BTreeMap::<usize, Vec<usize>>::new();
    for &i in indices {
        let edge = &graph.edges[i];
        if edge.kind == "contains" {
            continue;
        }
        let endpoint = if incoming { Some(edge.from) } else { edge.to };
        if let Some(endpoint) = endpoint {
            groups.entry(endpoint).or_default().push(i);
        }
    }
    groups.into_iter().map(|(index,mut edges)|{
        edges.sort_by_key(|&i|graph.edges[i].line);
        let kinds=edges.iter().map(|&i|graph.edges[i].kind.clone()).collect::<BTreeSet<_>>();
        let lines=edges.iter().map(|&i|graph.edges[i].line).collect::<BTreeSet<_>>();
        json!({"node":node_ref(graph,index),"edgeKinds":kinds,"lines":lines,
            "edgeCount":edges.len(),"uncertain":edges.iter().all(|&i|graph.edges[i].confidence=="low"),
            "synthesized":edges.iter().any(|&i|graph.canonical_edges[i].as_ref().is_some_and(|e|e.provenance.as_deref()==Some("heuristic"))),
            "edges":edges.into_iter().take(40).map(|i|edge_ref(graph,i)).collect::<Vec<_>>()})
    }).collect()
}

fn node(graph: &Graph, index: usize) -> Result<Value> {
    let focus = &graph.nodes[index];
    let incoming = relations(graph, &graph.incoming[index], true);
    let outgoing = relations(graph, &graph.outgoing[index], false);
    let members = graph.outgoing[index]
        .iter()
        .filter_map(|&i| {
            let edge = &graph.edges[i];
            (edge.kind == "contains").then_some(edge.to).flatten()
        })
        .map(|i| node_ref(graph, i))
        .collect::<Vec<_>>();
    let outside = graph.outgoing[index]
        .iter()
        .filter(|&&i| graph.edges[i].to.is_none())
        .map(|&i| edge_ref(graph, i))
        .collect::<Vec<_>>();
    let (blast, bounded) = walk(graph, index, 3, false, 10_000);
    let tests = blast
        .iter()
        .map(|&i| &graph.nodes[i].path)
        .filter(|path| crate::classify::is_test_file(path))
        .collect::<BTreeSet<_>>();
    let nodes = graph
        .files
        .values()
        .filter_map(|file| file.analysis.extraction.as_ref())
        .flat_map(|e| e.nodes.iter().cloned())
        .collect::<Vec<_>>();
    let edges = graph
        .canonical_edges
        .iter()
        .flatten()
        .cloned()
        .collect::<Vec<_>>();
    let hierarchy = canonical(graph, index)
        .map(|raw| crate::hierarchy::build_type_hierarchy(&nodes, &edges, &raw.id, true))
        .transpose()?
        .flatten();
    let mut ancestors = vec![];
    let mut seen = HashSet::from([index]);
    let mut current = index;
    while let Some(parent) = graph.incoming[current]
        .iter()
        .map(|&i| &graph.edges[i])
        .find(|edge| edge.kind == "contains")
        .map(|edge| edge.from)
    {
        if !seen.insert(parent) || ancestors.len() == 24 {
            break;
        }
        ancestors.push(node_ref(graph, parent));
        current = parent;
    }
    ancestors.reverse();
    Ok(
        json!({"node":node_ref(graph,index),"metadata":canonical(graph,index),"ancestors":ancestors,
        "members":wire_list(members,3000),"hierarchy":hierarchy,
        "counts":{"callers":incoming.len(),"callees":outgoing.len(),"fanIn":graph.incoming[index].len(),"fanOut":graph.outgoing[index].len(),"hub":incoming.len()>=40},
        "incoming":wire_list(incoming,300),"outgoing":wire_list(outgoing,200),
        "outsideIndex":wire_list(outside,40),"tests":tests,
        "blast":{"withinHops":blast.len().saturating_sub(1),"hops":3,"truncated":bounded},
        "source":source(graph,&focus.path,focus.symbol.start_line,focus.symbol.end_line)?,
        "sourceState":"indexed","truncated":graph.truncated || graph.resolution_truncated.get()}),
    )
}

fn walk(
    graph: &Graph,
    start: usize,
    depth: usize,
    both: bool,
    limit: usize,
) -> (HashSet<usize>, bool) {
    let mut seen = HashSet::from([start]);
    let mut queue = VecDeque::from([(start, 0)]);
    let mut bounded = false;
    while let Some((index, level)) = queue.pop_front() {
        if level == depth {
            continue;
        }
        for &i in graph.incoming[index].iter().chain(if both {
            graph.outgoing[index].iter()
        } else {
            [].iter()
        }) {
            let edge = &graph.edges[i];
            if edge.kind == "contains" {
                continue;
            }
            let target = if edge.from == index {
                edge.to
            } else {
                Some(edge.from)
            };
            if let Some(target) = target {
                if seen.contains(&target) {
                    continue;
                }
                if seen.len() == limit {
                    bounded = true;
                    continue;
                }
                seen.insert(target);
                queue.push_back((target, level + 1));
            }
        }
    }
    (seen, bounded)
}

fn source(graph: &Graph, path: &str, from: usize, to: usize) -> Result<Value> {
    validate_path(path)?;
    let to = if to == 0 { usize::MAX } else { to };
    ensure!(from <= to, "from must not exceed to");
    let file = graph
        .files
        .get(path)
        .context("file is not present in this indexed snapshot")?;
    ensure!(file.source.len() <= 8 * 1024 * 1024, "source exceeds 8 MiB");
    let lines = file.source.lines().collect::<Vec<_>>();
    let end = to.min(lines.len()).min(from.saturating_add(3999));
    let text = lines
        .get(from.saturating_sub(1)..end)
        .unwrap_or_default()
        .join("\n");
    Ok(
        json!({"file":path,"language":file.analysis.language,"from":from,"to":end,
        "totalLines":lines.len(),"text":text,"sourceState":"indexed","tokens":branches::highlights(&file.source,&file.analysis.language,from,end),
        "truncated":end < to.min(lines.len())}),
    )
}

fn file(graph: &Graph, path: &str, code: bool) -> Result<Value> {
    validate_path(path)?;
    let file = graph
        .files
        .get(path)
        .context("file is not present in this indexed snapshot")?;
    let outline = file
        .nodes
        .iter()
        .map(|&i| node_ref(graph, i))
        .collect::<Vec<_>>();
    let mut dependencies = BTreeSet::new();
    let mut dependents = BTreeSet::new();
    let mut outgoing = vec![];
    let mut incoming = vec![];
    let mut calls = vec![];
    let mut outside = vec![];
    for (i, edge) in graph.edges.iter().enumerate() {
        let from = &graph.nodes[edge.from].path;
        let to = edge.to.map(|i| &graph.nodes[i].path);
        if from == path {
            if to.is_some_and(|to| to != path) {
                dependencies.insert(to.unwrap());
                outgoing.push(i);
            }
            if edge.kind != "contains" {
                if edge.to.is_none() {
                    outside.push(edge_ref(graph, i));
                } else {
                    calls.push(edge_ref(graph, i));
                }
            }
        } else if to.is_some_and(|to| to == path) {
            dependents.insert(from);
            incoming.push(i);
        }
    }
    let mut value = json!({"file":{"path":path,"language":file.analysis.language,"nodeCount":file.nodes.len(),"test":crate::classify::is_test_file(path)},
        "outline":wire_list(outline,3000),"dependencies":dependencies,"dependents":dependents,
        "imports":wire_list(relations(graph,&outgoing,false),300),"importedBy":wire_list(relations(graph,&incoming,true),300),
        "sourceState":"indexed","truncated":graph.truncated || graph.resolution_truncated.get()});
    if code {
        value["source"] = source(graph, path, 1, usize::MAX)?;
        value["calls"] = wire_list(calls, 2000);
        value["outsideIndex"] = wire_list(outside, 3000);
    }
    Ok(value)
}

fn stats(graph: &Graph, engine: &Engine) -> Value {
    let mut node_kinds = BTreeMap::<&str, usize>::new();
    let mut edge_kinds = BTreeMap::<&str, usize>::new();
    let mut languages = BTreeMap::<&str, usize>::new();
    for node in &graph.nodes {
        *node_kinds.entry(&node.symbol.kind).or_default() += 1;
    }
    for edge in &graph.edges {
        *edge_kinds.entry(&edge.kind).or_default() += 1;
    }
    for file in graph.files.values() {
        *languages.entry(&file.analysis.language).or_default() += 1;
    }
    json!({"project":{"root":engine.root,"name":engine.root.file_name().and_then(|s|s.to_str())},
        "index":{"state":if graph.truncated {"partial"}else{"complete"},"backend":"sqlite","pendingReferences":graph.edges.iter().filter(|edge|edge.to.is_none()).count()},
        "graph":{"nodes":graph.nodes.len(),"edges":graph.edges.len(),"files":graph.files.len(),"nodesByKind":node_kinds,"edgesByKind":edge_kinds,"filesByLanguage":languages},
        "frameworks":graph.frameworks,"thresholds":{"hub":40,"uncertainBelow":0.6},"generation":graph.generation})
}

fn search(graph: &Graph, raw: &str, lexical: Option<&crate::SearchResult>, limit: usize) -> Value {
    let mut results = lexical
        .into_iter()
        .flat_map(|result| &result.results)
        .filter_map(|hit| graph.ids.get(&hit.id))
        .map(|&i| node_ref(graph, i))
        .collect::<Vec<_>>();
    let needle = raw.to_lowercase();
    if !needle.is_empty() && !needle.contains(':') {
        let mut ids = results
            .iter()
            .filter_map(|v| v["indexId"].as_i64())
            .collect::<HashSet<_>>();
        for (index, node) in graph.nodes.iter().enumerate() {
            if results.len() == 400 {
                break;
            }
            if node.symbol.qualified_name.to_lowercase().contains(&needle) && ids.insert(node.id) {
                results.push(node_ref(graph, index));
            }
        }
    }
    for value in &mut results {
        let name = value["name"].as_str().unwrap_or("").to_lowercase();
        value["matchKind"] = json!(if name == needle {
            "exact"
        } else if name.starts_with(&needle) {
            "prefix"
        } else if name.contains(&needle) {
            "substring"
        } else {
            "related"
        });
    }
    results.sort_by_key(|value| match value["matchKind"].as_str() {
        Some("exact") => 0,
        Some("prefix") => 1,
        Some("substring") => 2,
        _ => 3,
    });
    json!({"query":raw,"results":wire_list(results,limit),"truncated":lexical.is_some_and(|result|result.truncated)})
}

fn within(path: &str, root: &str) -> bool {
    let root = root.trim_end_matches('/');
    root.is_empty()
        || path == root
        || path
            .strip_prefix(root)
            .is_some_and(|rest| rest.starts_with('/'))
}
fn validate_path(path: &str) -> Result<()> {
    ensure!(
        path.len() <= 1024
            && !path.contains(['\\', '\0'])
            && Path::new(path)
                .components()
                .all(|part| matches!(part, Component::Normal(_))),
        "path must be relative without parent components"
    );
    Ok(())
}
type Query = Vec<(String, String)>;
fn argument<'a>(query: &'a Query, key: &str) -> Option<&'a str> {
    query
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.as_str())
}
fn integer(query: &Query, key: &str, default: usize, min: usize, max: usize) -> Result<usize> {
    let value = argument(query, key)
        .map(str::parse::<usize>)
        .transpose()?
        .unwrap_or(default);
    ensure!((min..=max).contains(&value), "invalid {key}");
    Ok(value)
}
fn parse_target(target: &str) -> Result<(String, Query)> {
    ensure!(
        target.len() <= 16 * 1024 && target.starts_with('/') && !target.starts_with("//"),
        "invalid request target"
    );
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let path = decode(path, false)?;
    ensure!(
        !path.contains(['\0', '\\']) && !path.split('/').any(|part| part == ".." || part == "."),
        "invalid request path"
    );
    let query = query
        .split('&')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let (key, value) = part.split_once('=').unwrap_or((part, ""));
            Ok((decode(key, true)?, decode(value, true)?))
        })
        .collect::<Result<_>>()?;
    Ok((path, query))
}
fn decode(value: &str, plus: bool) -> Result<String> {
    let bytes = value.as_bytes();
    let mut result = vec![];
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                ensure!(i + 2 < bytes.len(), "invalid URL escape");
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3])?;
                result.push(u8::from_str_radix(hex, 16)?);
                i += 3;
            }
            b'+' if plus => {
                result.push(b' ');
                i += 1;
            }
            byte => {
                result.push(byte);
                i += 1;
            }
        }
    }
    let value = String::from_utf8(result)?;
    ensure!(!value.contains('\0'), "invalid URL character");
    Ok(value)
}
