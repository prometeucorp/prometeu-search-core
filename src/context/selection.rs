use super::*;
use crate::search::{
    extract_search_terms, is_distinctive_identifier, score_path_relevance, stem_variants,
};
use std::collections::VecDeque;
const DEFINITIONS: &[&str] = &[
    "class",
    "interface",
    "struct",
    "union",
    "trait",
    "protocol",
    "enum",
    "type_alias",
];
const CALLABLES: &[&str] = &["function", "method", "component"];
const TYPES: &[&str] = &["class", "interface", "struct", "union", "trait", "protocol"];
const FALLBACK: &[&str] = &[
    "file",
    "module",
    "class",
    "struct",
    "union",
    "interface",
    "trait",
    "protocol",
    "function",
    "method",
    "property",
    "field",
    "variable",
    "constant",
    "enum",
    "enum_member",
    "type_alias",
    "namespace",
    "export",
    "route",
    "component",
];
#[derive(Clone)]
struct Hit<'a> {
    node: &'a Node,
    score: f64,
}
fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|s| (*s).to_string()).collect()
}
fn rank(hits: &mut [Hit<'_>]) {
    hits.sort_by(|a, b| b.score.total_cmp(&a.score));
}
fn title(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => {
            (if first.len_utf16() == 1 {
                first.to_uppercase().collect::<String>()
            } else {
                first.to_string()
            }) + &chars.as_str().to_lowercase()
        }
        None => String::new(),
    }
}
fn include(node: &Node) -> bool {
    node.kind == "import" && node.language == "cobol"
}
fn push_node<'a>(nodes: &mut Vec<&'a Node>, known: &mut HashSet<&'a str>, node: &'a Node) {
    if known.insert(&node.id) {
        nodes.push(node);
    }
}
fn merge_edges(edges: &mut Vec<Edge>, new: impl IntoIterator<Item = Edge>) {
    let mut known: HashSet<_> = edges
        .iter()
        .map(|edge| (edge.source.clone(), edge.target.clone(), edge.kind.clone()))
        .collect();
    for edge in new {
        if known.insert((edge.source.clone(), edge.target.clone(), edge.kind.clone())) {
            edges.push(edge);
        }
    }
}
impl<'a> ContextBuilder<'a> {
    fn search(&self, query: ContextQuery) -> Result<Vec<Hit<'a>>> {
        Ok((self.query)(&query)?
            .into_iter()
            .filter_map(|hit| {
                self.by_id.get(hit.id.as_str()).map(|node| Hit {
                    node,
                    score: hit.score,
                })
            })
            .collect())
    }
    fn text(&self, query: &str, kinds: &[String], limit: usize) -> Result<Vec<Hit<'a>>> {
        self.search(ContextQuery::Text {
            query: query.to_string(),
            kinds: kinds.to_vec(),
            limit,
        })
    }
    fn substring(&self, text: &str, kinds: &[&str], exclude_prefix: bool) -> Result<Vec<Hit<'a>>> {
        self.search(ContextQuery::Substring {
            text: text.to_string(),
            kinds: strings(kinds),
            exclude_prefix,
            limit: 200,
        })
    }
    pub fn find(&self, query: &str, options: &FindContextOptions) -> Result<ContextSubgraph> {
        ensure!(
            options.search_limit > 0
                && options.search_limit <= (i64::MAX as usize) / 15
                && options.max_nodes > 0
                && options.max_nodes <= usize::MAX / 15
                && options.min_score.is_finite(),
            "invalid context selection bounds"
        );
        ensure!(
            query.len() <= 1024 * 1024 && options.seed_names.len() <= 200,
            "context query exceeds the supported bound"
        );
        let empty = || ContextSubgraph {
            nodes: vec![],
            edges: vec![],
            roots: vec![],
            confidence: String::new(),
        };
        if query.chars().all(crate::search::query_whitespace) {
            return Ok(empty());
        }
        let kinds = options
            .node_kinds
            .clone()
            .unwrap_or_else(|| strings(HIGH_VALUE_KINDS));
        let symbols: Vec<_> = extract_symbols_from_query(query)?
            .into_iter()
            .take(MAX_QUERY_SYMBOLS)
            .collect();
        let limit = options.search_limit;
        let mut exact = if symbols.is_empty() {
            vec![]
        } else {
            self.search(ContextQuery::ExactNames {
                names: symbols.clone(),
                kinds: kinds.clone(),
                limit: limit * 5,
            })?
        };
        if !options.seed_names.is_empty() {
            for mut hit in self.search(ContextQuery::ExactNames {
                names: options.seed_names.clone(),
                kinds: kinds.clone(),
                limit: limit * 3,
            })? {
                if !exact.iter().any(|e| e.node.id == hit.node.id) {
                    hit.score *= 0.6;
                    exact.push(hit);
                }
            }
        }
        if exact.len() > 1 {
            let mut files: HashMap<&str, HashSet<String>> = HashMap::new();
            for hit in &exact {
                files
                    .entry(&hit.node.file_path)
                    .or_default()
                    .insert(hit.node.name.to_lowercase());
            }
            for hit in &mut exact {
                hit.score +=
                    files[hit.node.file_path.as_str()].len().saturating_sub(1) as f64 * 20.0;
            }
            rank(&mut exact);
        }
        exact.truncate(limit * 2);
        let mut expanded = symbols.clone();
        for symbol in &symbols {
            for stem in stem_variants(symbol) {
                if !expanded.contains(&stem) {
                    expanded.push(stem);
                }
            }
        }
        for symbol in expanded {
            let titled = title(&symbol);
            if titled == symbol {
                continue;
            }
            let mut matches = Vec::new();
            for mut hit in self.text(&titled, &strings(DEFINITIONS), 30)? {
                if hit
                    .node
                    .name
                    .to_lowercase()
                    .starts_with(&titled.to_lowercase())
                {
                    hit.score += 15.0
                        + (10.0
                            - (utf16_len(&hit.node.name) as f64 - utf16_len(&titled) as f64) / 3.0)
                            .max(0.0);
                    matches.push(hit);
                }
            }
            rank(&mut matches);
            for hit in matches.into_iter().take(limit) {
                if !exact.iter().any(|e| e.node.id == hit.node.id) {
                    exact.push(hit);
                }
            }
        }
        rank(&mut exact);
        exact.truncate(limit * 3);
        let filename = query.trim_matches(crate::search::query_whitespace);
        let allows = |kind: &str| {
            options
                .node_kinds
                .as_ref()
                .is_none_or(|kinds| kinds.is_empty() || kinds.iter().any(|k| k == kind))
        };
        let mut file_matches = Vec::new();
        if allows("file") && regex::Regex::new(r"\p{scx:Han}")?.is_match(filename) {
            let mut candidates: Vec<_> = self
                .nodes
                .iter()
                .copied()
                .filter(|n| n.kind == "file" && n.name == filename)
                .collect();
            let mut prefix: Vec<_> = self
                .nodes
                .iter()
                .copied()
                .filter(|n| n.kind == "file" && n.name.starts_with(&format!("{filename}.")))
                .collect();
            candidates.sort_by(|a, b| {
                a.file_path
                    .cmp(&b.file_path)
                    .then(a.start_line.cmp(&b.start_line))
                    .then(a.id.cmp(&b.id))
            });
            prefix.sort_by(|a, b| a.name.cmp(&b.name));
            candidates.extend(prefix);
            for node in candidates {
                let base = node.file_path.rsplit('/').next().unwrap_or(&node.file_path);
                let stem = base
                    .rsplit_once('.')
                    .filter(|(prefix, _)| !prefix.is_empty())
                    .map(|(prefix, _)| prefix)
                    .unwrap_or(base);
                if base == filename || stem == filename {
                    file_matches.push(Hit { node, score: 1.0 });
                }
            }
        }
        let (copybooks, members) = if allows("import") {
            self.copybooks(query)?
        } else {
            (vec![], HashSet::new())
        };
        let search_kinds = if kinds.is_empty() {
            strings(FALLBACK)
        } else {
            kinds.clone()
        };
        let terms: Vec<_> = extract_search_terms(query, true)
            .into_iter()
            .filter(|term| !members.contains(&term.to_uppercase()))
            .take(MAX_QUERY_TERMS)
            .collect();
        let mut text: Vec<(Hit<'a>, usize)> = Vec::new();
        for term in terms {
            for hit in self.text(&term, &search_kinds, limit * 2)? {
                if let Some((old, count)) =
                    text.iter_mut().find(|(old, _)| old.node.id == hit.node.id)
                {
                    *count += 1;
                    old.score = old.score.max(hit.score);
                } else {
                    text.push((hit, 1));
                }
            }
        }
        let mut text: Vec<_> = text
            .into_iter()
            .map(|(mut hit, count)| {
                hit.score += (count - 1) as f64 * 5.0;
                hit
            })
            .collect();
        rank(&mut text);
        text.truncate(limit * 2);
        let mut results: Vec<Hit<'a>> = Vec::new();
        for hit in exact.iter().cloned().chain(text) {
            if let Some(old) = results.iter_mut().find(|old| old.node.id == hit.node.id) {
                old.score = old.score.max(hit.score);
            } else {
                results.push(hit);
            }
        }
        let lower = query.to_lowercase();
        let test_query = lower.contains("test") || lower.contains("spec");
        if !test_query {
            for hit in &mut results {
                if is_test(&hit.node.file_path) {
                    hit.score *= 0.3;
                }
            }
        }
        if let Some(directory) = self.dominant_directory() {
            for hit in &mut results {
                if hit.node.file_path.starts_with(&directory) {
                    hit.score += 25.0;
                }
            }
        }
        let boost_terms: Vec<_> = extract_search_terms(query, true)
            .into_iter()
            .take(MAX_QUERY_TERMS)
            .collect();
        if boost_terms.len() >= 2 {
            let mut sorted = boost_terms;
            sorted.sort_by_key(|term| std::cmp::Reverse(utf16_len(term)));
            let (mut groups, mut assigned) = (Vec::new(), HashSet::new());
            for term in &sorted {
                if !assigned.insert(term.as_str()) {
                    continue;
                }
                let mut group = vec![term.as_str()];
                for other in &sorted {
                    if !assigned.contains(other.as_str())
                        && (term.contains(other) || other.contains(term))
                    {
                        assigned.insert(other.as_str());
                        group.push(other.as_str());
                    }
                }
                groups.push(group);
            }
            let exact_ids: HashSet<_> = exact.iter().map(|hit| hit.node.id.as_str()).collect();
            let distinctive: HashSet<_> = symbols
                .iter()
                .filter(|s| is_distinctive_identifier(s))
                .map(|s| s.to_lowercase())
                .collect();
            for hit in &mut results {
                let name = hit.node.name.to_lowercase();
                let dir = directory(&hit.node.file_path).to_lowercase();
                let count = groups
                    .iter()
                    .filter(|group| {
                        group.iter().any(|term| {
                            name.contains(term) || dir.split('/').any(|part| part == *term)
                        })
                    })
                    .count();
                if count >= 2 {
                    hit.score *= 1.0 + count as f64 * 0.5;
                } else if exact_ids.contains(hit.node.id.as_str()) && distinctive.contains(&name) {
                } else if exact_ids.contains(hit.node.id.as_str()) {
                    hit.score *= 0.3;
                } else {
                    hit.score *= 0.6;
                }
            }
            rank(&mut results);
        }
        let mut path_scores = HashMap::new();
        let project = HashSet::new();
        let mut path_score = |path: &str| {
            *path_scores.entry(path.to_string()).or_insert_with(|| {
                score_path_relevance(path, query, &project, (self.deprioritized)(path))
            })
        };
        let mut search_ids: HashSet<_> = results.iter().map(|h| h.node.id.as_str()).collect();
        let mut searched = HashSet::new();
        let mut camel: Vec<(Hit<'a>, usize)> = Vec::new();
        for symbol in &symbols {
            let titled = title(symbol);
            let key = titled.to_lowercase();
            if utf16_len(&titled) < 3 || !searched.insert(key.clone()) {
                continue;
            }
            let mut candidates = Vec::new();
            for mut hit in self
                .substring(&titled, DEFINITIONS, true)?
                .into_iter()
                .chain(self.substring(&titled, CALLABLES, true)?)
            {
                let name = &hit.node.name;
                let lowered = name.to_lowercase();
                let Some(offset) = lowered.find(&key) else {
                    continue;
                };
                let position = utf16_len(&lowered[..offset]);
                let original: Vec<_> = name.encode_utf16().collect();
                if position == 0
                    || !original
                        .get(position)
                        .is_some_and(|unit| matches!(unit, 65..=90))
                    || !original
                        .get(position - 1)
                        .is_some_and(|unit| matches!(unit,65..=90|97..=122))
                    || search_ids.contains(hit.node.id.as_str())
                    || (is_test(&hit.node.file_path) && !test_query)
                {
                    continue;
                }
                hit.score = 8.0
                    + (6.0 - (utf16_len(name) as f64 - utf16_len(&titled) as f64) / 4.0).max(0.0)
                    + path_score(&hit.node.file_path);
                candidates.push(hit);
            }
            rank(&mut candidates);
            for hit in candidates.into_iter().take(limit.div_ceil(2) * 4) {
                if let Some((_, count)) =
                    camel.iter_mut().find(|(old, _)| old.node.id == hit.node.id)
                {
                    *count += 1;
                } else {
                    camel.push((hit, 1));
                }
            }
        }
        let mut camel: Vec<_> = camel
            .into_iter()
            .map(|(mut hit, count)| {
                hit.score = hit.score * (1 + count) as f64 + (count - 1) as f64 * 30.0;
                hit
            })
            .collect();
        rank(&mut camel);
        for hit in camel.into_iter().take(limit) {
            search_ids.insert(&hit.node.id);
            results.push(hit);
        }
        if symbols.len() >= 2 {
            let mut compound: Vec<(&Node, HashSet<String>)> = Vec::new();
            for symbol in &symbols {
                let titled = title(symbol);
                if utf16_len(&titled) < 3 {
                    continue;
                }
                for hit in self
                    .substring(&titled, DEFINITIONS, false)?
                    .into_iter()
                    .chain(self.substring(&titled, CALLABLES, false)?)
                {
                    if search_ids.contains(hit.node.id.as_str())
                        || (is_test(&hit.node.file_path) && !test_query)
                    {
                        continue;
                    }
                    if let Some((_, terms)) =
                        compound.iter_mut().find(|(node, _)| node.id == hit.node.id)
                    {
                        terms.insert(titled.clone());
                    } else {
                        compound.push((hit.node, HashSet::from([titled.clone()])));
                    }
                }
            }
            let mut compound: Vec<_> = compound
                .into_iter()
                .filter(|(_, terms)| terms.len() >= 2)
                .map(|(node, terms)| Hit {
                    node,
                    score: 10.0
                        + (terms.len() - 1) as f64 * 20.0
                        + path_score(&node.file_path)
                        + (6.0 - utf16_len(&node.name) as f64 / 8.0).max(0.0),
                })
                .collect();
            rank(&mut compound);
            results.extend(compound.into_iter().take(limit.div_ceil(2)));
        }
        rank(&mut results);
        results.truncate(limit * 3);
        results.retain(|hit| hit.score >= options.min_score);
        let mut filtered = Vec::new();
        let mut seen = HashSet::new();
        for hit in results {
            if !matches!(hit.node.kind.as_str(), "import" | "export") {
                if seen.insert(hit.node.id.as_str()) {
                    filtered.push(hit);
                }
                continue;
            }
            for edge in self
                .outgoing
                .get(hit.node.id.as_str())
                .into_iter()
                .flatten()
                .map(|i| &self.edges[*i])
                .filter(|edge| {
                    edge.kind
                        == if hit.node.kind == "import" {
                            "imports"
                        } else {
                            "exports"
                        }
                })
            {
                if let Some(node) = self.by_id.get(edge.target.as_str())
                    && seen.insert(&node.id)
                {
                    filtered.push(Hit {
                        node,
                        score: hit.score,
                    });
                }
            }
        }
        if !file_matches.is_empty() {
            let ids: HashSet<_> = file_matches
                .iter()
                .map(|hit| hit.node.id.as_str())
                .collect();
            filtered.retain(|hit| !ids.contains(hit.node.id.as_str()));
            filtered.splice(..0, file_matches);
        }
        let copy_ids: HashSet<_> = copybooks.iter().map(|h| h.node.id.as_str()).collect();
        let mut leaf_ids = HashSet::new();
        if !copybooks.is_empty() {
            let elsewhere: HashSet<_> = filtered.iter().map(|h| h.node.id.as_str()).collect();
            let mut others: Vec<_> = filtered
                .into_iter()
                .filter(|h| !copy_ids.contains(h.node.id.as_str()))
                .collect();
            let files: HashSet<_> = others.iter().map(|h| h.node.file_path.as_str()).collect();
            let mut ordered = copybooks;
            ordered.sort_by_key(|h| {
                if !include(h.node) {
                    0
                } else if files.contains(h.node.file_path.as_str()) {
                    1
                } else {
                    2
                }
            });
            if !others.is_empty() {
                ordered.truncate(limit.div_ceil(2));
            }
            for hit in &ordered {
                if !elsewhere.contains(hit.node.id.as_str()) {
                    leaf_ids.insert(hit.node.id.as_str());
                }
            }
            ordered.append(&mut others);
            filtered = ordered;
        }
        filtered.truncate(limit);
        let confidence_terms: Vec<_> = extract_search_terms(query, false)
            .into_iter()
            .filter(|t| utf16_len(t) >= 3)
            .collect();
        let distinctive: HashSet<_> = symbols
            .iter()
            .filter(|s| is_distinctive_identifier(s))
            .map(|s| s.to_lowercase())
            .collect();
        let strong = filtered.iter().any(|hit| {
            let name = hit.node.name.to_lowercase();
            let dir = directory(&hit.node.file_path).to_lowercase();
            copy_ids.contains(hit.node.id.as_str())
                || distinctive.contains(&name)
                || confidence_terms
                    .iter()
                    .filter(|t| {
                        name.contains(t.as_str()) || dir.split('/').any(|s| s == t.as_str())
                    })
                    .take(2)
                    .count()
                    >= 2
        });
        let confidence = if confidence_terms.len() >= 2 && !filtered.is_empty() && !strong {
            "low"
        } else {
            "high"
        }
        .to_string();
        self.expand(filtered, leaf_ids, options, &kinds, test_query, confidence)
    }
    fn expand(
        &self,
        entries: Vec<Hit<'a>>,
        leaves: HashSet<&str>,
        options: &FindContextOptions,
        kinds: &[String],
        test_query: bool,
        confidence: String,
    ) -> Result<ContextSubgraph> {
        let mut roots: Vec<_> = entries.iter().map(|h| h.node.id.clone()).collect();
        let mut nodes = Vec::new();
        let mut known = HashSet::new();
        let mut edges = Vec::new();
        for hit in &entries {
            push_node(&mut nodes, &mut known, hit.node);
        }
        let budget = options.max_nodes.div_ceil(4);
        let mut hierarchy_count = 0;
        for pass in 0..2 {
            let candidates: Vec<_> = if pass == 0 {
                entries.iter().map(|h| h.node).collect()
            } else {
                nodes
                    .iter()
                    .copied()
                    .filter(|n| !roots.contains(&n.id))
                    .collect()
            };
            for node in candidates {
                if hierarchy_count >= budget {
                    break;
                }
                if !TYPES.contains(&node.kind.as_str()) {
                    continue;
                }
                let (found, found_edges) = self.hierarchy(node);
                for node in found {
                    if hierarchy_count < budget && !known.contains(node.id.as_str()) {
                        push_node(&mut nodes, &mut known, node);
                        hierarchy_count += 1;
                    }
                }
                merge_edges(
                    &mut edges,
                    found_edges.into_iter().filter(|e| {
                        known.contains(e.source.as_str()) && known.contains(e.target.as_str())
                    }),
                );
            }
            if hierarchy_count == 0 {
                break;
            }
        }
        for hit in &entries {
            if leaves.contains(hit.node.id.as_str()) {
                continue;
            }
            let (found, found_edges) = self.bfs(
                hit.node,
                options.traversal_depth,
                options.max_nodes.div_ceil(entries.len().max(1)),
                &options.edge_kinds,
                kinds,
            );
            for node in found {
                push_node(&mut nodes, &mut known, node);
            }
            merge_edges(&mut edges, found_edges);
        }
        if nodes.len() > options.max_nodes {
            let mut priority: Vec<&str> = roots.iter().map(String::as_str).collect();
            let mut selected: HashSet<&str> = priority.iter().copied().collect();
            for edge in &edges {
                if selected.contains(edge.source.as_str()) && selected.insert(&edge.target) {
                    priority.push(&edge.target);
                }
                if selected.contains(edge.target.as_str()) && selected.insert(&edge.source) {
                    priority.push(&edge.source);
                }
            }
            let mut kept = Vec::new();
            let mut ids = HashSet::new();
            for id in priority {
                if kept.len() >= options.max_nodes {
                    break;
                }
                if let Some(node) = nodes.iter().find(|n| n.id == id) {
                    push_node(&mut kept, &mut ids, node);
                }
            }
            for node in nodes {
                if kept.len() >= options.max_nodes {
                    break;
                }
                push_node(&mut kept, &mut ids, node);
            }
            nodes = kept;
        }
        let cap = 5.max(options.max_nodes.div_ceil(5));
        let mut files: Vec<(&str, Vec<&Node>)> = Vec::new();
        for node in &nodes {
            if let Some((_, nodes)) = files.iter_mut().find(|(path, _)| *path == node.file_path) {
                nodes.push(node);
            } else {
                files.push((&node.file_path, vec![node]));
            }
        }
        let mut removed = HashSet::new();
        for (_, mut list) in files {
            if list.len() <= cap {
                continue;
            }
            list.sort_by_key(|n| {
                std::cmp::Reverse(
                    if roots.contains(&n.id) { 10 } else { 0 }
                        + match n.kind.as_str() {
                            "class" | "interface" | "struct" | "trait" | "protocol" | "enum" => 3,
                            "method" | "function" => 1,
                            _ => 0,
                        },
                )
            });
            for node in list.into_iter().skip(cap) {
                removed.insert(node.id.as_str());
            }
        }
        nodes.retain(|node| !removed.contains(node.id.as_str()));
        if !test_query {
            let cap = 3.max((options.max_nodes * 15).div_ceil(100));
            let mut count = 0;
            nodes.retain(|node| {
                if !is_test(&node.file_path) {
                    return true;
                }
                count += 1;
                if count > cap {
                    roots.retain(|id| id != &node.id);
                    false
                } else {
                    true
                }
            });
        }
        let kept: HashSet<_> = nodes.iter().map(|n| n.id.as_str()).collect();
        edges.retain(|e| kept.contains(e.source.as_str()) && kept.contains(e.target.as_str()));
        merge_edges(
            &mut edges,
            self.edges
                .iter()
                .filter(|e| {
                    matches!(
                        e.kind.as_str(),
                        "calls"
                            | "extends"
                            | "implements"
                            | "references"
                            | "overrides"
                            | "navigates"
                    ) && kept.contains(e.source.as_str())
                        && kept.contains(e.target.as_str())
                })
                .cloned(),
        );
        Ok(ContextSubgraph {
            nodes: nodes.into_iter().cloned().collect(),
            edges,
            roots,
            confidence,
        })
    }
    fn hierarchy(&self, start: &'a Node) -> (Vec<&'a Node>, Vec<Edge>) {
        let mut nodes = vec![start];
        let mut known = HashSet::from([start.id.as_str()]);
        let mut edges = Vec::new();
        for incoming in [false, true] {
            let adjacent = if incoming {
                &self.incoming
            } else {
                &self.outgoing
            };
            let mut stack = vec![(start.id.as_str(), 0usize)];
            let mut visited = HashSet::from([start.id.as_str()]);
            while let Some((id, next)) = stack.last_mut() {
                let Some(list) = adjacent.get(id) else {
                    stack.pop();
                    continue;
                };
                if *next >= list.len() {
                    stack.pop();
                    continue;
                }
                let edge = &self.edges[list[*next]];
                *next += 1;
                if !matches!(edge.kind.as_str(), "extends" | "implements") {
                    continue;
                }
                let next_id = if incoming {
                    edge.source.as_str()
                } else {
                    edge.target.as_str()
                };
                if let Some(node) = self.by_id.get(next_id)
                    && known.insert(next_id)
                {
                    nodes.push(*node);
                    edges.push(edge.clone());
                    if visited.insert(next_id) {
                        stack.push((next_id, 0));
                    }
                }
            }
        }
        (nodes, edges)
    }
    fn bfs(
        &self,
        start: &'a Node,
        depth: usize,
        limit: usize,
        edge_kinds: &[String],
        node_kinds: &[String],
    ) -> (Vec<&'a Node>, Vec<Edge>) {
        let mut nodes = vec![start];
        let mut edges = Vec::new();
        let mut known = HashSet::from([start.id.as_str()]);
        let mut visited = HashSet::new();
        let mut seen_edges = HashSet::new();
        let mut queue = VecDeque::from([(start, 0)]);
        while nodes.len() < limit {
            let Some((node, current_depth)) = queue.pop_front() else {
                break;
            };
            if !visited.insert(node.id.as_str()) || current_depth >= depth {
                continue;
            }
            let mut adjacent: Vec<_> = self
                .outgoing
                .get(node.id.as_str())
                .into_iter()
                .flatten()
                .chain(self.incoming.get(node.id.as_str()).into_iter().flatten())
                .map(|i| &self.edges[*i])
                .filter(|e| edge_kinds.is_empty() || edge_kinds.contains(&e.kind))
                .collect();
            adjacent.sort_by_key(|e| match e.kind.as_str() {
                "contains" => 0,
                "calls" => 1,
                _ => 2,
            });
            for edge in adjacent {
                let id = if edge.source == node.id {
                    edge.target.as_str()
                } else {
                    edge.source.as_str()
                };
                let Some(next) = self.by_id.get(id) else {
                    continue;
                };
                if !node_kinds.is_empty() && !node_kinds.contains(&next.kind) {
                    continue;
                }
                if !known.contains(id) {
                    if nodes.len() >= limit {
                        continue;
                    }
                    known.insert(id);
                    nodes.push(*next);
                    queue.push_back((next, current_depth + 1));
                }
                if seen_edges.insert((
                    edge.source.as_str(),
                    edge.target.as_str(),
                    edge.kind.as_str(),
                    edge.line,
                    edge.column,
                )) {
                    edges.push(edge.clone());
                }
            }
        }
        (nodes, edges)
    }
    fn dominant_directory(&self) -> Option<String> {
        let mut counts: HashMap<&str, usize> = HashMap::new();
        for edge in self.edges {
            if let (Some(source), Some(target)) = (
                self.by_id.get(edge.source.as_str()),
                self.by_id.get(edge.target.as_str()),
            ) && source.file_path == target.file_path
            {
                *counts.entry(&source.file_path).or_default() += 1;
            }
        }
        let mut ranked: Vec<_> = counts
            .into_iter()
            .filter(|(path, _)| {
                !is_test(path)
                    && !path
                        .split('/')
                        .any(|part| part.eq_ignore_ascii_case("__test__"))
                    && !(self.generated)(path)
                    && !prometeu_extract::is_generated_file(path)
            })
            .collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        let &(path, count) = ranked.first()?;
        if count < 20 || count < 3 * ranked.get(1).map(|r| r.1).unwrap_or(0) {
            return None;
        }
        let slash = path.rfind('/')?;
        (slash > 0).then(|| path[..=slash].to_string())
    }
    fn copybooks(&self, query: &str) -> Result<(Vec<Hit<'a>>, HashSet<String>)> {
        if !self
            .nodes
            .iter()
            .any(|n| n.kind == "file" && n.language == "cobol")
        {
            return Ok((vec![], HashSet::new()));
        }
        let regex = regex::Regex::new(r"[A-Za-z0-9$#@][A-Za-z0-9$#@-]*")?;
        let words: Vec<_> = regex
            .find_iter(query)
            .map(|m| m.as_str().trim_end_matches('-'))
            .collect();
        let lone = words.len() == 1;
        let mut tokens = Vec::new();
        let mut seen = HashSet::new();
        for token in words {
            if token.len() < 3
                || !token.chars().any(|c| c.is_ascii_alphabetic())
                || (!lone && token.chars().all(|c| c.is_ascii_lowercase()))
                || !seen.insert(token.to_uppercase())
            {
                continue;
            }
            tokens.push(token);
            if tokens.len() == 16 {
                break;
            }
        }
        let (mut members, mut entries, mut includes) = (HashSet::new(), Vec::new(), Vec::new());
        let mut ids = HashSet::new();
        for token in tokens {
            let mut uses: Vec<_> = self
                .nodes
                .iter()
                .copied()
                .filter(|n| include(n) && n.name.eq_ignore_ascii_case(token))
                .collect();
            uses.sort_by(|a, b| {
                a.file_path
                    .cmp(&b.file_path)
                    .then(a.start_line.cmp(&b.start_line))
                    .then(a.id.cmp(&b.id))
            });
            uses.truncate(200);
            let mut files: Vec<_> = self
                .nodes
                .iter()
                .copied()
                .filter(|n| {
                    if n.kind != "file" || n.language != "cobol" {
                        return false;
                    }
                    n.name
                        .to_ascii_lowercase()
                        .starts_with(&format!("{}.", token.to_ascii_lowercase()))
                        && (n.file_path.to_ascii_lowercase().ends_with(".cpy")
                            || self
                                .incoming
                                .get(n.id.as_str())
                                .into_iter()
                                .flatten()
                                .any(|i| self.edges[*i].kind == "imports"))
                })
                .collect();
            files.sort_by(|a, b| {
                a.file_path
                    .cmp(&b.file_path)
                    .then(a.start_line.cmp(&b.start_line))
                    .then(a.id.cmp(&b.id))
            });
            if uses.is_empty() && files.is_empty() {
                continue;
            }
            members.insert(token.to_uppercase());
            includes.push(uses);
            for file in files {
                let mut declarations: Vec<_> = self
                    .outgoing
                    .get(file.id.as_str())
                    .into_iter()
                    .flatten()
                    .map(|i| &self.edges[*i])
                    .filter(|e| e.kind == "contains")
                    .filter_map(|e| self.by_id.get(e.target.as_str()).copied())
                    .filter(|n| !matches!(n.kind.as_str(), "import" | "export"))
                    .collect();
                declarations.sort_by(|a, b| {
                    a.start_line
                        .cmp(&b.start_line)
                        .then_with(|| a.id.cmp(&b.id))
                });
                for node in declarations.into_iter().take(3) {
                    if ids.insert(node.id.as_str()) {
                        entries.push(Hit { node, score: 1.0 });
                    }
                }
            }
        }
        for position in 0..includes.iter().map(Vec::len).max().unwrap_or(0) {
            for uses in &includes {
                if let Some(node) = uses.get(position)
                    && ids.insert(node.id.as_str())
                {
                    entries.push(Hit { node, score: 1.0 });
                }
            }
        }
        Ok((entries, members))
    }
}
