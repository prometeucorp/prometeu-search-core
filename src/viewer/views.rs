use super::*;
use std::collections::HashMap;

pub(super) fn routes(graph: &Graph, limit: usize) -> Value {
    let mut entries = vec![];
    for (index, node) in graph.nodes.iter().enumerate() {
        if node.symbol.kind != "route" {
            continue;
        }
        let name = &node.symbol.name;
        let (method, path) = name
            .split_once(' ')
            .filter(|(head, _)| {
                matches!(
                    *head,
                    "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "OPTIONS" | "HEAD" | "ALL"
                )
            })
            .map(|(method, path)| (Some(method), path))
            .unwrap_or((None, name));
        let handlers = graph.outgoing[index]
            .iter()
            .filter_map(|&i| {
                let edge = &graph.edges[i];
                if matches!(
                    edge.kind.as_str(),
                    "calls" | "references" | "renders" | "handles"
                ) {
                    edge.to
                } else {
                    None
                }
            })
            .collect::<BTreeSet<_>>();
        if handlers.is_empty() {
            entries.push(json!({"url":name,"method":method,"path":path,"handler":name,"handlerKind":"route","handlerId":null,"file":node.path,"line":node.symbol.start_line,"routeFile":node.path,"routeLine":node.symbol.start_line,"routeId":node_id(graph,index),"inline":true}));
        } else {
            for handler in handlers {
                let target = &graph.nodes[handler];
                entries.push(json!({"url":name,"method":method,"path":path,"handler":target.symbol.name,"handlerKind":target.symbol.kind,"handlerId":node_id(graph,handler),"file":target.path,"line":target.symbol.start_line,"routeFile":node.path,"routeLine":node.symbol.start_line,"routeId":node_id(graph,index),"inline":false}));
            }
        }
    }
    let total = entries.len();
    entries.truncate(limit);
    json!({"routed":total>0,"routeCount":graph.nodes.iter().filter(|node|node.symbol.kind=="route").count(),"shown":entries.len(),"truncated":entries.len()<total,"entries":entries})
}

pub(super) fn entrypoints(graph: &Graph, limit: usize, route_limit: usize) -> Value {
    let mut roots = vec![];
    let mut tests = vec![];
    let mut hubs = vec![];
    for (index, node) in graph.nodes.iter().enumerate() {
        if node.symbol.kind == "file" {
            let calls = graph.outgoing[index]
                .iter()
                .filter(|&&i| matches!(graph.edges[i].kind.as_str(), "calls" | "instantiates"))
                .count();
            if calls > 0
                || matches!(
                    node.path.rsplit('/').next(),
                    Some("main.rs" | "main.go" | "__main__.py" | "index.ts" | "main.ts" | "app.ts")
                )
            {
                let mut value = node_ref(graph, index);
                value["calls"] = json!(calls);
                roots.push(value);
            }
            if crate::classify::is_test_file(&node.path) {
                tests.push(node_ref(graph, index));
            }
        } else {
            let incoming = graph.incoming[index]
                .iter()
                .filter(|&&i| graph.edges[i].kind != "contains")
                .map(|&i| graph.edges[i].from)
                .collect::<HashSet<_>>()
                .len();
            if incoming > 0 {
                let mut value = node_ref(graph, index);
                value["fanIn"] = json!(incoming);
                hubs.push(value);
            }
        }
    }
    hubs.sort_by_key(|value| std::cmp::Reverse(value["fanIn"].as_u64().unwrap_or(0)));
    json!({"routes":routes(graph,route_limit),"files":wire_list(roots,limit),"tests":wire_list(tests,limit),"hubs":wire_list(hubs,limit),"truncated":graph.truncated || graph.resolution_truncated.get()})
}

