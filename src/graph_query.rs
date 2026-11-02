//! Canonical graph traversal for embedded hosts and snapshot queries.
// Ported from the pinned MIT-licensed reference; see THIRD_PARTY_NOTICES.md.

use anyhow::{Result, ensure};
use prometeu_extract::{Edge, Node};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

const CALL_KINDS: &[&str] = &[
    "calls",
    "references",
    "imports",
    "instantiates",
    "navigates",
];
const CONTAINER_KINDS: &[&str] = &[
    "class",
    "interface",
    "struct",
    "union",
    "trait",
    "protocol",
    "module",
    "enum",
];

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    #[default]
    Outgoing,
    Incoming,
    Both,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct TraversalOptions {
    pub max_depth: usize,
    pub edge_kinds: Vec<String>,
    pub node_kinds: Vec<String>,
    pub direction: Direction,
    pub limit: usize,
    pub include_start: bool,
}

impl Default for TraversalOptions {
    fn default() -> Self {
        Self {
            max_depth: usize::MAX,
            edge_kinds: Vec::new(),
            node_kinds: Vec::new(),
            direction: Direction::Outgoing,
            limit: 1000,
            include_start: true,
        }
    }
}

impl TraversalOptions {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.limit > 0, "limit must be positive");
        ensure!(
            self.edge_kinds.len() <= 32 && self.node_kinds.len() <= 64,
            "too many graph filters"
        );
        ensure!(
            self.edge_kinds
                .iter()
                .chain(&self.node_kinds)
                .all(|kind| kind.len() <= 64),
            "graph filter exceeds 64 bytes"
        );
        Ok(())
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Subgraph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub roots: Vec<String>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Neighbor {
    pub node: Node,
    pub edge: Edge,
}

