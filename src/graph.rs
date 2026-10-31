//! Bounded static graph queries. Resolution evidence is never compiler proof.
use crate::index::{clip, generation, validate_limits, words};
use crate::parser::{Import, ParsedFile, Reference, Symbol};
use crate::{Direction, Engine, Relation, RelationsResult, SymbolLocation};
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::path::{Component, Path};

const MAX_METADATA_BYTES: usize = 64 * 1024 * 1024;
const MAX_NODES: usize = 100_000;
const MAX_EDGES: usize = 200_000;
const MAX_CANDIDATES: usize = 50;
const MAX_VISITED: usize = 1_000;

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GraphOptions {
    pub path: Option<String>,
    pub line: Option<usize>,
    pub target: Option<String>,
    pub depth: usize,
    pub limit: usize,
    pub max_bytes: usize,
}

impl Default for GraphOptions {
    fn default() -> Self {
        Self {
            path: None,
            line: None,
            target: None,
            depth: 2,
            limit: 10,
            max_bytes: 12_000,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphMode {
    Impact,
    Path,
    Implementations,
    Affected,
}

#[derive(Debug, Serialize)]
pub struct ContextSymbol {
    pub id: i64,
    pub name: String,
    pub qualified_name: String,
    pub kind: String,
    pub start_line: usize,
    pub end_line: usize,
    pub snippet_start_line: usize,
    pub snippet: String,
    pub snippet_truncated: bool,
    pub syntax_errors: bool,
}

#[derive(Debug, Serialize)]
pub struct ContextFile {
    pub path: String,
    pub symbols: Vec<ContextSymbol>,
}

#[derive(Debug, Serialize)]
pub struct GraphEdge {
    pub from: i64,
    pub to: Option<i64>,
    pub target: String,
    pub line: usize,
    pub kind: String,
    pub resolution: &'static str,
    pub confidence: &'static str,
    pub ambiguous: bool,
}

#[derive(Debug, Serialize)]
pub struct GraphResult {
    pub generation: u64,
    pub roots: Vec<SymbolLocation>,
    pub files: Vec<ContextFile>,
    pub edges: Vec<GraphEdge>,
    /// Symbol IDs on the shortest directed call path, or an empty list.
    pub path: Vec<i64>,
    pub impact: Vec<SymbolLocation>,
    pub affected_tests: Vec<String>,
    pub ambiguous: bool,
    pub truncated: bool,
}

struct File {
    analysis: ParsedFile,
    nodes: Vec<usize>,
    qualified: HashMap<String, Vec<usize>>,
    exports: HashMap<String, Vec<usize>>,
}
struct Node {
    id: i64,
    path: String,
    symbol: Symbol,
}
struct Edge {
    from: usize,
    to: Option<usize>,
    target: String,
    line: usize,
    kind: String,
    method: &'static str,
    confidence: &'static str,
    ambiguous: bool,
}

pub(crate) struct Graph {
    generation: u64,
    files: BTreeMap<String, File>,
    nodes: Vec<Node>,
    ids: HashMap<i64, usize>,
    names: HashMap<String, Vec<usize>>,
    members: HashMap<(usize, String), Vec<usize>>,
    edges: Vec<Edge>,
    outgoing: Vec<Vec<usize>>,
    incoming: Vec<Vec<usize>>,
    dependents: BTreeMap<String, Vec<String>>,
    truncated: bool,
    resolution_truncated: Cell<bool>,
}

impl GraphOptions {
    pub fn validate(&self, query: &str) -> Result<()> {
        validate_limits(self.limit, self.max_bytes)?;
        ensure!(
            !query.trim().is_empty() && query.len() <= 1024,
            "query must contain 1 to 1024 bytes"
        );
        ensure!(
            (1..=8).contains(&self.depth),
            "depth must be between 1 and 8"
        );
        ensure!(
            self.line
                .is_none_or(|line| line > 0 && line <= i64::MAX as usize)
                && (self.line.is_none() || self.path.is_some()),
            "line must be positive and requires path"
        );
        if let Some(path) = &self.path {
            ensure!(
                !path.is_empty()
                    && path.len() <= 1024
                    && Path::new(path)
                        .components()
                        .all(|part| matches!(part, Component::Normal(_) | Component::CurDir)),
                "path must be relative and cannot contain parent components"
            );
        }
        if let Some(target) = &self.target {
            ensure!(
                !target.trim().is_empty() && target.len() <= 1024,
                "target must contain 1 to 1024 bytes"
            );
        }
        Ok(())
    }
}

fn with_graph<T>(
    engine: &Engine,
    action: impl FnOnce(&Graph, &Connection) -> Result<T>,
) -> Result<T> {
    let snapshot = engine.connection.unchecked_transaction()?;
    let current = generation(&snapshot)?;
    let mut cache = engine.graph_cache.borrow_mut();
    if cache
        .as_ref()
        .is_none_or(|graph| graph.generation != current)
    {
        *cache = Some(Graph::load(&snapshot, current)?);
    }
    action(cache.as_ref().unwrap(), &snapshot)
}

impl Graph {
    fn load(connection: &Connection, generation: u64) -> Result<Self> {
        let mut graph = Self {
            generation,
            files: BTreeMap::new(),
            nodes: Vec::new(),
            ids: HashMap::new(),
            names: HashMap::new(),
            members: HashMap::new(),
            edges: Vec::new(),
            outgoing: Vec::new(),
            incoming: Vec::new(),
            dependents: BTreeMap::new(),
            truncated: false,
            resolution_truncated: Cell::new(false),
        };
        // ponytail: metadata is cached per generation with fixed memory/work ceilings;
        // use indexed persisted edges if rebuild cost becomes a measured bottleneck.
        let mut bytes = 0;
        let mut statement = connection.prepare("SELECT path, analysis FROM files ORDER BY path")?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            let path: String = row.get(0)?;
            let metadata: String = row.get(1)?;
            bytes += metadata.len();
            if bytes > MAX_METADATA_BYTES {
                graph.truncated = true;
                break;
            }
            let analysis: ParsedFile =
                serde_json::from_str(&metadata).context("invalid cached syntax metadata")?;
            if graph.nodes.len() + analysis.symbols.len() > MAX_NODES {
                graph.truncated = true;
                break;
            }
            let mut symbols = connection
                .prepare("SELECT id, ordinal FROM symbols WHERE path=?1 ORDER BY ordinal")?;
            let pairs = symbols.query_map([&path], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)? as usize))
            })?;
            let mut nodes = Vec::new();
            for pair in pairs {
                let (id, ordinal) = pair?;
                ensure!(ordinal == nodes.len(), "invalid cached symbol ordinal");
                let symbol = analysis
                    .symbols
                    .get(ordinal)
                    .context("missing cached symbol metadata")?
                    .clone();
                let index = graph.nodes.len();
                graph.ids.insert(id, index);
                graph
                    .names
                    .entry(symbol.name.clone())
                    .or_default()
                    .push(index);
                if symbol.qualified_name != symbol.name {
                    graph
                        .names
                        .entry(symbol.qualified_name.clone())
                        .or_default()
                        .push(index);
                }
                graph.nodes.push(Node {
                    id,
                    path: path.clone(),
                    symbol,
                });
                nodes.push(index);
            }
            ensure!(
                nodes.len() == analysis.symbols.len(),
                "incomplete cached symbols"
            );
            let mut qualified: HashMap<String, Vec<usize>> = HashMap::new();
            let mut exports: HashMap<String, Vec<usize>> = HashMap::new();
            for &index in &nodes {
                let symbol = &graph.nodes[index].symbol;
                qualified
                    .entry(symbol.qualified_name.clone())
                    .or_default()
                    .push(index);
                for name in &symbol.exports {
                    exports.entry(name.clone()).or_default().push(index);
                }
                if analysis.language == "ruby" {
                    exports.entry(symbol.name.clone()).or_default().push(index);
                    exports
                        .entry(symbol.qualified_name.clone())
                        .or_default()
                        .push(index);
                }
            }
            graph.files.insert(
                path,
                File {
                    analysis,
                    nodes,
                    qualified,
                    exports,
                },
            );
        }
        graph.outgoing.resize_with(graph.nodes.len(), Vec::new);
        graph.incoming.resize_with(graph.nodes.len(), Vec::new);
        graph.build_edges();
        graph.truncated |= graph.resolution_truncated.get();
        Ok(graph)
    }

    fn location(&self, index: usize) -> SymbolLocation {
        let node = &self.nodes[index];
        SymbolLocation {
            id: node.id,
            name: node.symbol.name.clone(),
            qualified_name: node.symbol.qualified_name.clone(),
            path: node.path.clone(),
            start_line: node.symbol.start_line,
        }
    }

    fn module_files(&self, from: &str, module: &str) -> Vec<String> {
        let language = &self.files[from].analysis.language;
        let parent = from.rsplit_once('/').map_or("", |(parent, _)| parent);
        let mut stems = Vec::new();
        if module.starts_with('.') {
            if let Some(path) = normalize_path(&format!("{parent}/{module}")) {
                stems.push(path);
            }
        } else if language == "rust" {
            let mut parts: Vec<_> = module.split("::").filter(|part| !part.is_empty()).collect();
            let crate_root = from.rfind("src/").map_or(parent, |at| &from[..at + 3]);
            let base = if parts.first() == Some(&"crate") {
                parts.remove(0);
                crate_root.to_owned()
            } else if parts.first() == Some(&"self") {
                parts.remove(0);
                rust_module_dir(from)
            } else if parts.first() == Some(&"super") {
                let mut base = rust_module_dir(from);
                while parts.first() == Some(&"super") {
                    parts.remove(0);
                    base = base.rsplit_once('/').map_or("", |(p, _)| p).to_owned();
                }
                base
            } else {
                rust_module_dir(from)
            };
            if let Some(path) = normalize_path(&format!("{base}/{}", parts.join("/"))) {
                if parts.is_empty() {
                    stems.extend([
                        format!("{path}/lib.rs"),
                        format!("{path}/main.rs"),
                        format!("{path}/mod.rs"),
                    ]);
                }
                stems.push(path);
            }
            if let Some(path) = normalize_path(&format!("{parent}/{}", parts.join("/"))) {
                stems.push(path);
            }
        } else if language == "ruby"
            && let Some(path) = normalize_path(&format!("{parent}/{module}"))
        {
            stems.push(path);
        }
        let mut found = BTreeSet::new();
        for stem in stems {
            let mut candidates = vec![stem.clone()];
            if let Some(base) = stem
                .strip_suffix(".js")
                .or_else(|| stem.strip_suffix(".mjs"))
                .or_else(|| stem.strip_suffix(".cjs"))
            {
                candidates.extend([
                    format!("{base}.ts"),
                    format!("{base}.tsx"),
                    format!("{base}.mts"),
                    format!("{base}.cts"),
                ]);
            }
            for extension in [
                "ts", "tsx", "js", "jsx", "mjs", "cjs", "mts", "cts", "rs", "rb",
            ] {
                candidates.push(format!("{stem}.{extension}"));
                candidates.push(format!("{stem}/index.{extension}"));
            }
            candidates.push(format!("{stem}/mod.rs"));
            for candidate in candidates {
                if self.files.contains_key(&candidate) {
                    found.insert(candidate);
                }
            }
        }
        found.into_iter().collect()
    }

    fn exported(
        &self,
        path: &str,
        name: &str,
        visited: &mut HashSet<(String, String)>,
    ) -> Vec<usize> {
        if visited.contains(&(path.to_owned(), name.to_owned())) {
            return Vec::new();
        }
        if visited.len() >= 32 {
            self.resolution_truncated.set(true);
            return Vec::new();
        }
        visited.insert((path.to_owned(), name.to_owned()));
        let file = &self.files[path];
        let mut found = file.exports.get(name).cloned().unwrap_or_default();
        for import in &file.analysis.imports {
            let star = import.local == "*" && import.imported == "*" && name != "default";
            if import.reexport && (star || (import.local == name && import.imported != "*")) {
                let imported = if import.imported == "*" {
                    name
                } else {
                    &import.imported
                };
                if import.module.is_empty() {
                    found.extend(file.qualified.get(imported).into_iter().flatten().copied());
                } else {
                    for target in self.module_files(path, &import.module) {
                        found.extend(self.exported(&target, imported, visited));
                    }
                }
            }
        }
        found.sort_unstable();
        found.dedup();
        found.truncate(MAX_CANDIDATES);
        found
    }

    fn namespace_member(
        &self,
        path: &str,
        namespace: &str,
        member: &str,
        visited: &mut HashSet<(String, String)>,
    ) -> Vec<usize> {
        if visited.contains(&(path.to_owned(), namespace.to_owned())) {
            return Vec::new();
        }
        if visited.len() >= 32 {
            self.resolution_truncated.set(true);
            return Vec::new();
        }
        visited.insert((path.to_owned(), namespace.to_owned()));
        let mut found = Vec::new();
        for import in &self.files[path].analysis.imports {
            if !import.reexport || import.local != namespace {
                continue;
            }
            for target in self.module_files(path, &import.module) {
                if import.imported == "*" {
                    found.extend(self.exported(&target, member, &mut HashSet::new()));
                } else {
                    found.extend(self.namespace_member(&target, &import.imported, member, visited));
                }
            }
        }
        found.sort_unstable();
        found.dedup();
        if found.len() >= MAX_CANDIDATES {
            self.resolution_truncated.set(true);
            found.truncate(MAX_CANDIDATES);
        }
        found
    }

    fn imported(&self, path: &str, import: &Import, name: &str) -> Vec<usize> {
        let requested = if import.imported == "*" {
            name
        } else {
            &import.imported
        };
        let mut found = Vec::new();
        for target in self.module_files(path, &import.module) {
            found.extend(self.exported(&target, requested, &mut HashSet::new()));
        }
        if self.files[path].analysis.language == "rust" {
            let module = import
                .module
                .trim_start_matches("crate::")
                .trim_start_matches("self::");
            let qualified = if module.is_empty() {
                requested.to_owned()
            } else {
                format!("{module}::{requested}")
            };
            found.extend(
                self.files[path]
                    .qualified
                    .get(&qualified)
                    .into_iter()
                    .flatten()
                    .copied(),
            );
        }
        found.sort_unstable();
        found.dedup();
        found.truncate(MAX_CANDIDATES);
        found
    }

    fn visible_import<'a>(&'a self, path: &str, scope: &str, local: &str) -> Option<&'a Import> {
        self.files[path]
            .analysis
            .imports
            .iter()
            .filter(|import| {
                (!import.reexport || self.files[path].analysis.language == "rust")
                    && import.local == local
                    && (import.scope.is_empty()
                        || scope == import.scope
                        || scope.starts_with(&format!("{}::", import.scope)))
            })
            .max_by_key(|import| import.scope.len())
    }

    fn lookup(&self, path: &str, scope: &str, name: &str) -> (Vec<usize>, &'static str) {
        let file = &self.files[path];
        let mut current = scope;
        loop {
            let qualified = if current.is_empty() {
                name.to_owned()
            } else {
                format!("{current}::{name}")
            };
            let found = file.qualified.get(&qualified).cloned().unwrap_or_default();
            if !found.is_empty() {
                return (
                    found.into_iter().take(MAX_CANDIDATES).collect(),
                    "scope_match",
                );
            }
            if let Some(import) = self
                .visible_import(path, current, name)
                .filter(|import| import.scope == current)
            {
                return (self.imported(path, import, name), "import_match");
            }
            if let Some((namespace, member)) = name.split_once("::")
                && let Some(import) = self
                    .visible_import(path, current, namespace)
                    .filter(|import| import.scope == current)
            {
                if import.imported == "*" {
                    return (self.imported(path, import, member), "import_match");
                }
                let mut found = Vec::new();
                for target in self.module_files(path, &import.module) {
                    found.extend(self.namespace_member(
                        &target,
                        &import.imported,
                        member,
                        &mut HashSet::new(),
                    ));
                }
                found.sort_unstable();
                found.dedup();
                if found.len() > MAX_CANDIDATES {
                    self.resolution_truncated.set(true);
                }
                found.truncate(MAX_CANDIDATES);
                return (found, "import_match");
            }
            if current.is_empty() {
                break;
            }
            current = current.rsplit_once("::").map_or("", |(parent, _)| parent);
        }
        let mut required = Vec::new();
        for import in &file.analysis.imports {
            if import.local == "*" && import.imported == "*" {
                required.extend(self.imported(path, import, name));
            }
        }
        if !required.is_empty() {
            required.sort_unstable();
            required.dedup();
            required.truncate(MAX_CANDIDATES);
            return (required, "import_match");
        }
        let found = self
            .names
            .get(name)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|&index| self.nodes[index].symbol.kind != "file")
            .take(MAX_CANDIDATES)
            .collect();
        (found, "name_match")
    }

    fn members(&self, owner: usize, name: &str, visited: &mut HashSet<usize>) -> Vec<usize> {
        if visited.contains(&owner) {
            return Vec::new();
        }
        if visited.len() >= 32 {
            self.resolution_truncated.set(true);
            return Vec::new();
        }
        visited.insert(owner);
        let direct = self
            .members
            .get(&(owner, name.to_owned()))
            .cloned()
            .unwrap_or_default();
        if !direct.is_empty() {
            return direct;
        }
        let mut inherited = Vec::new();
        for &edge in &self.outgoing[owner] {
            let edge = &self.edges[edge];
            if matches!(edge.kind.as_str(), "extends" | "implements")
                && let Some(base) = edge.to
            {
                inherited.extend(self.members(base, name, visited));
            }
        }
        inherited.sort_unstable();
        inherited.dedup();
        inherited.truncate(MAX_CANDIDATES);
        inherited
    }

    fn resolve(
        &self,
        path: &str,
        caller: usize,
        reference: &Reference,
    ) -> (Vec<usize>, &'static str) {
        if reference.shadowed {
            return (Vec::new(), "unresolved");
        }
        let node = &self.nodes[caller];
        let scope = reference
            .owner
            .as_deref()
            .unwrap_or(&node.symbol.qualified_name);
        if let Some(receiver) = &reference.receiver_type {
            let (owners, owner_method) = self.lookup(path, scope, receiver);
            let uncertain_owner = owner_method == "name_match" || owners.len() > 1;
            let mut members = Vec::new();
            for owner in owners {
                members.extend(self.members(owner, &reference.target, &mut HashSet::new()));
            }
            members.sort_unstable();
            members.dedup();
            members.truncate(MAX_CANDIDATES);
            return (
                members,
                if uncertain_owner {
                    "name_match"
                } else {
                    "receiver_match"
                },
            );
        }
        if let Some(qualifier) = &reference.qualifier {
            if matches!(qualifier.as_str(), "self" | "this" | "Self") {
                let mut container = node.symbol.container.as_deref();
                while let Some(name) = container {
                    let owners: Vec<_> = self.files[path]
                        .nodes
                        .iter()
                        .copied()
                        .filter(|&index| {
                            self.nodes[index].symbol.qualified_name == name
                                && matches!(
                                    self.nodes[index].symbol.kind.as_str(),
                                    "class" | "struct" | "trait" | "module"
                                )
                        })
                        .collect();
                    if !owners.is_empty() {
                        let mut found = Vec::new();
                        for owner in owners {
                            found.extend(self.members(
                                owner,
                                &reference.target,
                                &mut HashSet::new(),
                            ));
                        }
                        return (found, "receiver_match");
                    }
                    container = name.rsplit_once("::").map(|(parent, _)| parent);
                }
                return (Vec::new(), "receiver_match");
            }
            if let Some(import) = self.visible_import(path, scope, qualifier) {
                if import.imported == "*" {
                    return (
                        self.imported(path, import, &reference.target),
                        "import_match",
                    );
                }
                let mut found = Vec::new();
                for owner in self.imported(path, import, qualifier) {
                    found.extend(self.members(owner, &reference.target, &mut HashSet::new()));
                }
                if found.is_empty() && self.files[path].analysis.language == "rust" {
                    for target in
                        self.module_files(path, &format!("{}::{}", import.module, import.imported))
                    {
                        found.extend(self.exported(
                            &target,
                            &reference.target,
                            &mut HashSet::new(),
                        ));
                    }
                }
                if found.is_empty() {
                    for target in self.module_files(path, &import.module) {
                        found.extend(self.namespace_member(
                            &target,
                            &import.imported,
                            &reference.target,
                            &mut HashSet::new(),
                        ));
                    }
                }
                return (found, "import_match");
            }
            let (owners, method) = self.lookup(path, scope, qualifier);
            let owners: Vec<_> = owners
                .into_iter()
                .filter(|&index| {
                    matches!(
                        self.nodes[index].symbol.kind.as_str(),
                        "class" | "struct" | "module" | "trait" | "enum"
                    )
                })
                .collect();
            if !owners.is_empty() {
                let mut found = Vec::new();
                for owner in owners {
                    found.extend(self.members(owner, &reference.target, &mut HashSet::new()));
                }
                return (
                    found,
                    if method == "name_match" {
                        "name_match"
                    } else {
                        "receiver_match"
                    },
                );
            }
            if self.files[path].analysis.language == "rust" {
                let mut found = Vec::new();
                for target in self.module_files(path, qualifier) {
                    found.extend(self.exported(&target, &reference.target, &mut HashSet::new()));
                }
                if !found.is_empty() {
                    return (found, "module_match");
                }
                let qualified = format!(
                    "{}::{}",
                    qualifier
                        .trim_start_matches("crate::")
                        .trim_start_matches("self::"),
                    reference.target
                );
                let local: Vec<_> = self.files[path]
                    .nodes
                    .iter()
                    .copied()
                    .filter(|&index| self.nodes[index].symbol.qualified_name == qualified)
                    .collect();
                if !local.is_empty() {
                    return (local, "module_match");
                }
                return (Vec::new(), "module_match");
            }
            // Unknown dynamic receivers retain only low-confidence name candidates.
            return (
                self.names
                    .get(&reference.target)
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .take(MAX_CANDIDATES)
                    .collect(),
                "name_match",
            );
        }
        self.lookup(path, scope, &reference.target)
    }

    fn push_edge(&mut self, edge: Edge) {
        if self.edges.len() == MAX_EDGES {
            self.truncated = true;
            return;
        }
        let index = self.edges.len();
        self.outgoing[edge.from].push(index);
        if let Some(to) = edge.to {
            self.incoming[to].push(index);
        }
        self.edges.push(edge);
    }

    fn build_edges(&mut self) {
        let paths: Vec<_> = self.files.keys().cloned().collect();
        for path in &paths {
            let imports: Vec<_> = self.files[path]
                .analysis
                .imports
                .iter()
                .map(|import| import.module.clone())
                .collect();
            for module in imports {
                for target in self.module_files(path, &module) {
                    self.dependents
                        .entry(target)
                        .or_default()
                        .push(path.clone());
                }
            }
        }
        for dependents in self.dependents.values_mut() {
            dependents.sort();
            dependents.dedup();
        }
        for index in 0..self.nodes.len() {
            let node = &self.nodes[index];
            if let Some(container) = &node.symbol.container {
                let owners: Vec<_> = self.files[&node.path]
                    .qualified
                    .get(container)
                    .into_iter()
                    .flatten()
                    .copied()
                    .filter(|&owner| owner != index)
                    .take(MAX_CANDIDATES)
                    .collect();
                for owner in owners {
                    if self.edges.len() == MAX_EDGES {
                        self.truncated = true;
                        return;
                    }
                    self.push_edge(Edge {
                        from: owner,
                        to: Some(index),
                        target: self.nodes[index].symbol.name.clone(),
                        line: self.nodes[index].symbol.start_line,
                        kind: "contains".into(),
                        method: "syntax",
                        confidence: "high",
                        ambiguous: false,
                    });
                }
            }
        }
        // Rust impl blocks may live in a different module from their type. Bind
        // methods through that module's imports instead of merging equal type names.
        for index in 0..self.nodes.len() {
            if !matches!(
                self.nodes[index].symbol.kind.as_str(),
                "function" | "method"
            ) {
                continue;
            }
            let Some(container) = self.nodes[index].symbol.container.clone() else {
                continue;
            };
            let path = self.nodes[index].path.clone();
            let (scope, owner_name) = container.rsplit_once("::").unwrap_or(("", &container));
            let (owners, method) = self.lookup(&path, scope, owner_name);
            for owner in owners {
                if !matches!(
                    self.nodes[owner].symbol.kind.as_str(),
                    "class" | "interface" | "struct" | "trait" | "enum"
                ) {
                    continue;
                }
                let local = self.nodes[owner].path == path;
                if !local
                    && !(self.files[&path].analysis.language == "rust" && method != "name_match")
                {
                    continue;
                }
                self.members
                    .entry((owner, self.nodes[index].symbol.name.clone()))
                    .or_default()
                    .push(index);
                if !local {
                    if self.edges.len() == MAX_EDGES {
                        self.truncated = true;
                        return;
                    }
                    self.push_edge(Edge {
                        from: owner,
                        to: Some(index),
                        target: self.nodes[index].symbol.name.clone(),
                        line: self.nodes[index].symbol.start_line,
                        kind: "contains".into(),
                        method: "import_match",
                        confidence: "medium",
                        ambiguous: false,
                    });
                }
            }
        }
        // Inheritance must exist before resolving calls to inherited members.
        for inheritance in [true, false] {
            for path in &paths {
                let references: Vec<_> = self.files[path]
                    .analysis
                    .references
                    .iter()
                    .filter(|reference| (reference.kind != "calls") == inheritance)
                    .cloned()
                    .collect();
                for reference in references {
                    if self.edges.len() == MAX_EDGES {
                        self.truncated = true;
                        return;
                    }
                    let (callers, owner_method) = if let Some(from) = reference
                        .caller
                        .and_then(|ordinal| self.files[path].nodes.get(ordinal).copied())
                    {
                        (vec![from], "syntax")
                    } else if let Some(owner) = &reference.owner {
                        let (scope, name) = owner.rsplit_once("::").unwrap_or(("", owner));
                        self.lookup(path, scope, name)
                    } else {
                        continue;
                    };
                    let owner_ambiguous = callers.len() > 1;
                    for from in callers {
                        if self.edges.len() == MAX_EDGES {
                            self.truncated = true;
                            return;
                        }
                        let (mut candidates, mut method) = self.resolve(path, from, &reference);
                        if owner_method == "name_match" {
                            method = "name_match";
                        }
                        candidates.sort_unstable();
                        candidates.dedup();
                        let ambiguous = owner_ambiguous || candidates.len() > 1;
                        if candidates.len() >= MAX_CANDIDATES {
                            self.truncated = true;
                            candidates.truncate(MAX_CANDIDATES);
                        }
                        if candidates.is_empty() {
                            self.push_edge(Edge {
                                from,
                                to: None,
                                target: reference.target.clone(),
                                line: reference.line,
                                kind: reference.kind.clone(),
                                method: "unresolved",
                                confidence: "unknown",
                                ambiguous: owner_ambiguous,
                            });
                        } else {
                            for to in candidates {
                                self.push_edge(Edge {
                                    from,
                                    to: Some(to),
                                    target: reference.target.clone(),
                                    line: reference.line,
                                    kind: reference.kind.clone(),
                                    method,
                                    confidence: if method == "name_match" || ambiguous {
                                        "low"
                                    } else {
                                        "medium"
                                    },
                                    ambiguous,
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    fn select(
        &self,
        connection: &Connection,
        query: &str,
        options: &GraphOptions,
        lexical: bool,
    ) -> Result<(Vec<usize>, bool)> {
        let qualified = query.replace('.', "::");
        let path = options
            .path
            .as_deref()
            .map(|path| path.trim_start_matches("./"));
        let matches = |index: usize| {
            let node = &self.nodes[index];
            path.is_none_or(|path| node.path == path)
                && options.line.is_none_or(|line| {
                    node.symbol.start_line <= line && node.symbol.end_line >= line
                })
        };
        let mut selected: Vec<_> = if let Some(file) = self.files.get(query) {
            file.nodes.clone()
        } else {
            let mut indices = self.names.get(query).cloned().unwrap_or_default();
            indices.extend(self.names.get(&qualified).into_iter().flatten().copied());
            indices.sort_unstable();
            indices.dedup();
            indices
        };
        selected.retain(|&index| matches(index));
        if selected.is_empty() && options.line.is_some() {
            selected = path
                .and_then(|path| self.files.get(path))
                .into_iter()
                .flat_map(|file| file.nodes.iter().copied())
                .filter(|&index| matches(index))
                .collect();
            selected.sort_by_key(|&index| {
                self.nodes[index].symbol.end_byte - self.nodes[index].symbol.start_byte
            });
            selected.truncate(1);
        }
        let exact = !selected.is_empty();
        if selected.is_empty() && lexical {
            let terms: Vec<_> = words(query)
                .into_iter()
                .filter(|word| {
                    ![
                        "how", "does", "what", "where", "the", "a", "an", "is", "in", "of", "to",
                        "and",
                    ]
                    .contains(&word.as_str())
                })
                .take(16)
                .collect();
            if !terms.is_empty() {
                let expression = terms
                    .iter()
                    .map(|word| format!("\"{}\"*", word.replace('"', "\"\"")))
                    .collect::<Vec<_>>()
                    .join(" OR ");
                let mut statement = connection.prepare("SELECT s.id FROM symbols_fts JOIN symbols s ON s.id=symbols_fts.rowid WHERE symbols_fts MATCH ?1 AND (?2 IS NULL OR s.path=?2) AND (?3 IS NULL OR (s.start_line<=?3 AND s.end_line>=?3)) ORDER BY bm25(symbols_fts,10.0,8.0,2.0,0.5,1.0),s.path,s.start_line LIMIT 51")?;
                let ids = statement.query_map(
                    params![expression, path, options.line.map(|line| line as i64)],
                    |row| row.get::<_, i64>(0),
                )?;
                for id in ids {
                    if let Some(&index) = self.ids.get(&id?) {
                        selected.push(index);
                    }
                }
            }
        }
        let ambiguous = selected.len() > 1 && exact && !self.files.contains_key(query);
        Ok((selected, ambiguous))
    }

    fn edge(&self, index: usize) -> GraphEdge {
        let edge = &self.edges[index];
        GraphEdge {
            from: self.nodes[edge.from].id,
            to: edge.to.map(|to| self.nodes[to].id),
            target: edge.target.clone(),
            line: edge.line,
            kind: edge.kind.clone(),
            resolution: edge.method,
            confidence: edge.confidence,
            ambiguous: edge.ambiguous,
        }
    }

    fn walk(
        &self,
        roots: &[usize],
        depth: usize,
        reverse: bool,
        kinds: &[&str],
    ) -> (Vec<usize>, Vec<usize>, bool) {
        let mut seen: HashSet<_> = roots.iter().copied().collect();
        let mut queue: VecDeque<_> = roots.iter().map(|&node| (node, 0)).collect();
        let mut nodes = Vec::new();
        let mut edges = Vec::new();
        let mut truncated = false;
        while let Some((node, distance)) = queue.pop_front() {
            let adjacent = if reverse {
                &self.incoming[node]
            } else {
                &self.outgoing[node]
            };
            for &index in adjacent {
                let edge = &self.edges[index];
                if !kinds.contains(&edge.kind.as_str()) {
                    continue;
                }
                let target = if reverse { Some(edge.from) } else { edge.to };
                if distance == depth {
                    if target.is_some_and(|target| !seen.contains(&target)) {
                        truncated = true;
                    }
                    continue;
                }
                if edges.len() == MAX_VISITED * 4 {
                    truncated = true;
                    return (nodes, edges, truncated);
                }
                edges.push(index);
                if let Some(target) = target
                    && seen.insert(target)
                {
                    if seen.len() > MAX_VISITED {
                        truncated = true;
                        return (nodes, edges, truncated);
                    }
                    nodes.push(target);
                    queue.push_back((target, distance + 1));
                }
            }
        }
        edges.sort_unstable();
        edges.dedup();
        (nodes, edges, truncated)
    }

    fn shortest(
        &self,
        roots: &[usize],
        targets: &[usize],
        depth: usize,
    ) -> (Vec<usize>, Vec<usize>, bool) {
        let targets: HashSet<_> = targets.iter().copied().collect();
        let mut seen: HashSet<_> = roots.iter().copied().collect();
        let mut previous: HashMap<usize, (usize, usize)> = HashMap::new();
        let mut queue: VecDeque<_> = roots.iter().map(|&node| (node, 0)).collect();
        let mut truncated = false;
        let mut visited_edges = 0;
        while let Some((node, distance)) = queue.pop_front() {
            if targets.contains(&node) {
                let mut nodes = vec![node];
                let mut edges = Vec::new();
                let mut current = node;
                while let Some(&(parent, edge)) = previous.get(&current) {
                    nodes.push(parent);
                    edges.push(edge);
                    current = parent;
                }
                nodes.reverse();
                edges.reverse();
                return (nodes, edges, truncated);
            }
            for &index in &self.outgoing[node] {
                visited_edges += 1;
                if visited_edges > MAX_VISITED * 4 {
                    return (Vec::new(), Vec::new(), true);
                }
                let edge = &self.edges[index];
                if edge.kind != "calls" {
                    continue;
                }
                let Some(target) = edge.to else {
                    continue;
                };
                if seen.contains(&target) {
                    continue;
                }
                if distance == depth {
                    truncated = true;
                    continue;
                }
                if seen.len() == MAX_VISITED {
                    return (Vec::new(), Vec::new(), true);
                }
                seen.insert(target);
                previous.insert(target, (node, index));
                queue.push_back((target, distance + 1));
            }
        }
        (Vec::new(), Vec::new(), truncated)
    }

    fn affected(&self, paths: &[String], depth: usize) -> (Vec<String>, bool) {
        let mut seen: BTreeSet<_> = paths.iter().cloned().collect();
        let mut queue: VecDeque<_> = paths.iter().map(|path| (path.clone(), 0)).collect();
        let mut truncated = false;
        let mut visited_edges = 0;
        'walk: while let Some((path, distance)) = queue.pop_front() {
            for dependent in self.dependents.get(&path).into_iter().flatten() {
                visited_edges += 1;
                if visited_edges > MAX_VISITED * 4 {
                    truncated = true;
                    break 'walk;
                }
                if seen.contains(dependent) {
                    continue;
                }
                if distance == depth {
                    truncated = true;
                    continue;
                }
                if seen.len() == MAX_VISITED {
                    truncated = true;
                    break 'walk;
                }
                seen.insert(dependent.clone());
                queue.push_back((dependent.clone(), distance + 1));
            }
        }
        (
            seen.into_iter().filter(|path| test_path(path)).collect(),
            truncated,
        )
    }
}

impl Engine {
    pub fn explore(&self, query: &str, options: &GraphOptions) -> Result<GraphResult> {
        options.validate(query)?;
        ensure!(
            options.target.is_none(),
            "target is only supported in path mode"
        );
        with_graph(self, |graph, connection| {
            graph.query(connection, query, None, options)
        })
    }

    pub fn analyze(
        &self,
        query: &str,
        mode: GraphMode,
        options: &GraphOptions,
    ) -> Result<GraphResult> {
        options.validate(query)?;
        ensure!(
            (mode == GraphMode::Path) == options.target.is_some(),
            "target is required only in path mode"
        );
        with_graph(self, |graph, connection| {
            graph.query(connection, query, Some(mode), options)
        })
    }
}

impl Graph {
    fn query(
        &self,
        connection: &Connection,
        query: &str,
        mode: Option<GraphMode>,
        options: &GraphOptions,
    ) -> Result<GraphResult> {
        let (mut roots, ambiguous) = self.select(connection, query, options, mode.is_none())?;
        let mut result = GraphResult {
            generation: self.generation,
            roots: Vec::new(),
            files: Vec::new(),
            edges: Vec::new(),
            path: Vec::new(),
            impact: Vec::new(),
            affected_tests: Vec::new(),
            ambiguous,
            truncated: self.truncated || roots.len() > options.limit,
        };
        roots.truncate(options.limit);
        result.roots = roots.iter().map(|&index| self.location(index)).collect();
        let mut included = roots.clone();
        let mut selected_edges = Vec::new();
        match mode {
            Some(GraphMode::Path) => {
                let target_options = GraphOptions {
                    path: None,
                    line: None,
                    target: None,
                    ..options.clone()
                };
                let (targets, ambiguous) = self.select(
                    connection,
                    options.target.as_deref().unwrap(),
                    &target_options,
                    false,
                )?;
                result.ambiguous |= ambiguous;
                let (nodes, edges, truncated) = self.shortest(&roots, &targets, options.depth);
                result.path = nodes.iter().map(|&index| self.nodes[index].id).collect();
                included.extend(nodes);
                selected_edges.extend(edges);
                result.truncated |= truncated;
            }
            Some(GraphMode::Affected) => {
                let paths = if self.files.contains_key(query) {
                    vec![query.to_owned()]
                } else {
                    roots
                        .iter()
                        .map(|&index| self.nodes[index].path.clone())
                        .collect()
                };
                let (tests, truncated) = self.affected(&paths, options.depth);
                result.truncated |= truncated || tests.len() > options.limit;
                result.affected_tests = tests.into_iter().take(options.limit).collect();
            }
            Some(GraphMode::Impact | GraphMode::Implementations) | None => {
                let kinds: &[&str] = if mode == Some(GraphMode::Implementations) {
                    &["extends", "implements"]
                } else {
                    &["calls", "extends", "implements"]
                };
                let mut impact_roots = roots.clone();
                if mode != Some(GraphMode::Implementations) {
                    for &root in &roots {
                        for &edge in &self.outgoing[root] {
                            if self.edges[edge].kind == "contains"
                                && let Some(child) = self.edges[edge].to
                            {
                                impact_roots.push(child);
                            }
                        }
                    }
                }
                let (impacted, edges, truncated) =
                    self.walk(&impact_roots, options.depth, true, kinds);
                result.truncated |= truncated || impacted.len() > options.limit;
                result.impact = impacted
                    .iter()
                    .take(options.limit)
                    .map(|&index| self.location(index))
                    .collect();
                selected_edges.extend(edges);
                if mode.is_none() {
                    let (neighbors, edges, truncated) = self.walk(
                        &roots,
                        options.depth,
                        false,
                        &["calls", "extends", "implements", "contains"],
                    );
                    included.extend(neighbors);
                    selected_edges.extend(edges);
                    result.truncated |= truncated;
                    'paths: for &from in roots.iter().take(4) {
                        for &to in roots.iter().take(4) {
                            if from == to {
                                continue;
                            }
                            let (path, edges, truncated) =
                                self.shortest(&[from], &[to], options.depth);
                            result.truncated |= truncated;
                            if !path.is_empty() {
                                result.path =
                                    path.iter().map(|&index| self.nodes[index].id).collect();
                                included.extend(path);
                                selected_edges.extend(edges);
                                break 'paths;
                            }
                        }
                    }
                }
                included.extend(impacted);
            }
        }
        let mut seen = HashSet::new();
        included.retain(|index| seen.insert(*index));
        result.truncated |= included.len() > options.limit;
        included.truncate(options.limit);
        let included_set: HashSet<_> = included.iter().copied().collect();
        if result
            .path
            .iter()
            .any(|id| !included_set.contains(&self.ids[id]))
        {
            result.path.clear();
            result.truncated = true;
        }
        selected_edges.sort_unstable();
        selected_edges.dedup();
        for index in selected_edges {
            let edge = &self.edges[index];
            if included_set.contains(&edge.from)
                && edge.to.is_none_or(|to| included_set.contains(&to))
            {
                result.edges.push(self.edge(index));
            } else {
                result.truncated = true;
            }
        }
        let terms = words(query);
        for index in included {
            let node = &self.nodes[index];
            let code: String =
                connection.query_row("SELECT code FROM symbols WHERE id=?1", [node.id], |row| {
                    row.get(0)
                })?;
            let (snippet, start, truncated) = snippet(
                &code,
                node.symbol.start_line,
                &terms,
                options.line,
                options.max_bytes.min(8000),
            );
            let symbol = ContextSymbol {
                id: node.id,
                name: node.symbol.name.clone(),
                qualified_name: node.symbol.qualified_name.clone(),
                kind: node.symbol.kind.clone(),
                start_line: node.symbol.start_line,
                end_line: node.symbol.end_line,
                snippet_start_line: start,
                snippet,
                snippet_truncated: truncated,
                syntax_errors: self.files[&node.path].analysis.has_errors,
            };
            if let Some(file) = result.files.iter_mut().find(|file| file.path == node.path) {
                file.symbols.push(symbol);
            } else {
                result.files.push(ContextFile {
                    path: node.path.clone(),
                    symbols: vec![symbol],
                });
            }
        }
        fit(&mut result, options.max_bytes)?;
        Ok(result)
    }
}

pub(crate) fn related(
    engine: &Engine,
    symbol: &str,
    direction: Direction,
    limit: usize,
    max_bytes: usize,
) -> Result<RelationsResult> {
    let options = GraphOptions {
        limit,
        max_bytes,
        ..GraphOptions::default()
    };
    options.validate(symbol)?;
    with_graph(engine, |graph, connection| {
        let (selected, _) = graph.select(connection, symbol, &options, false)?;
        let selected: HashSet<_> = selected.into_iter().collect();
        let mut result = RelationsResult {
            generation: graph.generation,
            relations: Vec::new(),
            truncated: graph.truncated,
        };
        for edge in &graph.edges {
            if edge.kind != "calls" {
                continue;
            }
            let matches = match direction {
                Direction::Callees => selected.contains(&edge.from),
                Direction::Callers => {
                    edge.to.is_some_and(|to| selected.contains(&to))
                        || (edge.to.is_none() && edge.target == symbol)
                }
            };
            if !matches {
                continue;
            }
            if result.relations.len() == limit {
                result.truncated = true;
                break;
            }
            result.relations.push(Relation {
                caller: graph.location(edge.from),
                target: edge.target.clone(),
                call_line: edge.line,
                candidate: edge.to.map(|to| graph.location(to)),
                resolution: edge.method,
                kind: edge.kind.clone(),
                confidence: edge.confidence,
                ambiguous: edge.ambiguous,
            });
            if serde_json::to_vec(&result)?.len() > max_bytes {
                result.relations.pop();
                result.truncated = true;
                break;
            }
        }
        Ok(result)
    })
}

fn normalize_path(path: &str) -> Option<String> {
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            other => parts.push(other),
        }
    }
    Some(parts.join("/"))
}
fn rust_module_dir(path: &str) -> String {
    let (parent, file) = path.rsplit_once('/').unwrap_or(("", path));
    if matches!(file, "mod.rs" | "lib.rs" | "main.rs") {
        parent.to_owned()
    } else {
        format!("{parent}/{}", file.trim_end_matches(".rs"))
            .trim_start_matches('/')
            .to_owned()
    }
}
fn test_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower
        .split('/')
        .any(|part| matches!(part, "test" | "tests" | "__tests__" | "spec" | "specs"))
        || [".test.", ".spec.", "_test.", "_spec."]
            .iter()
            .any(|pattern| lower.contains(pattern))
        || lower
            .rsplit('/')
            .next()
            .is_some_and(|name| name.starts_with("test_"))
}
fn snippet(
    code: &str,
    start_line: usize,
    terms: &[String],
    line: Option<usize>,
    max: usize,
) -> (String, usize, bool) {
    if code.len() <= max {
        return (code.to_owned(), start_line, false);
    }
    let lines: Vec<_> = code.split_inclusive('\n').collect();
    let focus = line
        .map(|line| line.saturating_sub(start_line))
        .filter(|&line| line < lines.len())
        .or_else(|| {
            lines
                .iter()
                .position(|line| terms.iter().any(|term| line.to_lowercase().contains(term)))
        })
        .unwrap_or(0);
    let first = focus.saturating_sub(2);
    let offset: usize = lines[..first].iter().map(|line| line.len()).sum();
    (
        clip(&code[offset..], max).to_owned(),
        start_line + first,
        true,
    )
}
fn fit(result: &mut GraphResult, max: usize) -> Result<()> {
    loop {
        let length = serde_json::to_vec(result)?.len();
        if length <= max {
            return Ok(());
        }
        result.truncated = true;
        if let Some(symbol) = result
            .files
            .iter_mut()
            .rev()
            .flat_map(|file| file.symbols.iter_mut().rev())
            .find(|symbol| !symbol.snippet.is_empty())
        {
            let room = symbol
                .snippet
                .len()
                .saturating_sub(length - max)
                .min(symbol.snippet.len() / 2);
            symbol.snippet = clip(&symbol.snippet, room).to_owned();
            symbol.snippet_truncated = true;
            continue;
        }
        if !result.edges.is_empty() {
            result.edges.pop();
            continue;
        }
        if !result.impact.is_empty() {
            result.impact.pop();
            continue;
        }
        if !result.affected_tests.is_empty() {
            result.affected_tests.pop();
            continue;
        }
        if let Some(file) = result.files.last_mut() {
            result.path.clear();
            file.symbols.pop();
            if file.symbols.is_empty() {
                result.files.pop();
            }
            continue;
        }
        if !result.path.is_empty() {
            result.path.clear();
            continue;
        }
        if !result.roots.is_empty() {
            result.roots.pop();
            continue;
        }
        anyhow::bail!("response metadata exceeds max_bytes");
    }
}