pub(super) fn map(graph: &Graph, root: &str, depth: usize) -> Result<Value> {
    validate_path(root)?;
    let group = |path: &str| {
        let relative = if root.is_empty() {
            path
        } else {
            path.strip_prefix(root)
                .unwrap_or(path)
                .trim_start_matches('/')
        };
        let parts = relative.split('/').collect::<Vec<_>>();
        let folder = parts[..depth.min(parts.len().saturating_sub(1))].join("/");
        if folder.is_empty() {
            if root.is_empty() {
                "(root)".into()
            } else {
                root.to_owned()
            }
        } else if root.is_empty() {
            folder
        } else {
            format!("{root}/{folder}")
        }
    };
    let mut modules = BTreeMap::<String, Vec<&str>>::new();
    for path in graph.files.keys().filter(|path| within(path, root)) {
        modules.entry(group(path)).or_default().push(path);
    }
    let mut links = BTreeMap::<(String, String), usize>::new();
    for edge in &graph.edges {
        let Some(to) = edge.to else { continue };
        let from = &graph.nodes[edge.from].path;
        let to = &graph.nodes[to].path;
        if from == to || !within(from, root) || !within(to, root) {
            continue;
        }
        let pair = (group(from), group(to));
        if pair.0 != pair.1 {
            *links.entry(pair).or_default() += 1;
        }
    }
    let count = modules.len();
    let module_values = modules.into_iter().map(|(id, paths)| {
        let total = paths.len();
        let files = paths.into_iter().take(40).collect::<Vec<_>>();
        json!({"id":id,"label":id,"fileCount":total,"files":files,
            "fileList":{"total":total,"shown":files.len(),"truncated":files.len()<total,"items":files}})
    }).collect::<Vec<_>>();
    let link_values = links
        .iter()
        .map(|((from, to), count)| json!({"from":from,"to":to,"count":count}))
        .collect::<Vec<_>>();
    let cycles = cycle_groups(&links);
    Ok(
        json!({"root":root,"depth":depth,"modules":module_values,"links":link_values,"cycles":wire_list(cycles,40),"totalModules":count,"truncated":graph.truncated}),
    )
}

// Iterative Kosaraju traversal keeps repository depth off the native stack.
fn cycle_groups(links: &BTreeMap<(String, String), usize>) -> Vec<Value> {
    let mut next = BTreeMap::<&str, Vec<&str>>::new();
    let mut previous = BTreeMap::<&str, Vec<&str>>::new();
    for (from, to) in links.keys() {
        next.entry(from).or_default().push(to);
        next.entry(to).or_default();
        previous.entry(to).or_default().push(from);
    }
    let mut visited = HashSet::new();
    let mut ordered = vec![];
    for &start in next.keys() {
        if visited.contains(start) {
            continue;
        }
        let mut stack = vec![(start, false)];
        while let Some((node, done)) = stack.pop() {
            if done {
                ordered.push(node);
                continue;
            }
            if !visited.insert(node) {
                continue;
            }
            stack.push((node, true));
            for &to in &next[node] {
                if !visited.contains(to) {
                    stack.push((to, false));
                }
            }
        }
    }
    visited.clear();
    let mut groups = vec![];
    for &start in ordered.iter().rev() {
        if visited.contains(start) {
            continue;
        }
        let mut stack = vec![start];
        let mut group = vec![];
        while let Some(node) = stack.pop() {
            if visited.insert(node) {
                group.push(node);
                stack.extend(previous.get(node).into_iter().flatten().copied());
            }
        }
        if group.len() > 1 {
            group.sort_unstable();
            let truncated = group.len() > 12;
            group.truncate(12);
            groups.push(json!({"modules":group,"truncated":truncated}));
        }
    }
    groups
}

