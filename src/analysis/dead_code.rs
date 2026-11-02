//! Conservative unreferenced-symbol report, ported from the pinned reference.
//! SPDX-License-Identifier: MIT. Provenance: crates/prometeu-extract/UPSTREAM.md.
use super::super::{Graph, with_graph};
use crate::{Engine, classify, index::validate_limits};
use anyhow::{Result, ensure};
use prometeu_extract::{Edge, Node, UnresolvedReference};
use serde::{Deserialize, Serialize};
use std::cell::Cell;
use std::collections::{BTreeMap, HashMap, HashSet};

const KINDS: &[&str] = &[
    "function",
    "method",
    "class",
    "component",
    "interface",
    "struct",
    "trait",
    "protocol",
    "enum",
    "union",
    "type_alias",
];
const VALUE_KINDS: &[&str] = &[
    "variable",
    "constant",
    "property",
    "field",
    "enum_member",
    "namespace",
    "module",
];
const CONTAINERS: &[&str] = &[
    "class",
    "interface",
    "struct",
    "trait",
    "protocol",
    "enum",
    "union",
    "type_alias",
];
const MEMBERS: &[&str] = &["method", "function", "property", "field"];
const MAX_CANDIDATES: usize = 20_000;
const MAX_FILES: usize = 600;
const MAX_SOURCE_BYTES: usize = 2_000_000;
const MAX_WORK: usize = 10_000_000;
const MAX_MENTION_BYTES: usize = 128 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DeadCodeOptions {
    pub kinds: Vec<String>,
    pub include_exported: bool,
    pub include_tests: bool,
    pub include_generated: bool,
    pub limit: usize,
    pub max_bytes: usize,
}