#[derive(Debug, Clone, Serialize)]
pub struct CallSiteRow {
    pub node: Node,
    pub edges: Vec<Edge>,
    pub recursive: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct PathStep {
    pub node: Node,
    pub edge: Option<Edge>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SymbolContext {
    pub focal: Node,
    pub ancestors: Vec<Node>,
    pub children: Vec<Node>,
    pub incoming_refs: Vec<Neighbor>,
    pub outgoing_refs: Vec<Neighbor>,
    pub types: Vec<Node>,
    pub imports: Vec<Node>,
}

/// Read-only index over host-owned canonical records. No source or runtime is read.
pub struct GraphSnapshot<'a> {
    nodes: &'a [Node],
    edges: &'a [Edge],
    ids: HashMap<&'a str, usize>,
    outgoing: Vec<Vec<usize>>,
    incoming: Vec<Vec<usize>>,
}

impl<'a> GraphSnapshot<'a> {
    pub fn new(nodes: &'a [Node], edges: &'a [Edge]) -> Result<Self> {
        let ids: HashMap<_, _> = nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.id.as_str(), i))
            .collect();
        ensure!(ids.len() == nodes.len(), "duplicate canonical node ID");
        let mut graph = Self {
            nodes,
            edges,
            ids,
            outgoing: vec![Vec::new(); nodes.len()],
            incoming: vec![Vec::new(); nodes.len()],
        };
        for (i, edge) in edges.iter().enumerate() {
            if let Some(&from) = graph.ids.get(edge.source.as_str()) {
                graph.outgoing[from].push(i);
            }
            if let Some(&to) = graph.ids.get(edge.target.as_str()) {
                graph.incoming[to].push(i);
            }
        }
        // Match the SQLite graph queries, including null-position ordering.
        for adjacent in &mut graph.outgoing {
            adjacent.sort_by_key(|&i| {
                let e = &edges[i];
                (&e.kind, &e.target, e.line, e.column)
            });
        }
        for adjacent in &mut graph.incoming {
            adjacent.sort_by_key(|&i| {
                let e = &edges[i];
                (&e.kind, &e.source, e.line, e.column)
            });
        }
        Ok(graph)
    }

    fn adjacent(&self, node: usize, direction: Direction, kinds: &[String]) -> Vec<usize> {
        let mut edges = Vec::new();
        if direction != Direction::Incoming {
            edges.extend(self.outgoing[node].iter().copied());
        }
        if direction != Direction::Outgoing {
            edges.extend(self.incoming[node].iter().copied());
        }
        edges.retain(|&i| kinds.is_empty() || kinds.contains(&self.edges[i].kind));
        edges
    }

    fn other(&self, edge: usize, current: usize) -> Option<usize> {
        let edge = &self.edges[edge];
        self.ids
            .get(if edge.source == self.nodes[current].id {
                edge.target.as_str()
            } else {
                edge.source.as_str()
            })
            .copied()
    }

    fn subgraph(
        &self,
        nodes: Vec<usize>,
        edges: Vec<usize>,
        root: &str,
        truncated: bool,
    ) -> Subgraph {
        Subgraph {
            nodes: nodes.into_iter().map(|i| self.nodes[i].clone()).collect(),
            edges: edges.into_iter().map(|i| self.edges[i].clone()).collect(),
            roots: vec![root.into()],
            truncated,
        }
    }

    /// Breadth-first traversal preserves parallel edges and prioritizes containment.
    pub fn breadth_first(&self, start: &str, opts: &TraversalOptions) -> Result<Subgraph> {
        opts.validate()?;
        let Some(&start_index) = self.ids.get(start) else {
            return Ok(Subgraph::default());
        };
        let mut nodes = if opts.include_start {
            vec![start_index]
        } else {
            Vec::new()
        };
        let mut kept: HashSet<_> = nodes.iter().copied().collect();
        let mut queued = HashSet::from([start_index]);
        let mut visited = HashSet::new();
        let mut seen_edges = HashSet::new();
        let mut edges = Vec::new();
        let mut queue = VecDeque::from([(start_index, 0)]);
        let mut truncated = false;
        while !queue.is_empty() && nodes.len() < opts.limit {
            let (node, depth) = queue.pop_front().unwrap();
            if !visited.insert(node) {
                continue;
            }
            if depth >= opts.max_depth {
                continue;
            }
            let mut adjacent = self.adjacent(node, opts.direction, &opts.edge_kinds);
            adjacent.sort_by_key(|&i| match self.edges[i].kind.as_str() {
                "contains" => 0,
                "calls" => 1,
                _ => 2,
            });
            for edge in adjacent {
                let Some(next) = self.other(edge, node) else {
                    continue;
                };
                if (queued.contains(&next) || visited.contains(&next)) && !kept.contains(&next) {
                    continue;
                }
                if !opts.node_kinds.is_empty() && !opts.node_kinds.contains(&self.nodes[next].kind)
                {
                    continue;
                }
                if !visited.contains(&next) && !queued.contains(&next) {
                    if nodes.len() >= opts.limit {
                        truncated = true;
                        continue;
                    }
                    queued.insert(next);
                    kept.insert(next);
                    nodes.push(next);
                    queue.push_back((next, depth + 1));
                }
                let e = &self.edges[edge];
                if seen_edges.insert((&e.source, &e.target, &e.kind, e.line, e.column)) {
                    edges.push(edge);
                }
            }
        }
        truncated |= !queue.is_empty() && nodes.len() >= opts.limit;
        Ok(self.subgraph(nodes, edges, start, truncated))
    }

    /// Iterative DFS keeps the reference visit order without growing the native stack.
    pub fn depth_first(&self, start: &str, opts: &TraversalOptions) -> Result<Subgraph> {
        opts.validate()?;
        let Some(&start_index) = self.ids.get(start) else {
            return Ok(Subgraph::default());
        };
        let mut nodes = if opts.include_start {
            vec![start_index]
        } else {
            Vec::new()
        };
        let mut kept: HashSet<_> = nodes.iter().copied().collect();
        let mut edges = Vec::new();
        let mut visited = HashSet::new();
        let mut stack = vec![(start_index, 0, None)];
        let mut truncated = false;
        while let Some((node, depth, via)) = stack.pop() {
            if visited.contains(&node) {
                continue;
            }
            if nodes.len() >= opts.limit {
                truncated = true;
                continue;
            }
            if let Some(edge) = via {
                if !opts.node_kinds.is_empty() && !opts.node_kinds.contains(&self.nodes[node].kind)
                {
                    continue;
                }
                if kept.insert(node) {
                    nodes.push(node);
                }
                edges.push(edge);
            }
            if nodes.len() >= opts.limit || depth >= opts.max_depth {
                truncated |= nodes.len() >= opts.limit;
                continue;
            }
            visited.insert(node);
            for edge in self
                .adjacent(node, opts.direction, &opts.edge_kinds)
                .into_iter()
                .rev()
            {
                if let Some(next) = self.other(edge, node) {
                    stack.push((next, depth + 1, Some(edge)));
                }
            }
        }
        Ok(self.subgraph(nodes, edges, start, truncated))
    }

    /// Caller/callee traversal re-expands a node if a shorter route reaches it later.
    pub fn calls(&self, start: &str, incoming: bool, max_depth: usize) -> Result<Vec<Neighbor>> {
        let Some(&root) = self.ids.get(start) else {
            return Ok(Vec::new());
        };
        let mut stack: Vec<(usize, usize, Option<usize>)> = vec![(root, 0, None)];
        let mut visited = HashMap::new();
        let mut reported = HashSet::from([root]);
        let mut result = Vec::new();
        while let Some((node, depth, via)) = stack.pop() {
            if visited.get(&node).is_some_and(|&old| old <= depth) {
                continue;
            }
            if reported.insert(node)
                && let Some(edge) = via
            {
                result.push(Neighbor {
                    node: self.nodes[node].clone(),
                    edge: self.edges[edge].clone(),
                });
            }
            visited.insert(node, depth);
            if depth >= max_depth {
                continue;
            }
            let adjacent = if incoming {
                &self.incoming[node]
            } else {
                &self.outgoing[node]
            };
            for &edge in adjacent.iter().rev() {
                if CALL_KINDS.contains(&self.edges[edge].kind.as_str())
                    && let Some(next) = self.other(edge, node)
                {
                    stack.push((next, depth + 1, Some(edge)));
                }
            }
        }
        Ok(result)
    }

    pub fn call_sites(&self, start: &str, incoming: bool) -> Vec<CallSiteRow> {
        let Some(&node) = self.ids.get(start) else {
            return Vec::new();
        };
        let adjacent = if incoming {
            &self.incoming[node]
        } else {
            &self.outgoing[node]
        };
        let mut rows: Vec<CallSiteRow> = Vec::new();
        let mut by_node = HashMap::new();
        let mut seen = HashSet::new();
        for &index in adjacent {
            let edge = &self.edges[index];
            if !CALL_KINDS.contains(&edge.kind.as_str()) {
                continue;
            }
            let Some(next) = self.other(index, node) else {
                continue;
            };
            if !seen.insert((
                &edge.source,
                &edge.target,
                &edge.kind,
                edge.line,
                edge.column,
            )) {
                continue;
            }
            let row = *by_node.entry(next).or_insert_with(|| {
                rows.push(CallSiteRow {
                    node: self.nodes[next].clone(),
                    edges: Vec::new(),
                    recursive: next == node,
                });
                rows.len() - 1
            });
            rows[row].edges.push(edge.clone());
        }
        rows
    }

    pub fn call_graph(&self, start: &str, depth: usize) -> Result<Subgraph> {
        let Some(&node) = self.ids.get(start) else {
            return Ok(Subgraph::default());
        };
        let mut result = Subgraph {
            nodes: vec![self.nodes[node].clone()],
            roots: vec![start.into()],
            ..Default::default()
        };
        let mut seen = HashSet::from([start.to_owned()]);
        for row in self
            .calls(start, true, depth)?
            .into_iter()
            .chain(self.calls(start, false, depth)?)
        {
            if seen.insert(row.node.id.clone()) {
                result.nodes.push(row.node);
            }
            result.edges.push(row.edge);
        }
        Ok(result)
    }

    pub fn usages(&self, start: &str) -> Vec<Neighbor> {
        self.ids
            .get(start)
            .into_iter()
            .flat_map(|&node| self.incoming[node].iter())
            .filter_map(|&edge| {
                Some(Neighbor {
                    node: self.nodes[*self.ids.get(self.edges[edge].source.as_str())?].clone(),
                    edge: self.edges[edge].clone(),
                })
            })
            .collect()
    }

    pub fn ancestors(&self, start: &str) -> Vec<Node> {
        let mut current = self.ids.get(start).copied();
        let mut seen = HashSet::new();
        let mut result = Vec::new();
        while let Some(node) = current {
            if !seen.insert(node) {
                break;
            }
            current = self.incoming[node]
                .iter()
                .find(|&&i| self.edges[i].kind == "contains")
                .and_then(|&i| self.ids.get(self.edges[i].source.as_str()).copied());
            if let Some(parent) = current {
                result.push(self.nodes[parent].clone());
            }
        }
        result
    }

    pub fn children(&self, start: &str) -> Vec<Node> {
        self.ids
            .get(start)
            .into_iter()
            .flat_map(|&node| self.outgoing[node].iter())
            .filter(|&&edge| self.edges[edge].kind == "contains")
            .filter_map(|&edge| {
                self.ids
                    .get(self.edges[edge].target.as_str())
                    .map(|&i| self.nodes[i].clone())
            })
            .collect()
    }

    pub fn type_graph(&self, start: &str) -> Subgraph {
        let Some(&root) = self.ids.get(start) else {
            return Subgraph::default();
        };
        let mut nodes = vec![root];
        let mut seen = HashSet::from([root]);
        let mut edges = Vec::new();
        for incoming in [false, true] {
            let mut stack = vec![(root, None)];
            while let Some((node, via)) = stack.pop() {
                if let Some(edge) = via {
                    if !seen.insert(node) {
                        continue;
                    }
                    nodes.push(node);
                    edges.push(edge);
                }
                for &edge in (if incoming {
                    &self.incoming[node]
                } else {
                    &self.outgoing[node]
                })
                .iter()
                .rev()
                {
                    if matches!(self.edges[edge].kind.as_str(), "extends" | "implements")
                        && let Some(next) = self.other(edge, node)
                        && !seen.contains(&next)
                    {
                        stack.push((next, Some(edge)));
                    }
                }
            }
        }
        self.subgraph(nodes, edges, start, false)
    }

    pub fn find_path(&self, from: &str, to: &str, kinds: &[String]) -> Option<Vec<PathStep>> {
        let (&start, &target) = (self.ids.get(from)?, self.ids.get(to)?);
        let mut queue = VecDeque::from([start]);
        let mut seen = HashSet::from([start]);
        let mut parents = HashMap::<usize, (usize, usize)>::new();
        while let Some(node) = queue.pop_front() {
            if node == target {
                let mut result = Vec::new();
                let mut current = target;
                while current != start {
                    let &(parent, edge) = parents.get(&current)?;
                    result.push(PathStep {
                        node: self.nodes[current].clone(),
                        edge: Some(self.edges[edge].clone()),
                    });
                    current = parent;
                }
                result.push(PathStep {
                    node: self.nodes[start].clone(),
                    edge: None,
                });
                result.reverse();
                return Some(result);
            }
            for &edge in &self.outgoing[node] {
                if !kinds.is_empty() && !kinds.contains(&self.edges[edge].kind) {
                    continue;
                }
                if let Some(&next) = self.ids.get(self.edges[edge].target.as_str())
                    && seen.insert(next)
                {
                    parents.insert(next, (node, edge));
                    queue.push_back(next);
                }
            }
        }
        None
    }

    pub fn symbol_context(&self, start: &str) -> Option<SymbolContext> {
        let &node = self.ids.get(start)?;
        let ancestors = self.ancestors(start);
        let children = self.children(start);
        let incoming_refs = self
            .usages(start)
            .into_iter()
            .filter(|r| r.edge.kind != "contains")
            .collect();
        let outgoing_refs = self.outgoing[node]
            .iter()
            .filter(|&&i| self.edges[i].kind != "contains")
            .filter_map(|&i| {
                Some(Neighbor {
                    node: self.nodes[*self.ids.get(self.edges[i].target.as_str())?].clone(),
                    edge: self.edges[i].clone(),
                })
            })
            .collect();
        let mut types = Vec::new();
        let mut seen = HashSet::new();
        for kind in ["type_of", "returns"] {
            for &edge in &self.outgoing[node] {
                if self.edges[edge].kind == kind
                    && let Some(&next) = self.ids.get(self.edges[edge].target.as_str())
                    && seen.insert(next)
                {
                    types.push(self.nodes[next].clone());
                }
            }
        }
        let imports = ancestors
            .iter()
            .find(|n| n.kind == "file")
            .into_iter()
            .flat_map(|file| self.outgoing[self.ids[file.id.as_str()]].iter())
            .filter(|&&i| self.edges[i].kind == "imports")
            .filter_map(|&i| {
                self.ids
                    .get(self.edges[i].target.as_str())
                    .map(|&i| self.nodes[i].clone())
            })
            .collect();
        Some(SymbolContext {
            focal: self.nodes[node].clone(),
            ancestors,
            children,
            incoming_refs,
            outgoing_refs,
            types,
            imports,
        })
    }

    pub fn file_dependencies(&self, path: &str, incoming: bool) -> Vec<String> {
        let mut result = Vec::new();
        let mut seen = HashSet::new();
        for edge in self.edges {
            if edge.kind == "contains" {
                continue;
            }
            let (Some(&from), Some(&to)) = (
                self.ids.get(edge.source.as_str()),
                self.ids.get(edge.target.as_str()),
            ) else {
                continue;
            };
            let (from, to) = if incoming { (to, from) } else { (from, to) };
            if self.nodes[from].file_path == path
                && self.nodes[to].file_path != path
                && seen.insert(&self.nodes[to].file_path)
            {
                result.push(self.nodes[to].file_path.clone());
            }
        }
        result
    }

    /// Reverse dependencies, including members of every encountered container.
    pub fn impact(
        &self,
        start: &str,
        max_depth: usize,
        max_nodes: usize,
        max_edges: usize,
    ) -> Result<Subgraph> {
        ensure!(max_nodes > 0, "impact node limit must be positive");
        let Some(&root) = self.ids.get(start) else {
            return Ok(Subgraph::default());
        };
        enum Step {
            Enter(usize, usize),
            Child(usize, usize),
            Incoming(usize, usize, bool),
            Dependency(usize, usize, bool),
        }
        let mut nodes = vec![root];
        let mut kept = HashSet::from([root]);
        let mut edges = Vec::new();
        let mut visited = HashMap::new();
        let mut expanded = HashSet::new();
        let mut truncated = false;
        let mut stack = vec![Step::Enter(root, 0)];
        while let Some(step) = stack.pop() {
            match step {
                Step::Enter(node, depth) => {
                    if visited.get(&node).is_some_and(|&old| old <= depth) {
                        continue;
                    }
                    visited.insert(node, depth);
                    if depth >= max_depth {
                        continue;
                    }
                    let first = expanded.insert(node);
                    stack.push(Step::Incoming(node, depth, first));
                    if CONTAINER_KINDS.contains(&self.nodes[node].kind.as_str()) {
                        let remaining = max_edges.saturating_sub(edges.len());
                        let selected: Vec<_> = self.outgoing[node]
                            .iter()
                            .copied()
                            .filter(|&e| self.edges[e].kind == "contains")
                            .take(remaining.saturating_add(1))
                            .collect();
                        truncated |= selected.len() > remaining;
                        for &edge in selected.iter().take(remaining).rev() {
                            stack.push(Step::Child(edge, depth));
                        }
                    }
                }
                Step::Child(edge, depth) => {
                    let Some(&node) = self.ids.get(self.edges[edge].target.as_str()) else {
                        continue;
                    };
                    if visited.get(&node).is_some_and(|&old| old <= depth) {
                        continue;
                    }
                    if !kept.contains(&node) {
                        if nodes.len() >= max_nodes || edges.len() >= max_edges {
                            truncated = true;
                            continue;
                        }
                        kept.insert(node);
                        nodes.push(node);
                        edges.push(edge);
                    }
                    stack.push(Step::Enter(node, depth));
                }
                Step::Incoming(node, depth, first) => {
                    let remaining = max_edges.saturating_sub(edges.len());
                    let selected: Vec<_> = self.incoming[node]
                        .iter()
                        .copied()
                        .filter(|&e| self.edges[e].kind != "contains")
                        .take(remaining.saturating_add(1))
                        .collect();
                    truncated |= selected.len() > remaining;
                    for &edge in selected.iter().take(remaining).rev() {
                        stack.push(Step::Dependency(edge, depth, first));
                    }
                }
                Step::Dependency(edge, depth, first) => {
                    let Some(&node) = self.ids.get(self.edges[edge].source.as_str()) else {
                        continue;
                    };
                    if !kept.contains(&node) && nodes.len() >= max_nodes || edges.len() >= max_edges
                    {
                        truncated = true;
                        continue;
                    }
                    if first {
                        edges.push(edge);
                    }
                    if visited.get(&node).is_none_or(|&old| depth + 1 < old) {
                        if kept.insert(node) {
                            nodes.push(node);
                        }
                        stack.push(Step::Enter(node, depth + 1));
                    }
                }
            }
        }
        Ok(self.subgraph(nodes, edges, start, truncated))
    }

    pub fn qualified_names(&self, pattern: &str) -> Result<Vec<Node>> {
        ensure!(
            pattern.len() <= 1024,
            "qualified-name pattern exceeds 1024 bytes"
        );
        let mut regex = String::from("^");
        for ch in pattern.chars() {
            match ch {
                '*' => regex.push_str(".*"),
                '?' => regex.push('.'),
                _ => regex.push_str(&regex::escape(&ch.to_string())),
            }
        }
        regex.push('$');
        let matcher = regex::Regex::new(&regex)?;
        let mut result = Vec::new();
        for kind in [
            "class",
            "union",
            "function",
            "method",
            "interface",
            "type_alias",
            "variable",
            "constant",
        ] {
            let mut matches = self
                .nodes
                .iter()
                .filter(|node| node.kind == kind && matcher.is_match(&node.qualified_name))
                .collect::<Vec<_>>();
            matches.sort_by_key(|n| (&n.file_path, n.start_line, &n.id));
            result.extend(matches.into_iter().cloned());
        }
        Ok(result)
    }

    pub fn filtered(&self, mut filter: impl FnMut(&Node) -> bool, include_edges: bool) -> Subgraph {
        let mut nodes = Vec::new();
        let mut seen = HashSet::new();
        let kinds = [
            "file",
            "module",
            "class",
            "struct",
            "union",
            "interface",
            "trait",
            "function",
            "method",
            "variable",
            "constant",
            "enum",
            "type_alias",
        ];
        for kind in kinds {
            let mut matches = self
                .nodes
                .iter()
                .enumerate()
                .filter(|(_, n)| n.kind == kind)
                .collect::<Vec<_>>();
            matches.sort_by_key(|(_, n)| (&n.file_path, n.start_line, &n.id));
            for (i, node) in matches {
                if filter(node) && seen.insert(i) {
                    nodes.push(i);
                }
            }
        }
        for (i, node) in self.nodes.iter().enumerate() {
            if !kinds.contains(&node.kind.as_str()) && filter(node) && seen.insert(i) {
                nodes.push(i);
            }
        }
        let edges = if include_edges {
            nodes
                .iter()
                .flat_map(|&n| self.outgoing[n].iter().copied())
                .filter(|&i| {
                    self.ids
                        .get(self.edges[i].target.as_str())
                        .is_some_and(|n| seen.contains(n))
                })
                .collect()
        } else {
            Vec::new()
        };
        let mut graph = self.subgraph(nodes, edges, "", false);
        graph.roots.clear();
        graph
    }

    pub fn exports(&self, path: &str) -> Vec<Node> {
        let mut nodes = self
            .nodes
            .iter()
            .filter(|n| n.file_path == path && n.is_exported == Some(true))
            .cloned()
            .collect::<Vec<_>>();
        nodes.sort_by(|a, b| (a.start_line, &a.id).cmp(&(b.start_line, &b.id)));
        nodes
    }

    pub fn module_structure(&self) -> BTreeMap<String, Vec<String>> {
        let mut result = BTreeMap::<String, Vec<String>>::new();
        let mut seen = HashSet::new();
        for node in self.nodes {
            if seen.insert(&node.file_path) {
                let dir = node.file_path.rsplit_once('/').map_or(".", |(dir, _)| dir);
                result
                    .entry(dir.into())
                    .or_default()
                    .push(node.file_path.clone());
            }
        }
        for files in result.values_mut() {
            files.sort();
        }
        result
    }
}