pub(super) fn screens(graph: &Graph) -> Value {
    let route_indices = graph
        .nodes
        .iter()
        .enumerate()
        .filter(|(_, node)| {
            node.symbol.kind == "route"
                && node.symbol.name.starts_with('/')
                && !format!("/{}", node.path).contains("/server/api/")
        })
        .map(|(i, _)| i)
        .collect::<BTreeSet<_>>();
    let mut owners = HashMap::<usize, Vec<usize>>::new();
    for &route in &route_indices {
        for &edge in &graph.outgoing[route] {
            if let Some(to) = graph.edges[edge].to {
                owners.entry(to).or_default().push(route);
            }
        }
    }
    let mut links = vec![];
    let mut origins = BTreeSet::new();
    let mut bounded = false;
    for (i, edge) in graph
        .edges
        .iter()
        .enumerate()
        .filter(|(_, edge)| edge.kind == "navigates")
    {
        let Some(to) = edge.to.filter(|to| route_indices.contains(to)) else {
            continue;
        };
        let mut queue = VecDeque::from([(edge.from, vec![])]);
        let mut visited = HashSet::new();
        let mut found = vec![];
        while let Some((index, via)) = queue.pop_front() {
            if !visited.insert(index) {
                continue;
            }
            if visited.len() > 800 {
                bounded = true;
                break;
            }
            if let Some(routes) = owners.get(&index) {
                for &route in routes {
                    found.push((route, via.clone(), index));
                }
                continue;
            }
            if via.len() == 7 {
                bounded = true;
                continue;
            }
            let callers = graph.incoming[index]
                .iter()
                .copied()
                .filter(|&edge| {
                    matches!(
                        graph.edges[edge].kind.as_str(),
                        "calls" | "references" | "renders"
                    )
                })
                .collect::<Vec<_>>();
            bounded |= callers.len() > 30;
            for incoming in callers.into_iter().take(30) {
                let previous = &graph.edges[incoming];
                if matches!(previous.kind.as_str(), "calls" | "references" | "renders") {
                    let mut chain = via.clone();
                    chain.push(index);
                    queue.push_back((previous.from, chain));
                }
            }
        }
        if found.is_empty() {
            origins.insert(edge.from);
            found.push((edge.from, vec![], edge.from));
        }
        for (from, mut via, owner) in found {
            via.reverse();
            let mut chain = vec![owner];
            chain.extend(via.iter().copied());
            let mut guards = vec![];
            for pair in chain.windows(2) {
                if let Some(&edge_index) = graph.outgoing[pair[0]]
                    .iter()
                    .find(|&&edge_index| graph.edges[edge_index].to == Some(pair[1]))
                {
                    guards.extend(branches::for_edge(graph, edge_index).guards);
                }
            }
            let site = branches::for_edge(graph, i);
            guards.extend(site.guards);
            let when = branches::label(&guards);
            links.push(json!({"id":format!("{i}:{from}"),"from":node_id(graph,from),"to":node_id(graph,to),"fromOrigin":!route_indices.contains(&from),"when":when,"guards":guards,"via":via.into_iter().map(|index|node_ref(graph,index)).collect::<Vec<_>>(),"sites":[{"file":graph.nodes[edge.from].path,"line":edge.line,"href":edge.target,"method":edge.target,"when":when}],"edge":edge_ref(graph,i)}));
        }
    }
    let screens=route_indices.iter().copied().map(|index| {
        let id=node_id(graph,index);let component=graph.outgoing[index].iter().find_map(|&i|graph.edges[i].to).map(|i|node_ref(graph,i));
        json!({"id":id,"path":graph.nodes[index].symbol.name,"file":graph.nodes[index].path,"line":graph.nodes[index].symbol.start_line,"component":component,"incoming":links.iter().filter(|link|link["to"]==id).count(),"outgoing":links.iter().filter(|link|link["from"]==id).count()})
    }).collect::<Vec<_>>();
    let chosen = screens
        .iter()
        .filter_map(|screen| screen["id"].as_str())
        .collect::<HashSet<_>>();
    links.retain(|link| {
        chosen.contains(link["to"].as_str().unwrap_or(""))
            && (link["fromOrigin"] == true || chosen.contains(link["from"].as_str().unwrap_or("")))
    });
    let selected_origins = origins.iter().copied().collect::<BTreeSet<_>>();
    let origin_ids = selected_origins
        .iter()
        .map(|&index| node_id(graph, index))
        .collect::<HashSet<_>>();
    links.retain(|link| {
        link["fromOrigin"] != true
            || link["from"]
                .as_str()
                .is_some_and(|id| origin_ids.contains(id))
    });
    bounded |= origins.len() > selected_origins.len();
    json!({"screens":screens,"origins":selected_origins.into_iter().map(|i|json!({"id":node_id(graph,i),"node":node_ref(graph,i)})).collect::<Vec<_>>(),"links":links,"truncated":bounded || graph.truncated,"conditionsAvailable":true})
}

fn named(graph: &Graph, name: &str) -> Vec<usize> {
    if let Ok(index) = lookup(graph, name) {
        vec![index]
    } else {
        graph
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| node.symbol.name == name || node.symbol.qualified_name == name)
            .map(|(index, _)| index)
            .collect()
    }
}