impl Default for DeadCodeOptions {
    fn default() -> Self {
        Self {
            kinds: Vec::new(),
            include_exported: false,
            include_tests: false,
            include_generated: false,
            limit: 200,
            max_bytes: usize::MAX,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct DeadCodeEntry {
    pub node: Node,
    pub members: Vec<Node>,
    pub lines: u32,
    pub exported: bool,
}

#[derive(Default, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeadCodeExclusions {
    pub tests: usize,
    pub generated: usize,
    pub exported: usize,
    pub exports_unknown: usize,
    pub declarations: usize,
    pub decorated: usize,
    pub overriding: usize,
    pub hooks: usize,
    pub implicit: usize,
    pub vendored: usize,
    pub test_scope: usize,
    pub markup: usize,
    pub unreachable_file: usize,
    pub unresolved_name: usize,
    pub ambiguous_name: usize,
    pub mentioned: usize,
    pub unreadable: usize,
    pub nested: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeadCodeReport {
    pub entries: Vec<DeadCodeEntry>,
    pub total: usize,
    pub candidates: usize,
    pub excluded: DeadCodeExclusions,
    pub kinds: Vec<String>,
    pub include_exported: bool,
    pub bounded: bool,
    pub corroborated: bool,
}

#[derive(Debug, Serialize)]
pub struct DeadCodeResult {
    pub generation: u64,
    #[serde(flatten)]
    pub report: DeadCodeReport,
    pub truncated: bool,
}

impl Engine {
    /// Report unreferenced candidates after conservative exclusions and source corroboration.
    pub fn dead_code(&self, options: &DeadCodeOptions) -> Result<DeadCodeResult> {
        validate_limits(options.limit, options.max_bytes)?;
        ensure!(
            options.kinds.len() <= 32 && options.kinds.iter().all(|kind| kind.len() <= 64),
            "too many or oversized symbol kinds"
        );
        with_graph(self, |graph, _| {
            // Missing graph records invalidate a negative assertion. Refuse a partial graph.
            ensure!(
                !graph.truncated && !graph.resolution_truncated.get(),
                "dead-code analysis requires a complete bounded graph"
            );
            let input = Input::from_graph(graph)?;
            let report = report(&input, options, true)?;
            let mut result = DeadCodeResult {
                generation: graph.generation,
                truncated: report.bounded || report.total > report.entries.len(),
                report,
            };
            while serde_json::to_vec(&result)?.len() > options.max_bytes {
                ensure!(
                    !result.report.entries.is_empty(),
                    "response budget is too small for dead-code report metadata"
                );
                result.report.entries.pop();
                result.truncated = true;
            }
            Ok(result)
        })
    }
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Input {
    nodes: Vec<Node>,
    edges: Vec<Edge>,
    unresolved: Vec<UnresolvedReference>,
    sources: BTreeMap<String, String>,
    generated: HashSet<String>,
}

impl Input {
    fn from_graph(graph: &Graph) -> Result<Self> {
        let mut input = Self::default();
        let mut ids = HashMap::new();
        for (index, node) in graph.nodes.iter().enumerate() {
            let file = &graph.files[&node.path];
            if let Some(canonical) = file
                .analysis
                .extraction
                .as_ref()
                .and_then(|extraction| extraction.nodes.get(node.ordinal))
            {
                ids.insert(index, canonical.id.clone());
                input.nodes.push(canonical.clone());
            } else {
                // Legacy-only projections can call canonical symbols. Keep their
                // edges as opaque file-level evidence, without inventing candidates.
                let id = format!("projection:{}", node.id);
                ids.insert(index, id.clone());
                input.nodes.push(Node {
                    id,
                    kind: "file".into(),
                    name: node.symbol.name.clone(),
                    qualified_name: node.symbol.qualified_name.clone(),
                    file_path: node.path.clone(),
                    language: file.analysis.language.clone(),
                    start_line: node.symbol.start_line as u32,
                    end_line: node.symbol.end_line as u32,
                    start_column: 0,
                    end_column: 0,
                    updated_at: 0,
                    docstring: None,
                    signature: None,
                    visibility: None,
                    is_exported: Some(false),
                    is_async: None,
                    is_static: None,
                    is_abstract: None,
                    decorators: None,
                    type_parameters: None,
                    return_type: None,
                    extra: BTreeMap::new(),
                });
            }
        }
        for (path, file) in &graph.files {
            input.sources.insert(path.clone(), file.source.clone());
            if classify::generated_file(path, &file.source)? {
                input.generated.insert(path.clone());
            }
        }
        for edge in &graph.edges {
            let Some(source) = ids.get(&edge.from) else {
                continue;
            };
            if let Some(target) = edge.to.and_then(|target| ids.get(&target)) {
                input.edges.push(Edge {
                    source: source.clone(),
                    target: target.clone(),
                    kind: edge.kind.clone(),
                    line: Some(edge.line as u32),
                    column: None,
                    provenance: None,
                    metadata: None,
                });
            } else {
                input.unresolved.push(UnresolvedReference {
                    from_node_id: source.clone(),
                    reference_name: edge.target.clone(),
                    reference_kind: edge.kind.clone(),
                    line: edge.line as u32,
                    column: 0,
                    file_path: None,
                    language: None,
                    candidates: None,
                });
            }
        }
        Ok(input)
    }
}

struct View<'a> {
    input: &'a Input,
    ids: HashMap<&'a str, usize>,
    outgoing: Vec<Vec<usize>>,
    incoming: Vec<Vec<usize>>,
    refs: Vec<Vec<usize>>,
    dependents: BTreeMap<&'a str, Vec<&'a str>>,
    work: Cell<usize>,
}

impl<'a> View<'a> {
    fn new(input: &'a Input) -> Self {
        let ids: HashMap<_, _> = input
            .nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.id.as_str(), i))
            .collect();
        let mut view = Self {
            input,
            ids,
            outgoing: vec![Vec::new(); input.nodes.len()],
            incoming: vec![Vec::new(); input.nodes.len()],
            refs: vec![Vec::new(); input.nodes.len()],
            dependents: BTreeMap::new(),
            work: Cell::new(0),
        };
        for (index, edge) in input.edges.iter().enumerate() {
            if let (Some(&from), Some(&to)) = (
                view.ids.get(edge.source.as_str()),
                view.ids.get(edge.target.as_str()),
            ) {
                view.outgoing[from].push(index);
                view.incoming[to].push(index);
                let source = &input.nodes[from].file_path;
                let target = &input.nodes[to].file_path;
                if source != target && edge.kind != "contains" {
                    view.dependents.entry(target).or_default().push(source);
                }
            }
        }
        for dependents in view.dependents.values_mut() {
            dependents.sort();
            dependents.dedup();
        }
        for (index, reference) in input.unresolved.iter().enumerate() {
            if let Some(&from) = view.ids.get(reference.from_node_id.as_str()) {
                view.refs[from].push(index);
            }
        }
        view
    }
    fn step(&self) -> Result<()> {
        let work = self.work.get() + 1;
        self.work.set(work);
        ensure!(work <= MAX_WORK, "dead-code graph work limit exceeded");
        Ok(())
    }
    fn outgoing(&self, index: usize) -> impl Iterator<Item = &Edge> {
        self.outgoing[index]
            .iter()
            .map(|&edge| &self.input.edges[edge])
    }
    fn references(&self, index: usize) -> impl Iterator<Item = &UnresolvedReference> {
        self.refs[index]
            .iter()
            .map(|&reference| &self.input.unresolved[reference])
    }
    fn decorated(&self, index: usize) -> bool {
        self.input.nodes[index]
            .decorators
            .as_ref()
            .is_some_and(|decorators| !decorators.is_empty())
            || self.outgoing(index).any(|edge| edge.kind == "decorates")
            || self
                .references(index)
                .any(|reference| reference.reference_kind == "decorates")
    }
    fn container(&self, index: usize) -> Option<usize> {
        if !MEMBERS.contains(&self.input.nodes[index].kind.as_str()) {
            return None;
        }
        let edge = self.incoming[index]
            .iter()
            .map(|&edge| &self.input.edges[edge])
            .find(|edge| edge.kind == "contains")?;
        let container = *self.ids.get(edge.source.as_str())?;
        CONTAINERS
            .contains(&self.input.nodes[container].kind.as_str())
            .then_some(container)
    }
    fn ancestors(
        &self,
        roots: &HashSet<usize>,
        kinds: &[&str],
    ) -> Result<HashMap<usize, HashSet<usize>>> {
        let mut reach: HashMap<usize, HashSet<usize>> = HashMap::new();
        let mut seen: HashMap<usize, HashSet<usize>> = roots
            .iter()
            .map(|&root| (root, HashSet::from([root])))
            .collect();
        let mut frontier = seen.clone();
        for _ in 0..8 {
            if frontier.is_empty() {
                break;
            }
            let mut next: HashMap<usize, HashSet<usize>> = HashMap::new();
            for (node, roots) in frontier {
                for edge in self
                    .outgoing(node)
                    .filter(|edge| kinds.contains(&edge.kind.as_str()))
                {
                    let Some(&target) = self.ids.get(edge.target.as_str()) else {
                        continue;
                    };
                    if target == node {
                        continue;
                    }
                    for &root in &roots {
                        self.step()?;
                        if seen.entry(target).or_default().insert(root) {
                            next.entry(target).or_default().insert(root);
                            reach.entry(target).or_default().insert(root);
                        }
                    }
                }
            }
            frontier = next;
        }
        Ok(reach)
    }
    fn overrides(
        &self,
        candidates: &[usize],
        containers: &HashMap<usize, usize>,
    ) -> Result<HashSet<usize>> {
        let roots: HashSet<_> = containers.values().copied().collect();
        let ancestors = self.ancestors(&roots, &["extends", "implements"])?;
        let external: HashSet<_> = roots
            .iter()
            .chain(ancestors.keys())
            .copied()
            .filter(|&node| {
                self.references(node).any(|reference| {
                    matches!(reference.reference_kind.as_str(), "extends" | "implements")
                })
            })
            .collect();
        let mut opaque: HashSet<_> = roots.intersection(&external).copied().collect();
        let mut names: HashMap<usize, HashSet<&str>> = HashMap::new();
        for (ancestor, descendants) in ancestors {
            let declared: Vec<_> = self
                .outgoing(ancestor)
                .filter(|edge| edge.kind == "contains")
                .filter_map(|edge| self.ids.get(edge.target.as_str()))
                .map(|&index| &self.input.nodes[index])
                .filter(|node| MEMBERS.contains(&node.kind.as_str()))
                .map(|node| node.name.as_str())
                .collect();
            for root in descendants {
                self.step()?;
                if external.contains(&ancestor) || declared.is_empty() {
                    opaque.insert(root);
                }
                names
                    .entry(root)
                    .or_default()
                    .extend(declared.iter().copied());
            }
        }
        Ok(candidates
            .iter()
            .copied()
            .filter(|index| {
                containers.get(index).is_some_and(|container| {
                    opaque.contains(container)
                        || names.get(container).is_some_and(|names| {
                            names.contains(self.input.nodes[*index].name.as_str())
                        })
                })
            })
            .collect())
    }
}

fn report(input: &Input, options: &DeadCodeOptions, corroborate: bool) -> Result<DeadCodeReport> {
    let view = View::new(input);
    let mut seen_kinds = HashSet::new();
    let mut kinds: Vec<_> = options
        .kinds
        .iter()
        .filter(|kind| KINDS.contains(&kind.as_str()) || VALUE_KINDS.contains(&kind.as_str()))
        .filter(|kind| seen_kinds.insert(kind.as_str()))
        .cloned()
        .collect();
    if kinds.is_empty() {
        kinds = KINDS.iter().map(|kind| (*kind).to_string()).collect();
    }
    let mut raw: Vec<_> = input
        .nodes
        .iter()
        .enumerate()
        .filter(|(index, node)| {
            kinds.contains(&node.kind)
                && !view.incoming[*index].iter().any(|&edge| {
                    let edge = &input.edges[edge];
                    edge.kind != "contains" && edge.source != node.id
                })
        })
        .map(|(index, _)| index)
        .collect();
    raw.sort_by(|&a, &b| {
        let (a, b) = (&input.nodes[a], &input.nodes[b]);
        a.file_path
            .cmp(&b.file_path)
            .then(a.start_line.cmp(&b.start_line))
            .then(a.name.cmp(&b.name))
    });
    let bounded = raw.len() > MAX_CANDIDATES;
    raw.truncate(MAX_CANDIDATES);
    let exported_languages: HashSet<_> = input
        .nodes
        .iter()
        .filter(|node| node.is_exported == Some(true))
        .map(|node| node.language.as_str())
        .collect();
    let mut excluded = DeadCodeExclusions::default();
    let mut surviving = Vec::new();
    for &index in &raw {
        let node = &input.nodes[index];
        let reason = if !options.include_tests && classify::is_test_file(&node.file_path) {
            Some(&mut excluded.tests)
        } else if !options.include_generated && input.generated.contains(&node.file_path) {
            Some(&mut excluded.generated)
        } else if !options.include_exported
            && (node.is_exported == Some(true) || header(&node.file_path))
        {
            Some(&mut excluded.exported)
        } else if !options.include_exported && !exported_languages.contains(node.language.as_str())
        {
            Some(&mut excluded.exports_unknown)
        } else if node.is_abstract == Some(true) {
            Some(&mut excluded.declarations)
        } else if implicit(&node.name) {
            Some(&mut excluded.implicit)
        } else if vendored(&node.file_path) {
            Some(&mut excluded.vendored)
        } else if !options.include_tests && test_scope(&node.qualified_name) {
            Some(&mut excluded.test_scope)
        } else if [
            "svelte",
            "vue",
            "astro",
            "liquid",
            "html",
            "razor",
            "twig",
            "blade",
            "erb",
            "handlebars",
        ]
        .contains(&node.language.as_str())
        {
            Some(&mut excluded.markup)
        } else {
            None
        };
        if let Some(count) = reason {
            *count += 1;
        } else {
            surviving.push(index);
        }
    }
    let decorated: HashSet<_> = surviving
        .iter()
        .copied()
        .filter(|&index| view.decorated(index))
        .collect();
    let containers: HashMap<_, _> = surviving
        .iter()
        .filter_map(|&index| view.container(index).map(|container| (index, container)))
        .collect();
    let overriding = view.overrides(&surviving, &containers)?;
    surviving.retain(|&index| {
        if view
            .dependents
            .contains_key(input.nodes[index].file_path.as_str())
        {
            true
        } else {
            excluded.unreachable_file += 1;
            false
        }
    });
    let mut unresolved = HashSet::new();
    for reference in &input.unresolved {
        unresolved.insert(reference.reference_name.clone());
        unresolved.insert(reference_tail(
            &reference.reference_name,
            &reference.reference_kind,
        ));
    }
    let mut counts: HashMap<&str, (usize, bool)> = HashMap::new();
    for (index, node) in input.nodes.iter().enumerate() {
        let count = counts.entry(&node.name).or_default();
        count.0 += 1;
        count.1 |= view.incoming[index]
            .iter()
            .any(|&edge| input.edges[edge].kind != "contains");
    }
    let eligible: Vec<_> = surviving
        .iter()
        .copied()
        .filter(|index| !decorated.contains(index) && !overriding.contains(index))
        .collect();
    let hooks = hooks(&view, &eligible, &containers, &raw)?;
    let mut kept = Vec::new();
    for index in surviving {
        let node = &input.nodes[index];
        let reason = if decorated.contains(&index) {
            Some(&mut excluded.decorated)
        } else if containers.get(&index).is_some_and(|&container| {
            ["interface", "trait", "protocol"].contains(&input.nodes[container].kind.as_str())
        }) {
            Some(&mut excluded.declarations)
        } else if overriding.contains(&index) {
            Some(&mut excluded.overriding)
        } else if hooks.contains(&index) {
            Some(&mut excluded.hooks)
        } else if unresolved.contains(node.name.as_str()) {
            Some(&mut excluded.unresolved_name)
        } else if counts
            .get(node.name.as_str())
            .is_some_and(|&(count, referenced)| count > 1 && referenced)
        {
            Some(&mut excluded.ambiguous_name)
        } else {
            None
        };
        if let Some(count) = reason {
            *count += 1;
        } else {
            kept.push(index);
        }
    }
    let mut confirmed = Vec::new();
    let mut read: HashMap<&str, Option<&str>> = HashMap::new();
    let mut bytes = 0usize;
    for index in kept {
        if !corroborate {
            confirmed.push(index);
            continue;
        }
        let node = &input.nodes[index];
        let mut readable = true;
        let mut mentions = 0;
        for path in std::iter::once(node.file_path.as_str()).chain(
            view.dependents
                .get(node.file_path.as_str())
                .into_iter()
                .flatten()
                .copied(),
        ) {
            let read_count = read.len();
            let source = read.entry(path).or_insert_with(|| {
                if read_count >= MAX_FILES {
                    None
                } else {
                    input
                        .sources
                        .get(path)
                        .filter(|source| source.len() <= MAX_SOURCE_BYTES)
                        .map(String::as_str)
                }
            });
            if let Some(source) = source {
                bytes = bytes.saturating_add(source.len());
                ensure!(
                    bytes <= MAX_MENTION_BYTES,
                    "dead-code source corroboration work limit exceeded"
                );
                mentions += mention_count(source, &node.name, 2 - mentions);
                if mentions >= 2 {
                    break;
                }
            } else {
                readable = false;
                break;
            }
        }
        if !readable {
            excluded.unreadable += 1;
        } else if mentions >= 2 {
            excluded.mentioned += 1;
        } else {
            confirmed.push(index);
        }
    }
    let kept: HashSet<_> = confirmed.iter().copied().collect();
    let mut entries = BTreeMap::new();
    let mut pending = Vec::new();
    for index in confirmed {
        if let Some(&container) = containers
            .get(&index)
            .filter(|container| kept.contains(container))
        {
            excluded.nested += 1;
            pending.push((index, container));
            continue;
        }
        let node = &input.nodes[index];
        entries.insert(
            index,
            DeadCodeEntry {
                node: stored_node(node),
                members: Vec::new(),
                lines: node.end_line.saturating_sub(node.start_line) + 1,
                exported: node.is_exported == Some(true),
            },
        );
    }
    for (index, container) in pending {
        if let Some(entry) = entries.get_mut(&container) {
            entry.members.push(stored_node(&input.nodes[index]));
        }
    }
    let mut entries: Vec<_> = entries.into_values().collect();
    for entry in &mut entries {
        entry
            .members
            .sort_by(|a, b| a.start_line.cmp(&b.start_line).then(a.name.cmp(&b.name)));
    }
    entries.sort_by(|a, b| {
        b.lines
            .cmp(&a.lines)
            .then(a.node.file_path.cmp(&b.node.file_path))
            .then(a.node.start_line.cmp(&b.node.start_line))
    });
    let total = entries.len();
    entries.truncate(options.limit.max(1));
    Ok(DeadCodeReport {
        entries,
        total,
        candidates: raw.len(),
        excluded,
        kinds,
        include_exported: options.include_exported,
        bounded,
        corroborated: corroborate,
    })
}

fn stored_node(node: &Node) -> Node {
    let mut node = node.clone();
    // The reference report reads SQLite nodes, whose absent flags become false
    // and whose absent visibility remains JSON null. Raw extraction stays intact.
    node.is_exported.get_or_insert(false);
    node.is_abstract.get_or_insert(false);
    node.is_async.get_or_insert(false);
    node.is_static.get_or_insert(false);
    if node.visibility.is_none() {
        node.extra
            .insert("visibility".into(), serde_json::Value::Null);
    }
    node
}

fn reference_tail(name: &str, kind: &str) -> String {
    fn stem(path: &str) -> &str {
        let name = path
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or(path);
        let end = name
            .char_indices()
            .skip(1)
            .find(|(_, ch)| *ch == '.')
            .map_or(name.len(), |(index, _)| index);
        let stem = &name[..end];
        if stem.chars().all(|ch| ch == '.') {
            ""
        } else {
            stem
        }
    }
    if kind == "imports" && name.trim_end_matches('/').contains('/') {
        let value = stem(name);
        if !value.is_empty() {
            return value.to_string();
        }
    }
    if matches!(kind, "references" | "calls") {
        let route = name.strip_prefix("layout:").unwrap_or(name);
        let module = route
            .strip_prefix("lazy-import:")
            .filter(|path| !path.is_empty())
            .or_else(|| {
                route
                    .strip_prefix("import:")
                    .and_then(|path| path.rsplit_once('#'))
                    .filter(|(_, export)| !export.is_empty())
                    .map(|(path, _)| path)
            });
        if let Some(module) = module {
            let value = stem(module);
            if !value.is_empty() {
                return format!("module:{value}");
            }
        }
    }
    if kind == "references"
        && let Some((_, file)) = name.rsplit_once('/')
    {
        static FILE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
            regex::Regex::new(r"^[A-Za-z0-9_$@+~.\-]+\.[A-Za-z][A-Za-z0-9]*$").unwrap()
        });
        if FILE.is_match(file) {
            return file.to_string();
        }
    }
    let base = name
        .rsplit_once('/')
        .filter(|(base, arity)| {
            !base.is_empty()
                && (1..=3).contains(&arity.len())
                && arity.bytes().all(|ch| ch.is_ascii_digit())
        })
        .map_or(name, |(base, _)| base);
    base.rsplit(['.', ':']).next().unwrap_or(base).to_string()
}

