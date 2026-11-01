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

#[path = "analysis.rs"]
mod analysis;
#[path = "context_adapter.rs"]
mod context_adapter;
pub use context_adapter::ContextResult;
#[path = "hierarchy_adapter.rs"]
mod hierarchy_adapter;
pub use hierarchy_adapter::HierarchyResult;
#[path = "graph_query_adapter.rs"]
mod graph_query_adapter;
#[path = "resolution.rs"]
mod resolution;
#[path = "viewer.rs"]
pub mod viewer;
pub use analysis::{
    CyclesResult, DeadCodeEntry, DeadCodeExclusions, DeadCodeOptions, DeadCodeReport,
    DeadCodeResult, MetricsResult, NodeMetrics,
};
pub use graph_query_adapter::{GraphOperation, GraphQueryOptions, GraphQueryResult};
pub(crate) use resolution::{
    ProjectModules, PythonModules, is_project_manifest, js_import_mappings, js_imports,
    type_parameters, workspace_snapshot,
};

const MAX_CANDIDATES: usize = 50;

#[cfg(test)]
mod snapshot_limits {
    use super::*;

    #[test]
    fn complete_committed_graph_survives_previous_snapshot_ceilings() -> Result<()> {
        let temporary = tempfile::tempdir()?;
        let root = temporary.path().join("project");
        std::fs::create_dir(&root)?;
        let mut engine = Engine::open(&root, Some(&temporary.path().join("index.sqlite")))?;
        let source = "key: value\n";
        let mut parsed = crate::parser::parse(Path::new("bulk.yaml"), source)?;
        parsed.extraction = None;
        let qualified = "q".repeat(640);
        for index in 0..100_001 {
            parsed.symbols.push(Symbol {
                name: format!("marker_{index}"),
                kind: "variable".into(),
                start_line: 1,
                end_line: 1,
                start_byte: 0,
                end_byte: 0,
                qualified_name: qualified.clone(),
                container: None,
                exports: Vec::new(),
            });
        }
        let transaction = engine.connection.transaction()?;
        crate::index::store_file(&transaction, "bulk.yaml", source, "fixture", &parsed)?;
        transaction.commit()?;
        drop(parsed);
        let metadata_bytes: i64 = engine.connection.query_row(
            "SELECT length(analysis) FROM files WHERE path='bulk.yaml'",
            [],
            |row| row.get(0),
        )?;
        assert!(metadata_bytes > 64 * 1024 * 1024);
        let mut graph = Graph::load(&engine.connection, 0)?;
        assert_eq!(graph.nodes.len(), 100_002);
        let last = graph.names["marker_100000"][0];
        assert_eq!(graph.nodes[last].symbol.name, "marker_100000");
        assert!(!graph.truncated);
        for column in 0..200_001 {
            graph.push_edge(Edge {
                column: Some(column),
                from: 0,
                to: Some(last),
                target: "marker_100000".into(),
                line: 1,
                kind: "calls".into(),
                method: "syntax",
                confidence: "high",
                ambiguous: false,
            });
        }
        assert_eq!(graph.outgoing[0].len(), 200_001);
        assert_eq!(graph.incoming[last].len(), 200_001);
        assert!(!graph.truncated);
        Ok(())
    }
}

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
            limit: 200,
            max_bytes: usize::MAX,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<u32>,
    pub from: i64,
    pub to: Option<i64>,
    pub target: String,
    pub line: usize,
    pub kind: String,
    pub resolution: &'static str,
    pub confidence: &'static str,
    pub ambiguous: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provenance: Option<String>,
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
    source: String,
    bindings: resolution::Bindings,
    browser: bool,
    script_imports: Vec<resolution::ScriptImport>,
    nodes: Vec<usize>,
    qualified: HashMap<String, Vec<usize>>,
    exports: HashMap<String, Vec<usize>>,
}
struct Node {
    id: i64,
    ordinal: usize,
    path: String,
    symbol: Symbol,
}
struct Edge {
    column: Option<u32>,
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
    folded_names: HashMap<String, Vec<usize>>,
    members: HashMap<(usize, String), Vec<usize>>,
    go_modules: resolution::GoModules,
    workspaces: Option<resolution::WorkspacePackages>,
    packages: resolution::PackageScopes,
    aliases: resolution::Aliases,
    python_modules: Option<PythonModules>,
    cpp_imports: resolution::cpp_imports::CppImports,
    viewer_project: std::cell::OnceCell<&'static str>,
    manifests: BTreeMap<String, String>,
    workspace_directories: Vec<String>,
    auxiliary_sources: BTreeMap<String, String>,
    frameworks: Vec<String>,
    react_bindings: resolution::ReactBindings,
    cics_transactions: HashMap<String, usize>,
    infrastructure_work: Cell<usize>,
    reference_spellings: HashMap<(usize, String, String, usize, usize), usize>,
    framework_bindings: resolution::FrameworkBindings,
    go_packages: HashMap<String, Vec<String>>,
    edges: Vec<Edge>,
    canonical_edges: Vec<Option<prometeu_extract::Edge>>,
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
            !query.trim().is_empty() && query.encode_utf16().count() <= 10_000,
            "query must contain 1 to 10000 UTF-16 units"
        );
        ensure!(self.depth > 0, "depth must be positive");
        ensure!(
            self.line
                .is_none_or(|line| line > 0 && line <= i64::MAX as usize)
                && (self.line.is_none() || self.path.is_some()),
            "line must be positive and requires path"
        );
        if let Some(path) = &self.path {
            ensure!(
                !path.is_empty()
                    && path.encode_utf16().count() <= 4096
                    && Path::new(path)
                        .components()
                        .all(|part| matches!(part, Component::Normal(_) | Component::CurDir)),
                "path must be relative and cannot contain parent components"
            );
        }
        if let Some(target) = &self.target {
            ensure!(
                !target.trim().is_empty() && target.encode_utf16().count() <= 10_000,
                "target must contain 1 to 10000 UTF-16 units"
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
        let manifests = resolution::load_manifests(connection)?;
        let root: String =
            connection.query_row("SELECT value FROM metadata WHERE key='root'", [], |row| {
                row.get(0)
            })?;
        let mut graph = Self {
            generation,
            files: BTreeMap::new(),
            nodes: Vec::new(),
            ids: HashMap::new(),
            names: HashMap::new(),
            folded_names: HashMap::new(),
            members: HashMap::new(),
            go_modules: resolution::GoModules::load(connection)?,
            workspaces: resolution::WorkspacePackages::load(connection, &manifests)?,
            packages: resolution::PackageScopes::new(&manifests)?,
            aliases: resolution::Aliases::new(&manifests, &root)?,
            python_modules: None,
            cpp_imports: Default::default(),
            viewer_project: Default::default(),
            manifests: BTreeMap::new(),
            workspace_directories: resolution::cargo_workspace::load_directories(connection)?,
            auxiliary_sources: crate::auxiliary_sources::load(connection)?,
            frameworks: crate::frameworks::active(connection)?,
            react_bindings: Default::default(),
            cics_transactions: HashMap::new(),
            infrastructure_work: Cell::new(0),
            reference_spellings: HashMap::new(),
            framework_bindings: resolution::FrameworkBindings::default(),
            go_packages: HashMap::new(),
            edges: Vec::new(),
            canonical_edges: Vec::new(),
            outgoing: Vec::new(),
            incoming: Vec::new(),
            dependents: BTreeMap::new(),
            truncated: false,
            resolution_truncated: Cell::new(false),
        };
        // Cache the complete committed graph; query budgets apply after resolution.
        let mut statement = connection.prepare("SELECT path, analysis FROM files ORDER BY path")?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            let path: String = row.get(0)?;
            let metadata: String = row.get(1)?;
            let mut analysis: ParsedFile =
                serde_json::from_str(&metadata).context("invalid cached syntax metadata")?;
            let source: String = connection.query_row(
                "SELECT code FROM symbols WHERE path=?1 AND kind='file' ORDER BY ordinal LIMIT 1",
                [&path],
                |row| row.get(0),
            )?;
            crate::parser::recover_owned_call_sites(
                std::path::Path::new(&path),
                &source,
                &mut analysis,
            )?;
            let bindings = resolution::Bindings::read(&analysis.language, &source)?;
            let script_imports = resolution::script_imports(&analysis.language, &source)?;
            let browser = graph.workspaces.is_some()
                && resolution::browser_source(&analysis.language, &source)?;
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
                for name in [&symbol.name, &symbol.qualified_name] {
                    let found = graph.folded_names.entry(name.to_lowercase()).or_default();
                    if found.last() != Some(&index) {
                        found.push(index);
                    }
                }
                if symbol.qualified_name != symbol.name {
                    graph
                        .names
                        .entry(symbol.qualified_name.clone())
                        .or_default()
                        .push(index);
                }
                graph.nodes.push(Node {
                    id,
                    ordinal,
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
                    .entry(resolution::name_key(
                        &analysis.language,
                        &symbol.qualified_name,
                    ))
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
            if source.contains("export")
                && matches!(
                    analysis.language.as_str(),
                    "typescript"
                        | "tsx"
                        | "javascript"
                        | "jsx"
                        | "arkts"
                        | "vue"
                        | "svelte"
                        | "astro"
                )
            {
                let aliases = crate::frameworks::frontend::react_exports::local(&source)?;
                let mut local_names = HashMap::new();
                let priority = |index: usize| {
                    let node = &graph.nodes[index];
                    let metadata = analysis
                        .extraction
                        .as_ref()
                        .and_then(|extraction| extraction.nodes.get(node.ordinal));
                    (
                        metadata.is_none_or(|node| node.qualified_name != node.name),
                        node.symbol.start_line,
                        metadata.map(|node| node.id.as_str()),
                    )
                };
                if !aliases.is_empty() {
                    for &index in &nodes {
                        local_names
                            .entry(graph.nodes[index].symbol.name.as_str())
                            .and_modify(|current| {
                                if priority(index) < priority(*current) {
                                    *current = index;
                                }
                            })
                            .or_insert(index);
                    }
                }
                for alias in aliases {
                    if exports.contains_key(&alias.exported_name) {
                        continue;
                    }
                    if let Some(&target) = local_names.get(alias.local_name.as_str()) {
                        exports.insert(alias.exported_name, vec![target]);
                    }
                }
            }
            if analysis.language == "go" {
                graph
                    .go_packages
                    .entry(
                        path.rsplit_once('/')
                            .map_or("", |(directory, _)| directory)
                            .into(),
                    )
                    .or_default()
                    .push(path.clone());
            }
            graph.files.insert(
                path,
                File {
                    analysis,
                    source,
                    bindings,
                    browser,
                    script_imports,
                    nodes,
                    qualified,
                    exports,
                },
            );
        }
        graph.outgoing.resize_with(graph.nodes.len(), Vec::new);
        graph.incoming.resize_with(graph.nodes.len(), Vec::new);
        graph.cpp_imports = resolution::cpp_imports::CppImports::new(graph.files.keys().cloned());
        graph
            .cpp_imports
            .configure(&root, &manifests, &graph.workspace_directories);
        graph.python_modules = Some(PythonModules::new(
            graph.files.keys().cloned(),
            &manifests,
            &graph
                .files
                .values()
                .flat_map(|file| {
                    file.analysis
                        .extraction
                        .iter()
                        .flat_map(|extraction| extraction.nodes.iter().cloned())
                })
                .collect::<Vec<_>>(),
            graph
                .files
                .iter()
                .filter(|(_, file)| file.analysis.language == "python")
                .map(|(path, file)| (path.clone(), file.analysis.imports.clone()))
                .collect(),
        )?);
        graph.manifests = manifests;
        graph.initialize_import_bindings();
        graph.initialize_reference_spellings()?;
        graph.initialize_cics()?;
        graph.initialize_react()?;
        graph.build_edges()?;
        graph.retain_canonical_edges();
        graph.apply_navigation()?;
        graph.apply_synthesis()?;
        // File impact follows resolved cross-file relationships, including
        // implicit package calls and synthesized runtime wiring.
        for edge in &graph.edges {
            if edge.kind == "contains" {
                continue;
            }
            if let Some(to) = edge.to {
                let from = &graph.nodes[edge.from].path;
                let target = &graph.nodes[to].path;
                if from != target {
                    graph
                        .dependents
                        .entry(target.clone())
                        .or_default()
                        .push(from.clone());
                }
            }
        }
        for dependents in graph.dependents.values_mut() {
            dependents.sort();
            dependents.dedup();
        }
        graph.truncated |= graph.resolution_truncated.get()
            || graph.files.values().any(|file| file.bindings.truncated());
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
        self.module_files_as(from, module, &self.files[from].analysis.language)
    }

    fn module_files_as(&self, from: &str, module: &str, language: &str) -> Vec<String> {
        if matches!(language, "c" | "cpp") {
            return self
                .cpp_imports
                .resolve_as(module, from, language)
                .into_iter()
                .collect();
        }
        if let Some(found) = self.esm_module(from, module, language) {
            return found;
        }
        let parent = from.rsplit_once('/').map_or("", |(parent, _)| parent);
        if language == "go" {
            let Some(directory) = self.go_modules.package(from, module) else {
                return Vec::new();
            };
            return self
                .go_packages
                .get(&directory)
                .cloned()
                .unwrap_or_default();
        }
        let mut stems = Vec::new();
        if language == "python" {
            return self
                .python_modules
                .as_ref()
                .and_then(|modules| modules.resolve(module, from))
                .into_iter()
                .collect();
        } else if module.starts_with('.') {
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
        } else if matches!(language, "java" | "kotlin" | "scala" | "csharp") {
            let stem = module.replace('.', "/");
            // A package's source root varies; only accept a complete path suffix.
            for candidate in self.files.keys() {
                let without_extension = candidate
                    .rsplit_once('.')
                    .map_or(candidate.as_str(), |(p, _)| p);
                if without_extension == stem || without_extension.ends_with(&format!("/{stem}")) {
                    stems.push(candidate.clone());
                }
            }
        } else if matches!(language, "c" | "cpp" | "php" | "lua" | "luau" | "r") {
            if let Some(path) = normalize_path(&format!("{parent}/{module}")) {
                stems.push(path);
            }
            if let Some(path) = normalize_path(module) {
                stems.push(path);
            }
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
                "ts", "tsx", "js", "jsx", "mjs", "cjs", "mts", "cts", "rs", "rb", "py", "pyw",
                "go", "java", "cs", "php", "swift", "kt", "kts", "scala", "sc", "dart", "c", "h",
                "cpp", "hpp", "lua", "luau", "r", "R",
            ] {
                candidates.push(format!("{stem}.{extension}"));
                candidates.push(format!("{stem}/index.{extension}"));
            }
            candidates.push(format!("{stem}/mod.rs"));
            candidates.push(format!("{stem}/__init__.py"));
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
        if name == "default"
            && found.is_empty()
            && let Some(&component) = file.nodes.iter().find(|&&i| {
                self.nodes[i].symbol.kind == "component" && !self.nodes[i].symbol.exports.is_empty()
            })
        {
            found.push(component);
        }
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
            let found = file
                .qualified
                .get(&resolution::name_key(&file.analysis.language, &qualified))
                .cloned()
                .unwrap_or_default();
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
            .named(path, name)
            .into_iter()
            .filter(|&index| !matches!(self.nodes[index].symbol.kind.as_str(), "file" | "import"))
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
            .get(&(
                owner,
                resolution::name_key(&self.files[&self.nodes[owner].path].analysis.language, name),
            ))
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
        if let Some(result) = self.resolve_evidence(path, caller, reference) {
            return result;
        }
        let node = &self.nodes[caller];
        let scope = reference
            .owner
            .as_deref()
            .unwrap_or(&node.symbol.qualified_name);
        let bare_instance = reference.qualifier.as_deref() == Some("this")
            && matches!(
                self.files[path].analysis.language.as_str(),
                "typescript" | "tsx" | "javascript" | "jsx"
            )
            && self
                .canonical_reference(caller, reference)
                .is_some_and(|raw| raw.reference_name == reference.target);
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
                } else if bare_instance {
                    "scope_match"
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
                        let method = if bare_instance {
                            "scope_match"
                        } else {
                            "receiver_match"
                        };
                        return (found, method);
                    }
                    container = name.rsplit_once("::").map(|(parent, _)| parent);
                }
                return (Vec::new(), "receiver_match");
            }
            if let Some(import) = self.visible_import(path, scope, qualifier) {
                if import.imported == "*" {
                    let constructor = reference.kind == "instantiates"
                        && matches!(
                            self.files[path].analysis.language.as_str(),
                            "typescript" | "tsx" | "javascript" | "jsx"
                        )
                        && self
                            .canonical_reference(caller, reference)
                            .is_some_and(|raw| raw.reference_name == reference.target);
                    return (
                        self.imported(path, import, &reference.target),
                        if constructor {
                            "scope_match"
                        } else {
                            "import_match"
                        },
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
                self.named(path, &reference.target)
                    .into_iter()
                    .take(MAX_CANDIDATES)
                    .collect(),
                "name_match",
            );
        }
        self.lookup(path, scope, &reference.target)
    }

    fn push_edge(&mut self, edge: Edge) {
        let index = self.edges.len();
        self.outgoing[edge.from].push(index);
        if let Some(to) = edge.to {
            self.incoming[to].push(index);
        }
        self.edges.push(edge);
    }

    fn build_edges(&mut self) -> Result<()> {
        let paths: Vec<_> = self.files.keys().cloned().collect();
        let canonical: HashMap<String, usize> = self
            .files
            .values()
            .flat_map(|file| {
                file.analysis
                    .extraction
                    .iter()
                    .flat_map(|extraction| extraction.nodes.iter().zip(&file.nodes))
                    .map(|(node, &index)| (node.id.clone(), index))
            })
            .collect();
        for path in &paths {
            let edges: Vec<_> = self.files[path]
                .analysis
                .extraction
                .iter()
                .flat_map(|extraction| extraction.edges.iter())
                .filter_map(|edge| {
                    Some((
                        *canonical.get(&edge.source)?,
                        *canonical.get(&edge.target)?,
                        edge.kind.clone(),
                        edge.line,
                    ))
                })
                .collect();
            for (from, to, kind, line) in edges {
                self.push_edge(Edge {
                    column: None,
                    from,
                    to: Some(to),
                    target: self.nodes[to].symbol.name.clone(),
                    line: line.map_or(self.nodes[from].symbol.start_line, |line| line as usize),
                    kind,
                    method: "syntax",
                    confidence: "high",
                    ambiguous: false,
                });
            }
        }
        for index in 0..self.nodes.len() {
            let node = &self.nodes[index];
            if let Some(container) = &node.symbol.container {
                let owners: Vec<_> = self.files[&node.path]
                    .qualified
                    .get(&resolution::name_key(
                        &self.files[&node.path].analysis.language,
                        container,
                    ))
                    .into_iter()
                    .flatten()
                    .copied()
                    .filter(|&owner| owner != index)
                    .take(MAX_CANDIDATES)
                    .collect();
                for owner in owners {
                    // Canonical JS ownership excludes object-literal functions
                    // that merely occur inside another callable's source range.
                    if matches!(
                        self.files[&self.nodes[index].path]
                            .analysis
                            .language
                            .as_str(),
                        "typescript" | "tsx" | "javascript" | "jsx"
                    ) && self.metadata(index).is_some()
                        && self.metadata(owner).is_some()
                        && !self.outgoing[owner].iter().any(|&edge| {
                            self.edges[edge].to == Some(index)
                                && self.edges[edge].kind == "contains"
                        })
                    {
                        continue;
                    }
                    if self.outgoing[owner].iter().any(|&edge| {
                        self.edges[edge].to == Some(index) && self.edges[edge].kind == "contains"
                    }) {
                        continue;
                    }
                    self.push_edge(Edge {
                        column: None,
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
                    .entry((
                        owner,
                        resolution::name_key(
                            &self.files[&path].analysis.language,
                            &self.nodes[index].symbol.name,
                        ),
                    ))
                    .or_default()
                    .push(index);
                if !local {
                    self.push_edge(Edge {
                        column: None,
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
        self.initialize_go_members();
        self.initialize_framework_bindings(true)?;
        // Inheritance must exist before resolving calls to inherited members.
        for inheritance in [true, false] {
            if !inheritance {
                self.initialize_framework_bindings(false)?;
                self.build_import_edges();
            }
            let type_targets = self.visible_type_targets(inheritance)?;
            for path in &paths {
                let references: Vec<_> = self
                    .resolution_references(path)
                    .into_iter()
                    .filter(|reference| {
                        matches!(reference.kind.as_str(), "extends" | "implements") == inheritance
                    })
                    .collect();
                for reference in references {
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
                        let (mut candidates, mut method) = self.resolve(path, from, &reference);
                        Self::apply_type_visibility(
                            &type_targets,
                            from,
                            &reference,
                            &mut candidates,
                        );
                        candidates
                            .retain(|&to| self.compatible_reference(path, from, &reference, to));
                        if owner_method == "name_match" {
                            method = "name_match";
                        }
                        candidates.sort_unstable();
                        candidates.dedup();
                        let multiple_binding = self
                            .canonical_reference(from, &reference)
                            .and_then(|raw| {
                                self.cached_framework_reference(
                                    from,
                                    &raw.reference_name,
                                    &raw.reference_kind,
                                    raw.line as usize,
                                    Some(raw.column as usize),
                                )
                            })
                            .is_some_and(|targets| {
                                targets.len() > 1
                                    && targets.iter().all(|&to| {
                                        self.canonical_reference(from, &reference)
                                            .and_then(|raw| {
                                                self.cached_framework_metadata(
                                                    from,
                                                    to,
                                                    &raw.reference_name,
                                                    raw.line as usize,
                                                    Some(raw.column),
                                                )
                                            })
                                            .is_none_or(|answer| !answer.uncertain)
                                    })
                            });
                        let ambiguous =
                            owner_ambiguous || (candidates.len() > 1 && !multiple_binding);
                        if candidates.len() >= MAX_CANDIDATES {
                            self.truncated = true;
                            candidates.truncate(MAX_CANDIDATES);
                        }
                        if candidates.is_empty() {
                            self.push_edge(Edge {
                                column: reference.column.map(|column| column as u32),
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
                                    column: reference.column.map(|column| column as u32),
                                    from,
                                    to: Some(to),
                                    target: reference.target.clone(),
                                    line: reference.line,
                                    kind: self.resolved_kind(path, from, to, &reference).into(),
                                    method,
                                    confidence: if ambiguous {
                                        "low"
                                    } else if let Some(answer) =
                                        self.canonical_reference(from, &reference).and_then(|raw| {
                                            self.cached_framework_metadata(
                                                from,
                                                to,
                                                &raw.reference_name,
                                                raw.line as usize,
                                                Some(raw.column),
                                            )
                                        })
                                    {
                                        if answer.by_name {
                                            "low"
                                        } else if answer.confidence >= 0.7 {
                                            "medium"
                                        } else {
                                            "low"
                                        }
                                    } else if method == "name_match" {
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
        Ok(())
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
                let mut statement = connection.prepare("SELECT s.id FROM symbols_fts JOIN symbols s ON s.id=symbols_fts.rowid WHERE symbols_fts MATCH ?1 AND (?2 IS NULL OR s.path=?2) AND (?3 IS NULL OR (s.start_line<=?3 AND s.end_line>=?3)) ORDER BY bm25(symbols_fts,10.0,8.0,2.0,0.5,1.0),s.path,s.start_line LIMIT ?4")?;
                let ids = statement.query_map(
                    params![
                        expression,
                        path,
                        options.line.map(|line| line as i64),
                        options.limit.saturating_add(1).min(i64::MAX as usize) as i64
                    ],
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
            column: self.canonical_edges[index]
                .as_ref()
                .and_then(|edge| edge.column)
                .or(edge.column),
            from: self.nodes[edge.from].id,
            to: edge.to.map(|to| self.nodes[to].id),
            target: edge.target.clone(),
            line: edge.line,
            kind: edge.kind.clone(),
            resolution: edge.method,
            confidence: edge.confidence,
            ambiguous: edge.ambiguous,
            metadata: self.canonical_edges[index]
                .as_ref()
                .and_then(|edge| edge.metadata.clone()),
            provenance: self.canonical_edges[index]
                .as_ref()
                .and_then(|edge| edge.provenance.clone()),
        }
    }

    fn walk(
        &self,
        roots: &[usize],
        depth: usize,
        reverse: bool,
        kinds: &[&str],
        limit: usize,
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
                edges.push(index);
                if let Some(target) = target
                    && seen.insert(target)
                {
                    if seen.len() > limit {
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
                let edge = &self.edges[index];
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
        while let Some((path, distance)) = queue.pop_front() {
            for dependent in self.dependents.get(&path).into_iter().flatten() {
                if seen.contains(dependent) {
                    continue;
                }
                if distance == depth {
                    truncated = true;
                    continue;
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

    fn impact_walk(&self, roots: &[usize], depth: usize) -> (Vec<usize>, Vec<usize>, bool) {
        let mut distance: HashMap<_, _> = roots.iter().map(|&node| (node, 0)).collect();
        let mut queue: VecDeque<_> = roots.iter().map(|&node| (node, 0)).collect();
        let mut nodes = Vec::new();
        let mut edges = Vec::new();
        let mut seen_edges = HashSet::new();
        let mut truncated = false;
        while let Some((node, level)) = queue.pop_front() {
            if distance.get(&node).is_some_and(|&nearer| nearer < level) {
                continue;
            }
            let container = matches!(
                self.nodes[node].symbol.kind.as_str(),
                "class"
                    | "interface"
                    | "struct"
                    | "union"
                    | "trait"
                    | "protocol"
                    | "module"
                    | "enum"
            );
            let contained = if container {
                self.outgoing[node].as_slice()
            } else {
                &[]
            };
            let children = contained
                .iter()
                .copied()
                .filter(|&i| self.edges[i].kind == "contains")
                .filter_map(|i| self.edges[i].to.map(|node| (i, node, level)));
            let incoming = self.incoming[node]
                .iter()
                .copied()
                .filter(|&i| self.edges[i].kind != "contains")
                .map(|i| (i, self.edges[i].from, level + 1));
            for (edge, next, next_level) in children.chain(incoming) {
                if level >= depth {
                    truncated |= !distance.contains_key(&next);
                    continue;
                }
                if !distance.contains_key(&next) && distance.len() >= 10_000
                    || edges.len() >= 50_000
                {
                    return (nodes, edges, true);
                }
                if seen_edges.insert(edge) {
                    edges.push(edge);
                }
                if distance.get(&next).is_none_or(|&old| next_level < old) {
                    if distance.insert(next, next_level).is_none() {
                        nodes.push(next);
                    }
                    if next_level == level {
                        queue.push_front((next, next_level));
                    } else {
                        queue.push_back((next, next_level));
                    }
                }
            }
        }
        (nodes, edges, truncated)
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
                let (impacted, edges, truncated) = if mode == Some(GraphMode::Implementations) {
                    self.walk(
                        &roots,
                        options.depth,
                        true,
                        &["extends", "implements"],
                        options.limit.max(1000),
                    )
                } else {
                    self.impact_walk(&roots, options.depth)
                };
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
                        &[
                            "calls",
                            "references",
                            "imports",
                            "instantiates",
                            "navigates",
                            "extends",
                            "implements",
                            "contains",
                        ],
                        options.limit.max(1000),
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
            if !resolution::call_edge(&edge.kind) {
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
            if max_bytes != usize::MAX && serde_json::to_vec(&result)?.len() > max_bytes {
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
    if max == usize::MAX {
        return Ok(());
    }
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