pub(super) fn steps(graph: &Graph, query: &Query, limit: usize) -> Result<Value> {
    let name = argument(query, "anchor")
        .or_else(|| argument(query, "symbol"))
        .context("anchor or symbol is required")?;
    let starts = named(graph, name);
    let start = *starts
        .first()
        .context("anchor is not in the indexed graph")?;
    let depth = integer(query, "depth", 8, 1, 14)?;
    let mut queue = VecDeque::from([(start, 0, start, 0)]);
    let mut discovered = HashMap::from([(start, 0)]);
    let mut step_nodes = HashSet::from([start]);
    let mut step_count = 1;
    let mut folded = HashMap::<usize, HashSet<usize>>::new();
    let mut nodes = BTreeMap::<String, Value>::new();
    let mut links = vec![];
    let mut outside = vec![];
    let mut emitted = HashSet::new();
    let mut bounded = false;
    while let Some((index, level, owner, fold_depth)) = queue.pop_front() {
        if discovered[&index] < level {
            continue;
        }
        let id = node_id(graph, index);
        let mut value = node_ref(graph, index);
        let step_kind = if index == start {
            "anchor"
        } else if graph.nodes[index].symbol.kind == "route" {
            "screen"
        } else if store_file(&graph.nodes[index].path) {
            "store"
        } else {
            "symbol"
        };
        value["stepKind"] = json!(step_kind);
        nodes.insert(id.clone(), value);
        if level == depth {
            bounded |= !graph.outgoing[index].is_empty();
            continue;
        }
        let mut outgoing = graph.outgoing[index]
            .iter()
            .copied()
            .filter(|&edge| {
                !matches!(
                    graph.edges[edge].kind.as_str(),
                    "contains" | "imports" | "extends" | "implements"
                )
            })
            .collect::<Vec<_>>();
        outgoing.sort_by(|&left, &right| {
            (graph.edges[left].line, &graph.edges[left].target)
                .cmp(&(graph.edges[right].line, &graph.edges[right].target))
        });
        bounded |= outgoing.len() > 80;
        for edge_index in outgoing.into_iter().take(80) {
            let edge = &graph.edges[edge_index];
            let mut link = edge_ref(graph, edge_index);
            link["id"] = json!(format!("site:{edge_index}"));
            if let Some(to) = edge.to {
                let metadata = graph.canonical_edges[edge_index]
                    .as_ref()
                    .and_then(|edge| edge.metadata.as_ref());
                let cross =
                    language_family(&graph.files[&graph.nodes[index].path].analysis.language)
                        != language_family(&graph.files[&graph.nodes[to].path].analysis.language);
                let evidenced = link["provenance"] == "heuristic"
                    || metadata.is_some_and(|m| {
                        m["bridge"] == "react-native"
                            || m["resolvedBy"] == "framework"
                            || m["channel"] == "server-action"
                    });
                if cross && !evidenced {
                    continue;
                }
                let kind = if edge.kind == "navigates" || graph.nodes[to].symbol.kind == "route" {
                    "screen"
                } else if metadata
                    .and_then(|m| m["synthesizedBy"].as_str())
                    .is_some_and(|by| by.contains("event"))
                {
                    "event"
                } else if cross {
                    "bridge"
                } else if store_file(&graph.nodes[to].path) && !store_file(&graph.nodes[index].path)
                {
                    "store"
                } else if graph.nodes[to].symbol.kind != "component"
                    && (metadata.is_some_and(|m| m["valueRef"] == true || m["fnRef"] == true)
                        || !link["trigger"].is_null())
                {
                    "trigger"
                } else if !link["effect"].is_null() {
                    "effect"
                } else {
                    "call"
                };
                let is_step = kind != "call";
                if is_step && !step_nodes.contains(&to) {
                    if step_count >= limit {
                        bounded = true;
                        continue;
                    }
                    step_nodes.insert(to);
                    step_count += 1;
                }
                let next_level = level + usize::from(is_step);
                if discovered.get(&to).is_none_or(|&best| next_level < best) {
                    if !is_step {
                        let folded_nodes = folded.entry(owner).or_default();
                        if fold_depth >= 7
                            || !folded_nodes.contains(&to) && folded_nodes.len() >= 300
                        {
                            bounded = true;
                            continue;
                        }
                        folded_nodes.insert(to);
                    }
                    discovered.insert(to, next_level);
                    queue.push_back((
                        to,
                        next_level,
                        if is_step { to } else { owner },
                        if is_step { 0 } else { fold_depth + 1 },
                    ));
                }
                link["stepKind"] = json!(kind);
                if emitted.insert(edge_index) {
                    links.push(link);
                }
            } else if !link["effect"].is_null() {
                if emitted.contains(&edge_index) {
                    continue;
                }
                if step_count >= limit {
                    bounded = true;
                    continue;
                }
                step_count += 1;
                let effect_id = format!("outside:{edge_index}");
                nodes.insert(effect_id.clone(),json!({"id":effect_id,"name":link["call"]["callee"].as_str().unwrap_or(&edge.target),"kind":"external","stepKind":"effect","effect":link["effect"],"status":link["status"],"file":graph.nodes[index].path,"line":edge.line}));
                link["to"] = json!(effect_id);
                link["stepKind"] = json!("effect");
                emitted.insert(edge_index);
                links.push(link);
            } else if emitted.insert(edge_index) {
                outside.push(link);
            }
        }
    }
    links.retain(|link| {
        nodes.contains_key(link["from"].as_str().unwrap_or(""))
            && nodes.contains_key(link["to"].as_str().unwrap_or(""))
    });
    let mut essential = HashSet::from([node_id(graph, start)]);
    for link in &links {
        if link["stepKind"] != "call"
            && let Some(id) = link["to"].as_str()
        {
            essential.insert(id.to_owned());
            if let Some(node) = nodes.get_mut(id) {
                if id != node_id(graph, start) {
                    node["stepKind"] = link["stepKind"].clone();
                }
                if !link["effect"].is_null() {
                    node["effect"] = link["effect"].clone();
                    node["status"] = link["status"].clone();
                }
            }
        }
    }
    let mut sites = BTreeMap::<String, Vec<program::Site>>::new();
    let descriptions = nodes
        .iter()
        .map(|(id, node)| {
            (
                id.clone(),
                program::Step {
                    reply: node["effect"]["category"] == "response",
                    into: (node["kind"] != "external").then(|| id.clone()),
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    for link in &links {
        let from = link["from"].as_str().unwrap_or("");
        let line = link["line"].as_u64().unwrap_or(1) as usize;
        let column = link["col"].as_u64().unwrap_or(0) as usize;
        let span = &link["call"]["span"];
        sites.entry(from.into()).or_default().push(program::Site {
            step: link["to"]
                .as_str()
                .filter(|id| essential.contains(*id))
                .map(str::to_owned),
            link: link["id"].as_str().map(str::to_owned),
            into: link["to"]
                .as_str()
                .filter(|id| !essential.contains(*id))
                .map(str::to_owned),
            at: program::Position {
                line: span["start"]["line"].as_u64().unwrap_or(line as u64) as usize,
                column: span["start"]["column"].as_u64().unwrap_or(column as u64) as usize,
                end: program::Point {
                    line: span["end"]["line"].as_u64().unwrap_or(line as u64) as usize,
                    column: span["end"]["column"].as_u64().unwrap_or(column as u64 + 1) as usize,
                },
            },
            within: link["call"]["within"].as_str().map(str::to_owned),
            guards: serde_json::from_value(link["guards"].clone())?,
            loops: serde_json::from_value(link["loops"].clone())?,
            trigger: (!link["trigger"].is_null()).then(|| link["trigger"].clone()),
        });
    }
    let program = program::build(Some(&node_id(graph, start)), &sites, &nodes, &descriptions);
    let (folded, fold_bounded) = fold_steps(&links, &nodes, &essential);
    nodes.retain(|id, _| essential.contains(id));
    Ok(
        json!({"anchor":node_id(graph,start),"nodes":nodes.into_values().collect::<Vec<_>>(),"links":folded,"program":program,"outsideIndex":wire_list(outside,100),"depth":depth,"limit":limit,"ambiguous":starts.len()>1,"truncated":bounded || fold_bounded || graph.truncated,"conditionsAvailable":true}),
    )
}
fn fold_steps(
    links: &[Value],
    nodes: &BTreeMap<String, Value>,
    essential: &HashSet<String>,
) -> (Vec<Value>, bool) {
    let mut outgoing = HashMap::<&str, Vec<&Value>>::new();
    for link in links {
        if let Some(from) = link["from"].as_str() {
            outgoing.entry(from).or_default().push(link);
        }
    }
    let mut result = vec![];
    let mut bounded = false;
    let mut roots = essential.iter().collect::<Vec<_>>();
    roots.sort();
    for root in roots {
        let mut visited = HashSet::new();
        let mut queue = VecDeque::from([(
            root.as_str(),
            Vec::<String>::new(),
            Vec::<branches::Guard>::new(),
        )]);
        while let Some((current, via, guards)) = queue.pop_front() {
            if !visited.insert((current, guards.clone())) {
                continue;
            }
            if visited.len() > 300 {
                bounded = true;
                break;
            }
            for original in outgoing.get(current).into_iter().flatten() {
                let Some(to) = original["to"].as_str() else {
                    continue;
                };
                if to == root || via.iter().any(|id| id == to) {
                    continue;
                }
                let mut next_guards = guards.clone();
                if let Ok(site_guards) =
                    serde_json::from_value::<Vec<branches::Guard>>(original["guards"].clone())
                {
                    next_guards.extend(site_guards);
                }
                if essential.contains(to) {
                    let mut link = (*original).clone();
                    link["from"] = json!(root);
                    link["via"] = json!(
                        via.iter()
                            .filter_map(|id| nodes.get(id))
                            .collect::<Vec<_>>()
                    );
                    link["when"] = json!(branches::label(&next_guards));
                    link["guards"] = json!(next_guards);
                    link["sites"] = json!([{"file":nodes.get(current).and_then(|node|node["file"].as_str()),"line":original["line"],"text":original["call"]["callee"].as_str().or_else(||original["target"].as_str()),"args":original["call"]["args"],"when":original["when"],"decision":original["guards"].as_array().and_then(|guards|guards.last())}]);
                    result.push(link);
                } else if via.len() < 7 {
                    let mut next_via = via.clone();
                    next_via.push(to.into());
                    queue.push_back((to, next_via, next_guards));
                } else {
                    bounded = true;
                }
            }
        }
    }
    (result, bounded)
}

fn store_file(path: &str) -> bool {
    static PATTERN: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    PATTERN.get_or_init(||regex::Regex::new(r"(?i)(?:^|/)(?:stores?|storage|state|slices?|reducers?)/|\.(?:store|storage|slice|reducer)\.[cm]?[jt]sx?$").unwrap()).is_match(path)
}
fn language_family(language: &str) -> &str {
    match language {
        "typescript" | "tsx" | "javascript" | "jsx" => "js",
        "java" | "kotlin" | "scala" => "jvm",
        "c" | "cpp" | "objc" => "c",
        _ => language,
    }
}

pub(super) fn flow(graph: &Graph, query: &Query) -> Result<Value> {
    let limit = integer(query, "limit", 4, 1, 4)?;
    let raw_hops = query
        .iter()
        .filter(|(key, value)| key == "hop" && value.len() > 1)
        .map(|(_, value)| value)
        .collect::<Vec<_>>();
    if !raw_hops.is_empty() {
        ensure!(
            (1..=64).contains(&raw_hops.len()),
            "trail requires 1 through 64 hops"
        );
        let mut indices = Vec::<usize>::new();
        let mut connections = vec![];
        let mut missing = vec![];
        for hop in raw_hops {
            let id = hop.get(1..).context("invalid trail direction prefix")?;
            let Ok(index) = lookup(graph, id) else {
                missing.push(id);
                continue;
            };
            if let Some(&previous) = indices.last() {
                let forward = graph.outgoing[previous]
                    .iter()
                    .find(|&&i| graph.edges[i].to == Some(index))
                    .map(|&i| (i, false));
                let backward = graph.outgoing[index]
                    .iter()
                    .find(|&&i| graph.edges[i].to == Some(previous))
                    .map(|&i| (i, true));
                connections.push(forward.or(backward));
            }
            indices.push(index);
        }
        let flows = if !indices.is_empty() {
            vec![wire_flow(graph, &indices, &connections, None, false)?]
        } else {
            vec![]
        };
        return Ok(
            json!({"query":{"kind":"trail","from":null,"to":null,"symbols":[]},"flows":flows,"ambiguous":[],"unresolved":missing,"reason":if indices.is_empty(){Some("No trail symbols remain in the index.")}else{None},"truncated":false}),
        );
    }
    let from = argument(query, "from").filter(|value| !value.trim().is_empty());
    let to = argument(query, "to").filter(|value| !value.trim().is_empty());
    let directed = from.is_some() && to.is_some();
    let tokens = if directed {
        ensure!(from != to, "from and to name the same symbol");
        vec![from.unwrap().to_owned(), to.unwrap().to_owned()]
    } else {
        argument(query, "symbols")
            .context("provide from/to, symbols, or repeated hop parameters")?
            .split(|ch: char| ch.is_whitespace() || ",;→".contains(ch))
            .map(|word| word.trim_matches(['`', '\'', '"', '(', ')']))
            .filter(|word| {
                !word.is_empty()
                    && !matches!(
                        *word,
                        "how"
                            | "does"
                            | "call"
                            | "calls"
                            | "from"
                            | "to"
                            | "through"
                            | "and"
                            | "the"
                            | "flow"
                            | "between"
                    )
            })
            .take(16)
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    let candidates = tokens
        .iter()
        .map(|token| {
            named(graph, token)
                .into_iter()
                .take(if directed { 12 } else { 6 })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let named_indices = candidates
        .iter()
        .flatten()
        .copied()
        .take(40)
        .collect::<BTreeSet<_>>();
    let starts = if directed {
        candidates.first().cloned().unwrap_or_default()
    } else {
        candidates.iter().flatten().copied().take(8).collect()
    };
    let targets = if directed {
        candidates
            .get(1)
            .into_iter()
            .flatten()
            .copied()
            .collect::<BTreeSet<_>>()
    } else {
        named_indices.clone()
    };
    let mut queue = starts
        .iter()
        .map(|&index| (vec![index], vec![], 0usize))
        .collect::<VecDeque<_>>();
    let mut paths = vec![];
    let mut work = 0;
    let mut bounded = false;
    while let Some((indices, edges, bridge)) = queue.pop_front() {
        work += 1;
        if work > if directed { 12_000 } else { 1500 } {
            bounded = true;
            break;
        }
        let index = *indices.last().unwrap();
        let covered = candidates
            .iter()
            .filter(|set| set.iter().any(|candidate| indices.contains(candidate)))
            .count();
        if indices.len() > 1
            && ((directed && targets.contains(&index))
                || (!directed && named_indices.contains(&index) && covered > 1))
        {
            paths.push((indices.clone(), edges.clone(), covered));
            if directed {
                if paths.len() == limit {
                    break;
                }
                continue;
            }
            if paths.len() == 200 {
                bounded = true;
                break;
            }
        }
        if indices.len() > if directed { 12 } else { 7 } {
            bounded = true;
            continue;
        }
        let mut choices = graph.outgoing[index]
            .iter()
            .copied()
            .filter(|&i| {
                matches!(graph.edges[i].kind.as_str(), "calls" | "navigates")
                    && graph.edges[i].to.is_some()
            })
            .collect::<Vec<_>>();
        choices.sort_by_key(|&i| {
            let edge = &graph.edges[i];
            (edge.confidence == "low", edge.line, edge.column)
        });
        let mut added = HashSet::new();
        for edge_index in choices {
            let target = graph.edges[edge_index].to.unwrap();
            if indices.contains(&target) || !added.insert(target) {
                continue;
            }
            let next_bridge = if named_indices.contains(&target) {
                0
            } else {
                bridge + 1
            };
            if !directed && next_bridge > 1 {
                continue;
            }
            if queue.len() >= 10_000 {
                bounded = true;
                break;
            }
            let mut path = indices.clone();
            path.push(target);
            let mut links = edges.clone();
            links.push(Some((edge_index, false)));
            queue.push_back((path, links, next_bridge));
        }
    }
    if !directed {
        paths.sort_by_key(|(indices, _, covered)| {
            (
                std::cmp::Reverse(*covered),
                std::cmp::Reverse(indices.len()),
            )
        });
        paths.truncate(limit);
    }
    let mut flows = vec![];
    for (indices, edges, covered) in &paths {
        let boundary = if !directed && *covered < candidates.len() {
            boundary(graph, *indices.last().unwrap(), &named_indices)
        } else {
            None
        };
        flows.push(wire_flow(graph, indices, edges, boundary, false)?);
    }
    if flows.is_empty() {
        for &index in starts.iter().chain(targets.iter()) {
            if let Some(boundary) = boundary(graph, index, &named_indices) {
                flows.push(wire_flow(graph, &[index], &[], Some(boundary), true)?);
                break;
            }
        }
    }
    let ambiguous=tokens.iter().zip(&candidates).filter(|(_,nodes)|nodes.len()>1).map(|(token,nodes)|json!({"token":token,"chosen":paths.iter().find_map(|(path,_,_)|path.iter().find(|index|nodes.contains(index))).map(|&i|node_ref(graph,i)),"others":nodes.iter().map(|&i|node_ref(graph,i)).collect::<Vec<_>>()})).collect::<Vec<_>>();
    let unresolved = tokens
        .iter()
        .zip(&candidates)
        .filter(|(_, nodes)| nodes.is_empty())
        .map(|(token, _)| token)
        .collect::<Vec<_>>();
    Ok(
        json!({"query":{"kind":if directed{"directed"}else{"symbols"},"from":from,"to":to,"symbols":tokens},"flows":flows,"ambiguous":ambiguous,"unresolved":unresolved,"reason":if paths.is_empty(){Some("No connecting call path within the traversal limits. Dynamic dispatch may not have a static edge.")}else{None},"truncated":bounded || graph.truncated}),
    )
}
fn wire_flow(
    graph: &Graph,
    indices: &[usize],
    edges: &[Option<(usize, bool)>],
    boundary: Option<Value>,
    partial: bool,
) -> Result<Value> {
    let mut hops = vec![];
    for (position, &index) in indices.iter().enumerate() {
        let node = &graph.nodes[index];
        let incoming = position
            .checked_sub(1)
            .and_then(|i| edges.get(i))
            .copied()
            .flatten();
        let edge = incoming.map(|(i, upward)| {
            let mut value = edge_ref(graph, i);
            value["upward"] = json!(upward);
            value["label"] = json!(if value["provenance"] == "heuristic" {
                format!(
                    "via {}",
                    value["metadata"]["synthesizedBy"]
                        .as_str()
                        .unwrap_or("inferred binding")
                        .replace('-', " ")
                )
            } else if upward {
                "called by".into()
            } else {
                graph.edges[i].kind.clone()
            });
            value
        });
        let outgoing = edges.get(position).copied().flatten();
        let call_ref=outgoing.filter(|(_,upward)|!*upward).map(|(i,_)|json!({"line":graph.edges[i].line,"targetId":node_id(graph,*indices.get(position+1).unwrap_or(&index)),"backwards":false})).or_else(||incoming.filter(|(_,upward)|*upward).map(|(i,_)|json!({"line":graph.edges[i].line,"targetId":node_id(graph,indices[position-1]),"backwards":true})));
        let anchor = call_ref
            .as_ref()
            .and_then(|reference| reference["line"].as_u64())
            .map(|line| line as usize)
            .unwrap_or(node.symbol.start_line);
        hops.push(json!({"node":node_ref(graph,index),"edge":edge,"callRef":call_ref,"source":source(graph,&node.path,anchor.saturating_sub(4).max(node.symbol.start_line),anchor.saturating_add(12).min(node.symbol.end_line))?}));
    }
    Ok(
        json!({"id":indices.iter().map(|&i|node_id(graph,i)).collect::<Vec<_>>().join("|"),"label":indices.iter().map(|&i|graph.nodes[i].symbol.name.clone()).collect::<Vec<_>>().join(" → "),"hops":hops,"partial":partial,"boundary":boundary}),
    )
}
fn boundary(graph: &Graph, index: usize, named: &BTreeSet<usize>) -> Option<Value> {
    let sites = graph.outgoing[index]
        .iter()
        .copied()
        .filter(|&i| {
            let edge = &graph.edges[i];
            edge.to.is_none()
                && matches!(edge.kind.as_str(), "calls" | "navigates")
                && (edge.target.contains('[')
                    || matches!(
                        edge.target.rsplit('.').next().unwrap_or(&edge.target),
                        "dispatch"
                            | "emit"
                            | "publish"
                            | "invoke"
                            | "apply"
                            | "trigger"
                            | "execute"
                            | "call"
                    ))
        })
        .take(3)
        .map(|i| edge_ref(graph, i))
        .collect::<Vec<_>>();
    if sites.is_empty() {
        return None;
    }
    let mut certain = vec![];
    let mut uncertain = vec![];
    for &i in &graph.outgoing[index] {
        let edge = &graph.edges[i];
        if let Some(to) = edge.to
            && edge.kind != "contains"
        {
            let candidate = json!({"node":node_ref(graph,to),"line":edge.line,"confidence":edge.confidence,"named":named.contains(&to)});
            if edge.ambiguous || edge.confidence == "low" {
                uncertain.push(candidate);
            } else {
                certain.push(candidate);
            }
        }
    }
    Some(
        json!({"node":node_ref(graph,index),"kind":"dynamic-dispatch","sites":sites,"uncertain":wire_list(uncertain,6),"further":wire_list(certain,6),"missed":named.iter().copied().filter(|&i|i!=index).take(4).map(|i|node_ref(graph,i)).collect::<Vec<_>>(),"explanation":"The indexed call target is unresolved. Continuations are candidates, not a proven runtime path."}),
    )
}