fn hooks(
    view: &View<'_>,
    nodes: &[usize],
    containers: &HashMap<usize, usize>,
    unreferenced: &[usize],
) -> Result<HashSet<usize>> {
    let shaped = |index: usize| {
        let node = &view.input.nodes[index];
        ["method", "function"].contains(&node.kind.as_str())
            && ["typescript", "tsx"].contains(&node.language.as_str())
    };
    let eligible: Vec<_> = nodes
        .iter()
        .copied()
        .filter(|&index| {
            shaped(index)
                && containers.get(&index).is_some_and(|&container| {
                    view.input.nodes[container].kind == "class" && view.decorated(container)
                })
        })
        .collect();
    if eligible.is_empty() {
        return Ok(HashSet::new());
    }
    let names: HashSet<_> = eligible
        .iter()
        .map(|&index| view.input.nodes[index].name.as_str())
        .collect();
    let mut declaring: HashMap<&str, HashSet<usize>> = HashMap::new();
    for &index in unreferenced {
        if shaped(index)
            && names.contains(view.input.nodes[index].name.as_str())
            && let Some(container) = view.container(index)
            && view.input.nodes[container].kind == "class"
        {
            declaring
                .entry(&view.input.nodes[index].name)
                .or_default()
                .insert(container);
        }
    }
    let mut interfaces: HashMap<usize, HashSet<&str>> = HashMap::new();
    for &class in declaring.values().flatten() {
        interfaces.entry(class).or_default().extend(
            view.references(class)
                .filter(|reference| reference.reference_kind == "implements")
                .map(|reference| reference.reference_name.as_str()),
        );
    }
    let contracts: HashSet<_> = interfaces.values().flatten().copied().collect();
    let mut writers: HashMap<&str, HashSet<usize>> = HashMap::new();
    for reference in &view.input.unresolved {
        if reference.reference_kind == "implements"
            && contracts.contains(reference.reference_name.as_str())
            && let Some(&writer) = view.ids.get(reference.from_node_id.as_str())
            && ["typescript", "tsx"].contains(&view.input.nodes[writer].language.as_str())
        {
            writers
                .entry(&reference.reference_name)
                .or_default()
                .insert(writer);
        }
    }
    writers.retain(|_, writers| {
        writers
            .iter()
            .filter(|&&writer| view.decorated(writer))
            .count()
            * 2
            > writers.len()
    });
    let implementers = writers.values().flatten().copied().collect();
    let ancestors = view.ancestors(&implementers, &["extends"])?;
    let external = |index: usize| {
        view.references(index)
            .any(|reference| reference.reference_kind == "extends")
    };
    let mut opaque: HashSet<_> = implementers
        .iter()
        .copied()
        .filter(|&index| external(index))
        .collect();
    let mut inherited: HashMap<usize, HashSet<usize>> = HashMap::new();
    for (ancestor, roots) in ancestors {
        for root in roots {
            view.step()?;
            inherited.entry(root).or_default().insert(ancestor);
            if external(ancestor) {
                opaque.insert(root);
            }
        }
    }
    let mut hook_names = HashSet::new();
    for (name, declared) in declaring {
        for contract in declared
            .iter()
            .flat_map(|class| interfaces.get(class).into_iter().flatten())
        {
            let Some(writers) = writers.get(contract) else {
                continue;
            };
            let declared_count = writers.intersection(&declared).count();
            if declared_count >= 2
                && declared_count * 2 > writers.len()
                && writers.iter().all(|writer| {
                    declared.contains(writer)
                        || opaque.contains(writer)
                        || inherited
                            .get(writer)
                            .is_some_and(|ancestors| !ancestors.is_disjoint(&declared))
                })
            {
                hook_names.insert(name);
                break;
            }
        }
    }
    Ok(eligible
        .into_iter()
        .filter(|&index| hook_names.contains(view.input.nodes[index].name.as_str()))
        .collect())
}

fn header(path: &str) -> bool {
    let path = path.to_lowercase();
    [
        ".h", ".hh", ".hpp", ".hxx", ".h++", ".inc", ".d.ts", ".d.mts", ".d.cts", ".pyi", ".pxd",
    ]
    .iter()
    .any(|suffix| path.ends_with(suffix))
}
fn test_scope(name: &str) -> bool {
    name.split(['.', ':', '/', '\\', '#', '>']).any(|part| {
        ["test", "tests", "__tests__", "spec", "specs", "testing"]
            .contains(&part.to_lowercase().as_str())
    })
}
fn vendored(path: &str) -> bool {
    path.split(['/', '\\']).any(|part| {
        [
            "vendor",
            "vendored",
            "third_party",
            "third-party",
            "thirdparty",
            "external",
            "externals",
            "node_modules",
            "bower_components",
            "site-packages",
            "godeps",
            "pods",
            ".venv",
            "venv",
        ]
        .contains(&part.to_lowercase().as_str())
    })
}
fn implicit(name: &str) -> bool {
    let dunder = name
        .strip_prefix("__")
        .and_then(|name| name.strip_suffix("__"))
        .is_some_and(|name| {
            !name.is_empty()
                && name
                    .bytes()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == b'_')
        });
    let name = name.to_lowercase();
    [
        "constructor",
        "main",
        "init",
        "deinit",
        "finalize",
        "destructor",
        "dispose",
        "drop",
        "default",
        "tostring",
        "equals",
        "gethashcode",
        "hashcode",
    ]
    .contains(&name.as_str())
        || dunder
}

fn mention_count(source: &str, name: &str, stop: usize) -> usize {
    if name.is_empty() {
        return 0;
    }
    fn identifier(ch: Option<char>) -> bool {
        static IDENTIFIER: std::sync::LazyLock<regex::Regex> =
            std::sync::LazyLock::new(|| regex::Regex::new(r"^[\p{L}\p{N}_$]$").unwrap());
        ch.is_some_and(|ch| ch.len_utf16() == 1 && IDENTIFIER.is_match(ch.encode_utf8(&mut [0; 4])))
    }
    let mut count = 0;
    for (offset, _) in source.match_indices(name) {
        if !identifier(source[..offset].chars().next_back())
            && !identifier(source[offset + name.len()..].chars().next())
        {
            count += 1;
            if count >= stop {
                break;
            }
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn skipped_dependent_source_cannot_confirm_a_dead_symbol() {
        let node: Node = serde_json::from_value(serde_json::json!({
            "id":"plugin", "kind":"function", "name":"plugin", "qualifiedName":"plugin",
            "filePath":"service.ts", "language":"typescript", "startLine":1,
            "endLine":1, "startColumn":0, "endColumn":0, "updatedAt":0
        }))
        .unwrap();
        let mut file = node.clone();
        file.id = "service-file".into();
        file.kind = "file".into();
        file.name = "service.ts".into();
        file.qualified_name = file.name.clone();
        let mut input = Input {
            nodes: vec![node, file.clone()],
            sources: BTreeMap::from([("service.ts".into(), "function plugin() {}".into())]),
            ..Default::default()
        };
        for index in 0..MAX_FILES {
            let mut dependent = file.clone();
            dependent.id = format!("consumer{index:03}");
            dependent.file_path = format!("consumer{index:03}.ts");
            dependent.name = dependent.file_path.clone();
            dependent.qualified_name = dependent.name.clone();
            input.sources.insert(
                dependent.file_path.clone(),
                if index + 1 == MAX_FILES {
                    "const registration = 'plugin';".into()
                } else {
                    "import './service';".into()
                },
            );
            input.edges.push(Edge {
                source: dependent.id.clone(),
                target: file.id.clone(),
                kind: "imports".into(),
                line: Some(1),
                column: None,
                provenance: None,
                metadata: None,
            });
            input.nodes.push(dependent);
        }
        let report = report(
            &input,
            &DeadCodeOptions {
                include_exported: true,
                ..Default::default()
            },
            true,
        )
        .unwrap();
        assert_eq!(report.candidates, 1);
        assert!(report.entries.is_empty());
        assert_eq!(report.excluded.unreadable, 1);
    }

    #[test]
    fn conservative_reports_match_reference_sqlite_queries() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/upstream/analysis/dead-code.json"
        ))
        .unwrap();
        for case in fixture["cases"].as_array().unwrap() {
            let input: Input = serde_json::from_value(case["input"].clone()).unwrap();
            let options: DeadCodeOptions = serde_json::from_value(case["options"].clone()).unwrap();
            let actual = report(&input, &options, case["corroborate"].as_bool().unwrap()).unwrap();
            assert_eq!(
                serde_json::to_value(actual).unwrap(),
                case["expected"],
                "{}",
                case["id"]
            );
        }
    }

    #[test]
    fn candidate_cap_preserves_the_number_examined_and_marks_the_report() {
        let mut input = Input::default();
        let node:Node=serde_json::from_value(serde_json::json!({"id":"base","kind":"function","name":"base","qualifiedName":"base","filePath":"entry.ts","language":"typescript","startLine":1,"endLine":1,"startColumn":0,"endColumn":0,"updatedAt":0})).unwrap();
        for index in 0..=MAX_CANDIDATES {
            let mut node = node.clone();
            node.id = format!("candidate{index}");
            node.name = node.id.clone();
            input.nodes.push(node);
        }
        let report = report(&input, &DeadCodeOptions::default(), false).unwrap();
        assert!(report.bounded);
        assert_eq!(report.candidates, MAX_CANDIDATES);
        assert_eq!(report.excluded.exports_unknown, MAX_CANDIDATES);
        assert!(report.entries.is_empty());
    }
}
