//! Resolution rules checked against independently persisted reference graphs.

#[path = "resolution/cpp.rs"]
mod cpp;
#[path = "resolution/cpp_imports.rs"]
pub(crate) mod cpp_imports;

use super::{Edge, Graph};
use crate::parser::{Import, Reference};
use anyhow::{Context, Result, ensure};
use prometeu_extract::Node;
use rusqlite::{Connection, OptionalExtension};
use std::collections::BTreeMap;
use std::collections::{HashMap, HashSet};

#[path = "resolution/bindings.rs"]
mod bindings;
#[path = "resolution/function_refs.rs"]
mod function_refs;
#[path = "resolution/javascript.rs"]
mod javascript;
#[path = "resolution/pascal_values.rs"]
mod pascal_values;
#[path = "resolution/python.rs"]
mod python;
#[path = "resolution/python_modules.rs"]
pub(crate) mod python_modules;
#[path = "resolution/swift_visibility.rs"]
mod swift_visibility;
#[path = "resolution/type_parameters.rs"]
pub(crate) mod type_parameters;

#[path = "resolution/react.rs"]
mod react;
pub(super) use react::ReactBindings;

#[path = "resolution/framework_bindings.rs"]
mod framework_bindings;
pub(super) use framework_bindings::FrameworkBindings;
#[path = "resolution/components.rs"]
mod components;
#[path = "resolution/frameworks.rs"]
mod frameworks;
#[path = "resolution/infrastructure.rs"]
mod infrastructure;
#[cfg(test)]
#[path = "resolution/tier_tests.rs"]
mod tier_tests;
pub(super) use bindings::Bindings;
#[path = "resolution/aliases.rs"]
mod aliases;
#[path = "resolution/cargo_workspace.rs"]
pub(crate) mod cargo_workspace;
#[cfg(test)]
#[path = "resolution/constructor_tests.rs"]
mod constructor_tests;
#[cfg(test)]
#[path = "resolution/go_binding_tests.rs"]
mod go_binding_tests;
#[cfg(test)]
#[path = "resolution/vapor_tests.rs"]
mod vapor_tests;
#[path = "resolution/workspace.rs"]
mod workspace;
pub(super) use aliases::Aliases;
#[path = "resolution/modules.rs"]
mod modules;
pub(crate) use modules::ProjectModules;
pub(crate) use python_modules::PythonModules;
pub(super) use workspace::{WorkspacePackages, load_manifests};
#[path = "resolution/default_exports.rs"]
mod default_exports;
#[path = "resolution/js_imports.rs"]
pub(crate) mod js_imports;
pub(super) use js_imports::Mapping as ScriptImport;
pub(crate) use js_imports::extract as js_import_mappings;
pub(super) fn script_imports(language: &str, source: &str) -> Result<Vec<ScriptImport>> {
    if workspace::esm(language) {
        js_imports::extract(source)
    } else {
        Ok(Vec::new())
    }
}
#[path = "resolution/navigation.rs"]
mod navigation;
#[path = "resolution/packages.rs"]
mod packages;
pub(super) use packages::PackageScopes;
#[path = "resolution/workspace_snapshot.rs"]
mod workspace_snapshot;
pub(crate) use workspace_snapshot::{
    is_manifest as is_project_manifest, snapshot as workspace_snapshot,
};

pub(super) fn browser_source(language: &str, source: &str) -> Result<bool> {
    if workspace::esm(language) {
        workspace::browser_source(source)
    } else {
        Ok(false)
    }
}

pub(super) fn call_edge(kind: &str) -> bool {
    matches!(
        kind,
        "calls" | "references" | "imports" | "instantiates" | "navigates"
    )
}

pub(super) fn name_key(language: &str, name: &str) -> String {
    if matches!(
        language,
        "php" | "pascal" | "cfml" | "cfscript" | "cfquery" | "cobol" | "vbnet"
    ) {
        name.to_lowercase()
    } else {
        name.into()
    }
}

pub(super) struct GoModules(Vec<(String, String)>);
impl GoModules {
    pub(super) fn load(connection: &Connection) -> Result<Self> {
        let source: Option<String> = connection
            .query_row(
                "SELECT value FROM metadata WHERE key='resolution_go_modules'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        let Some(source) = source else {
            return Ok(Self(Vec::new()));
        };
        ensure!(
            source.len() <= 9 * 1024 * 1024,
            "cached Go module metadata exceeds limit"
        );
        let files: BTreeMap<String, String> =
            serde_json::from_str(&source).context("invalid cached Go module metadata")?;
        let mut modules = Vec::new();
        for (path, source) in files {
            if let Some(module) = source
                .lines()
                .filter_map(|line| line.split("//").next())
                .find_map(|line| {
                    let mut words = line.split_whitespace();
                    if words.next() != Some("module") {
                        return None;
                    }
                    let name = words.next()?.trim_matches(['\'', '"']);
                    (!name.is_empty() && words.next().is_none()).then(|| name.to_owned())
                })
            {
                modules.push((
                    path.rsplit_once('/').map_or("", |(dir, _)| dir).into(),
                    module,
                ));
            }
        }
        Ok(Self(modules))
    }
    pub(super) fn package(&self, from: &str, import: &str) -> Option<String> {
        let own = self
            .0
            .iter()
            .filter(|(dir, _)| dir.is_empty() || from.starts_with(&format!("{dir}/")))
            .max_by_key(|(dir, _)| dir.len());
        let (directory, module) = self
            .0
            .iter()
            .filter(|(_, module)| import == module || import.starts_with(&format!("{module}/")))
            .max_by_key(|candidate| (candidate.1.len(), Some(*candidate) == own))?;
        let tail = import[module.len()..].trim_start_matches('/');
        Some(if directory.is_empty() {
            tail.into()
        } else if tail.is_empty() {
            directory.clone()
        } else {
            format!("{directory}/{tail}")
        })
    }
}

fn code_family(language: &str) -> Option<&str> {
    Some(match language {
        "java" | "kotlin" | "scala" => "jvm",
        "c" | "cpp" | "objc" | "swift" => "native",
        "typescript" | "tsx" | "javascript" | "jsx" | "arkts" | "vue" | "svelte" | "astro" => "web",
        "csharp" | "razor" | "vbnet" => "dotnet",
        "cfml" | "cfscript" => "cfml",
        "lua" | "luau" => "lua",
        "python" | "go" | "rust" | "php" | "ruby" | "dart" | "r" | "erlang" | "pascal"
        | "solidity" | "nix" | "cobol" => language,
        _ => return None,
    })
}

impl Graph {
    pub(super) fn esm_module(
        &self,
        from: &str,
        module: &str,
        language: &str,
    ) -> Option<Vec<String>> {
        if !workspace::esm(language) {
            return None;
        }
        Some(
            modules::Modules {
                workspace: self.workspaces.as_ref(),
                aliases: &self.aliases,
                packages: &self.packages,
                browser: self.files[from].browser,
            }
            .resolve(from, module, language, |file| self.files.contains_key(file))
            .into_iter()
            .collect(),
        )
    }

    fn synthesis_imports(&self) -> Result<Vec<crate::synthesis::ImportBinding>> {
        let mut out = Vec::new();
        for (path, file) in &self.files {
            if file.analysis.language == "go" {
                out.extend(crate::synthesis::go_imports(path, &file.source)?);
            }
            if !workspace::esm(&file.analysis.language) {
                continue;
            }
            for binding in &file.script_imports {
                let resolved_path = self.module_files(path, &binding.source).into_iter().next();
                let external = resolved_path.is_none()
                    && !binding.source.starts_with('.')
                    && !["@/", "~/", "src/"]
                        .iter()
                        .any(|prefix| binding.source.starts_with(prefix))
                    && !self
                        .workspaces
                        .as_ref()
                        .is_some_and(|ws| ws.targets(&binding.source, Some(path)).is_some())
                    && self
                        .packages
                        .external(&binding.source, path, self.workspaces.as_ref());
                out.push(crate::synthesis::ImportBinding {
                    file: path.clone(),
                    local_name: binding.local_name.clone(),
                    imported_name: binding.exported_name.clone(),
                    source: binding.source.clone(),
                    is_namespace: binding.is_namespace,
                    is_default: binding.is_default,
                    mapped_path: None,
                    resolved_path,
                    external,
                });
            }
        }
        Ok(out)
    }

    pub(super) fn initialize_reference_spellings(&mut self) -> Result<()> {
        let mut spellings = HashMap::new();
        for file in self.files.values() {
            let Some(extraction) = &file.analysis.extraction else {
                continue;
            };
            let ids = file
                .nodes
                .iter()
                .filter_map(|&index| self.metadata(index).map(|node| (node.id.as_str(), index)))
                .collect::<HashMap<_, _>>();
            for (ordinal, reference) in extraction.unresolved_references.iter().enumerate() {
                let Some(&caller) = ids.get(reference.from_node_id.as_str()) else {
                    continue;
                };
                let name = reference
                    .reference_name
                    .replace("->", "::")
                    .replace('.', "::");
                spellings
                    .entry((
                        caller,
                        name,
                        reference.reference_kind.clone(),
                        reference.line as usize,
                        reference.column as usize,
                    ))
                    .or_insert(ordinal);
            }
        }
        self.reference_spellings = spellings;
        Ok(())
    }
    pub(super) fn canonical_reference(
        &self,
        caller: usize,
        reference: &Reference,
    ) -> Option<&prometeu_extract::UnresolvedReference> {
        let name = reference
            .qualifier
            .as_ref()
            .map_or_else(
                || reference.target.clone(),
                |qualifier| format!("{qualifier}::{}", reference.target),
            )
            .replace("->", "::")
            .replace('.', "::");
        let mut key = (
            caller,
            name,
            reference.kind.clone(),
            reference.line,
            reference.column?,
        );
        let ordinal = *self.reference_spellings.get(&key).or_else(|| {
            if (reference.qualifier.as_deref() != Some("this") && reference.kind != "instantiates")
                || !workspace::esm(&self.files[&self.nodes[caller].path].analysis.language)
            {
                return None;
            }
            key.1 = reference.target.clone();
            self.reference_spellings.get(&key)
        })?;
        self.files[&self.nodes[caller].path]
            .analysis
            .extraction
            .as_ref()?
            .unresolved_references
            .get(ordinal)
    }

    pub(super) fn initialize_go_members(&mut self) {
        for index in 0..self.nodes.len() {
            let node = &self.nodes[index];
            if node.symbol.kind != "method" || self.files[&node.path].analysis.language != "go" {
                continue;
            }
            let Some((receiver, _)) = node.symbol.qualified_name.rsplit_once("::") else {
                continue;
            };
            let directory = node.path.rsplit_once('/').map_or("", |(dir, _)| dir);
            let owners: Vec<_> = self
                .names
                .get(receiver)
                .into_iter()
                .flatten()
                .copied()
                .filter(|&owner| {
                    let target = &self.nodes[owner];
                    self.files[&target.path].analysis.language == "go"
                        && matches!(
                            target.symbol.kind.as_str(),
                            "struct" | "type_alias" | "interface"
                        )
                        && target.path.rsplit_once('/').map_or("", |(dir, _)| dir) == directory
                        && self.files[&target.path].bindings.package
                            == self.files[&node.path].bindings.package
                })
                .collect();
            if owners.len() > super::MAX_CANDIDATES {
                self.resolution_truncated.set(true);
            }
            for owner in owners.into_iter().take(super::MAX_CANDIDATES) {
                let members = self
                    .members
                    .entry((owner, node.symbol.name.clone()))
                    .or_default();
                if !members.contains(&index) {
                    members.push(index);
                }
            }
        }
    }
    pub(super) fn named(&self, path: &str, name: &str) -> Vec<usize> {
        if matches!(
            self.files[path].analysis.language.as_str(),
            "php" | "pascal" | "cfml" | "cfscript" | "cfquery" | "cobol" | "vbnet"
        ) {
            self.folded_names.get(&name.to_lowercase())
        } else {
            self.names.get(name)
        }
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|&index| {
            !(workspace::esm(&self.files[path].analysis.language)
                && workspace::esm(&self.files[&self.nodes[index].path].analysis.language)
                && self
                    .workspaces
                    .as_ref()
                    .is_some_and(|ws| ws.leaves_root(path, &self.nodes[index].path)))
        })
        .collect()
    }
    pub(super) fn retain_canonical_edges(&mut self) {
        let mut syntax = HashMap::new();
        let mut references: HashMap<(&str, usize), Vec<&prometeu_extract::UnresolvedReference>> =
            HashMap::new();
        for extraction in self
            .files
            .values()
            .filter_map(|file| file.analysis.extraction.as_ref())
        {
            for edge in &extraction.edges {
                syntax
                    .entry((
                        edge.source.as_str(),
                        edge.target.as_str(),
                        edge.kind.as_str(),
                    ))
                    .or_insert(edge);
            }
            for reference in &extraction.unresolved_references {
                references
                    .entry((reference.from_node_id.as_str(), reference.line as usize))
                    .or_default()
                    .push(reference);
            }
        }
        let mut columns = HashMap::new();
        let mut canonical = Vec::with_capacity(self.edges.len());
        for edge in &self.edges {
            let Some((from, to)) = self
                .metadata(edge.from)
                .zip(edge.to.and_then(|to| self.metadata(to)))
            else {
                canonical.push(None);
                continue;
            };
            if edge.method == "syntax"
                && let Some(raw) =
                    syntax.get(&(from.id.as_str(), to.id.as_str(), edge.kind.as_str()))
            {
                canonical.push(Some((*raw).clone()));
                continue;
            }
            let candidates = references.get(&(from.id.as_str(), edge.line));
            let matching: Vec<_> = candidates
                .into_iter()
                .flatten()
                .copied()
                .filter(|raw| {
                    if edge.column.is_some_and(|column| raw.column != column) {
                        return false;
                    }
                    let name = raw.reference_name.replace("->", "::").replace('.', "::");
                    raw.reference_name == edge.target
                        || name.rsplit("::").next() == Some(edge.target.as_str())
                })
                .collect();
            let next = columns
                .entry((from.id.as_str(), edge.line, edge.target.as_str()))
                .or_insert(0usize);
            let raw = matching.get(*next).or_else(|| matching.first()).copied();
            *next += 1;
            let canonical_method = match edge.method {
                "import_match" => "import",
                "framework_match" => "framework",
                "qualified_match" => "qualified-name",
                "receiver_match" | "typed_receiver_match" => "instance-method",
                "function_ref_match" => "function-ref",
                "scope_match" | "name_match" => "exact-match",
                method => method,
            };
            let mut metadata = serde_json::json!({"resolvedBy":canonical_method,"confidence":match edge.confidence {"high"=>1.0,"medium"=>0.9,"low"=>0.4,_=>0.0},"refName":raw.map_or(edge.target.as_str(),|raw|raw.reference_name.as_str())});
            if edge.method == "import_match"
                && edge.kind == "imports"
                && matches!(from.language.as_str(), "c" | "cpp")
                && crate::auxiliary_sources::normalize(
                    from.file_path
                        .rsplit_once('/')
                        .map_or("", |(parent, _)| parent),
                    raw.map_or(edge.target.as_str(), |raw| raw.reference_name.as_str()),
                )
                .as_deref()
                    == Some(to.file_path.as_str())
            {
                metadata["confidence"] = 0.92.into();
            }
            if edge.method == "typed_receiver_match" && !edge.ambiguous {
                metadata["confidence"] = 0.85.into();
            }
            if edge.method == "file-path"
                && edge.kind == "imports"
                && matches!(from.language.as_str(), "c" | "cpp")
            {
                let name = raw.map_or(edge.target.as_str(), |raw| raw.reference_name.as_str());
                metadata["confidence"] = if to.file_path == name {
                    0.95
                } else if to.file_path.ends_with(name) {
                    0.85
                } else {
                    0.7
                }
                .into();
            }
            if edge.method == "framework_match" && !edge.ambiguous {
                let name = raw.map_or(edge.target.as_str(), |raw| raw.reference_name.as_str());
                metadata["confidence"] = self
                    .framework_confidence(edge.from, edge.to.unwrap(), name, edge.line, edge.column)
                    .into();
                if !self
                    .cached_framework_metadata(
                        edge.from,
                        edge.to.unwrap(),
                        name,
                        edge.line,
                        edge.column,
                    )
                    .is_some_and(|answer| answer.secondary)
                    && let Some(framework) = self.framework_name(edge.from, raw)
                {
                    metadata["framework"] = framework.into();
                }
                if (from.id.ends_with(":vue") && name.starts_with("layout:"))
                    || self.react_layout(edge.from, edge.to.unwrap(), name)
                {
                    metadata["layout"] = true.into();
                }
            }
            if !edge.ambiguous
                && let Some(answer) = self.cached_framework_metadata(
                    edge.from,
                    edge.to.unwrap(),
                    raw.map_or(edge.target.as_str(), |raw| raw.reference_name.as_str()),
                    edge.line,
                    edge.column,
                )
            {
                if let Some(extra) = answer.extra.as_ref().and_then(serde_json::Value::as_object) {
                    metadata.as_object_mut().unwrap().extend(extra.clone());
                }
                metadata["resolvedBy"] = answer.resolved_by.into();
                metadata["confidence"] = answer.confidence.into();
            }
            if edge.ambiguous {
                metadata["ambiguous"] = true.into();
                if let Some(answer) = self.cached_framework_metadata(
                    edge.from,
                    edge.to.unwrap(),
                    raw.map_or(edge.target.as_str(), |raw| raw.reference_name.as_str()),
                    edge.line,
                    edge.column,
                ) && answer.uncertain
                {
                    metadata["resolvedBy"] = answer.resolved_by.into();
                }
            }
            if let Some(raw) = raw
                && raw.reference_kind != edge.kind
            {
                metadata["refKind"] = raw.reference_kind.clone().into();
            }
            if raw.is_some_and(|raw| raw.reference_kind == "function_ref") {
                metadata["fnRef"] = true.into();
            }
            canonical.push(Some(prometeu_extract::Edge {
                source: from.id.clone(),
                target: to.id.clone(),
                kind: edge.kind.clone(),
                line: Some(edge.line as u32),
                column: edge.column.or_else(|| raw.map(|raw| raw.column)),
                provenance: None,
                metadata: Some(metadata),
            }));
        }
        self.canonical_edges = canonical;
    }

    fn synthesis_import_reference(
        &self,
        reference: &prometeu_extract::UnresolvedReference,
        import_bindings: &HashMap<(&str, &str), &crate::synthesis::ImportBinding>,
        defaults: &std::cell::RefCell<HashMap<String, default_exports::DefaultExport>>,
    ) -> Result<Option<String>> {
        let Some(path) = reference.file_path.as_deref() else {
            return Ok(None);
        };
        let mut parts = reference.reference_name.split('.');
        let root = parts.next().unwrap_or("");
        let Some(binding) = import_bindings.get(&(path, root)) else {
            return Ok(None);
        };
        let requested = if binding.is_namespace {
            parts.next()
        } else if binding.is_default {
            Some("default")
        } else {
            Some(binding.imported_name.as_str())
        };
        let Some(requested) = requested else {
            return Ok(None);
        };
        let modules = self.module_files(path, &binding.source);
        if modules.len() != 1 {
            return Ok(None);
        }
        let mut found = self.exported(&modules[0], requested, &mut HashSet::new());
        if found.is_empty() && binding.is_default {
            let file = &self.files[&modules[0]];
            let mut defaults = defaults.borrow_mut();
            if !defaults.contains_key(&modules[0]) {
                let nodes: Vec<_> = file
                    .analysis
                    .extraction
                    .iter()
                    .flat_map(|extraction| extraction.nodes.iter())
                    .collect();
                defaults.insert(
                    modules[0].clone(),
                    default_exports::DefaultExport::read(&modules[0], &file.source, &nodes)?,
                );
            }
            found.extend(file.nodes.iter().copied().filter(|&index| {
                self.metadata(index).is_some_and(|node| {
                    matches!(
                        node.kind.as_str(),
                        "function" | "class" | "constant" | "variable" | "component"
                    ) && defaults[&modules[0]].matches(node)
                })
            }));
        }
        if found.len() == 1 && binding.is_default {
            let Some(target) = self.metadata(found[0]) else {
                return Ok(None);
            };
            let file = &self.files[&target.file_path];
            let mut defaults = defaults.borrow_mut();
            if !defaults.contains_key(&target.file_path) {
                let nodes: Vec<_> = file
                    .analysis
                    .extraction
                    .iter()
                    .flat_map(|extraction| extraction.nodes.iter())
                    .collect();
                defaults.insert(
                    target.file_path.clone(),
                    default_exports::DefaultExport::read(&target.file_path, &file.source, &nodes)?,
                );
            }
            if !defaults[&target.file_path].matches(target) {
                return Ok(None);
            }
        }
        Ok((found.len() == 1)
            .then(|| self.metadata(found[0]).map(|node| node.id.clone()))
            .flatten())
    }

    pub(super) fn apply_synthesis(&mut self) -> Result<()> {
        let nodes: Vec<_> = self
            .files
            .values()
            .filter_map(|file| file.analysis.extraction.as_ref())
            .flat_map(|extraction| extraction.nodes.iter().cloned())
            .collect();
        let edges: Vec<_> = self.canonical_edges.iter().flatten().cloned().collect();
        let sources = self
            .files
            .iter()
            .map(|(path, file)| (path.as_str(), file.source.as_str()))
            .collect();
        let resolved: HashSet<_> = edges
            .iter()
            .filter_map(|edge| {
                Some((
                    edge.source.as_str(),
                    edge.line?,
                    edge.column?,
                    edge.metadata.as_ref()?.get("refName")?.as_str()?,
                ))
            })
            .collect();
        let unresolved: Vec<_> = self
            .files
            .iter()
            .flat_map(|(path, file)| {
                file.analysis.extraction.iter().flat_map(move |extraction| {
                    extraction
                        .unresolved_references
                        .iter()
                        .map(move |reference| (path, file, reference))
                })
            })
            .filter(|(_, _, reference)| {
                !resolved.contains(&(
                    reference.from_node_id.as_str(),
                    reference.line,
                    reference.column,
                    reference.reference_name.as_str(),
                ))
            })
            .map(|(path, file, reference)| {
                let mut reference = reference.clone();
                reference.file_path = Some(path.clone());
                reference.language = Some(file.analysis.language.clone());
                reference
            })
            .collect();
        let module_dirs = self
            .workspaces
            .as_ref()
            .map_or_else(Vec::new, |ws| ws.module_dirs());
        let imports = self.synthesis_imports()?;
        let manifests = self
            .manifests
            .iter()
            .map(|(path, source)| (path.as_str(), source.as_str()))
            .collect();
        let mut import_bindings = HashMap::new();
        for binding in &imports {
            import_bindings
                .entry((binding.file.as_str(), binding.local_name.as_str()))
                .or_insert(binding);
        }
        let defaults = std::cell::RefCell::new(HashMap::new());
        let resolve_import = |reference: &prometeu_extract::UnresolvedReference| {
            self.synthesis_import_reference(reference, &import_bindings, &defaults)
        };
        let auxiliary_sources = self
            .auxiliary_sources
            .iter()
            .map(|(path, source)| (path.as_str(), source.as_str()))
            .collect();
        let environment = crate::synthesis::SynthesisEnvironment {
            auxiliary_sources: &auxiliary_sources,
            imports: &imports,
            manifests: &manifests,
            frameworks: &self.frameworks,
            resolve_import: &resolve_import,
            go_package_dir: &|from, source| self.go_modules.package(from, source),
            resolve_module: &|specifier, from, language| {
                let candidates = self.module_files_as(from, specifier, language);
                (candidates.len() == 1).then(|| candidates[0].clone())
            },
        };
        let derived = crate::synthesis::synthesize_with_imports(
            &nodes,
            &edges,
            &sources,
            &unresolved,
            &module_dirs,
            &environment,
        )?;
        let ids: HashMap<_, _> = self
            .nodes
            .iter()
            .enumerate()
            .filter_map(|(index, _)| self.metadata(index).map(|node| (node.id.clone(), index)))
            .collect();
        for edge in derived {
            let Some((&from, &to)) = ids.get(&edge.source).zip(ids.get(&edge.target)) else {
                continue;
            };
            self.push_edge(Edge {
                column: edge.column,
                from,
                to: Some(to),
                target: self.nodes[to].symbol.name.clone(),
                line: edge
                    .line
                    .map_or(self.nodes[from].symbol.start_line, |line| line as usize),
                kind: edge.kind.clone(),
                method: "synthesized",
                confidence: "medium",
                ambiguous: false,
            });
            self.canonical_edges.push(Some(edge));
        }
        Ok(())
    }
    pub(super) fn initialize_import_bindings(&mut self) {
        let paths: Vec<_> = self.files.keys().cloned().collect();
        for path in paths {
            if self.files[&path].analysis.language != "go" {
                continue;
            }
            let mut imports = Vec::new();
            for (alias, module) in &self.files[&path].bindings.imports {
                if alias.as_deref() == Some("_") {
                    continue;
                }
                let local = alias
                    .clone()
                    .or_else(|| {
                        self.module_files(&path, module)
                            .first()
                            .and_then(|target| self.files[target].bindings.package.clone())
                    })
                    .unwrap_or_else(|| module.rsplit('/').next().unwrap_or(module).into());
                imports.push(Import {
                    local: if local == "." { "*".into() } else { local },
                    imported: "*".into(),
                    module: module.clone(),
                    scope: String::new(),
                    reexport: false,
                });
            }
            self.files.get_mut(&path).unwrap().analysis.imports = imports;
        }
    }
    pub(super) fn resolve_evidence(
        &self,
        path: &str,
        caller: usize,
        reference: &Reference,
    ) -> Option<(Vec<usize>, &'static str)> {
        if let Some(found) = self
            .infrastructure_reference(path, reference)
            .or_else(|| self.framework_reference(path, caller, reference))
        {
            return Some(found);
        }

        let file = &self.files[path];
        if reference.qualifier.is_none()
            && reference.kind == "calls"
            && file
                .bindings
                .receiver(&reference.target, reference.line)
                .is_some()
        {
            return Some((Vec::new(), "scope_match"));
        }
        if file.analysis.language == "erlang" {
            let name = reference.qualifier.as_ref().map_or_else(
                || reference.target.clone(),
                |module| format!("{module}::{}", reference.target),
            );
            let exact = self.names.get(&name).cloned().unwrap_or_default();
            if !exact.is_empty() {
                return Some((exact, "qualified_match"));
            }
        }
        let qualifier = reference.qualifier.as_deref()?;
        if file.analysis.language == "swift" {
            let qualified = format!("{qualifier}::{}", reference.target).replace('.', "::");
            let targets: Vec<_> = self
                .names
                .get(&qualified)
                .into_iter()
                .flatten()
                .copied()
                .filter(|&index| {
                    self.metadata(index).is_some_and(|node| {
                        node.language == "swift"
                            && matches!(
                                node.kind.as_str(),
                                "class"
                                    | "struct"
                                    | "enum"
                                    | "interface"
                                    | "protocol"
                                    | "type_alias"
                            )
                    })
                })
                .collect();
            if !targets.is_empty() {
                return Some((targets, "qualified_match"));
            }
        }
        if matches!(file.analysis.language.as_str(), "cfml" | "cfscript")
            && let Some(receiver) = qualifier.strip_suffix("()")
        {
            let (owners, _) =
                self.lookup(path, &self.nodes[caller].symbol.qualified_name, receiver);
            let mut found = Vec::new();
            for owner in owners.into_iter().filter(|&owner| {
                matches!(
                    self.nodes[owner].symbol.kind.as_str(),
                    "class" | "component"
                )
            }) {
                found.extend(self.members(owner, &reference.target, &mut HashSet::new()));
            }
            return Some((found, "receiver_match"));
        }
        if file.analysis.language == "go"
            && !file.bindings.has(qualifier)
            && self
                .visible_import(path, &self.nodes[caller].symbol.qualified_name, qualifier)
                .is_none()
        {
            return Some((Vec::new(), "import_match"));
        }
        let binding = file.bindings.receiver(qualifier, reference.line)?;
        let Some(receiver) = binding else {
            return Some((Vec::new(), "receiver_match"));
        };
        let (owners, method) =
            self.lookup(path, &self.nodes[caller].symbol.qualified_name, receiver);
        let mut found = Vec::new();
        for owner in owners {
            if file.analysis.language == "go"
                && !receiver.contains("::")
                && (self.nodes[owner]
                    .path
                    .rsplit_once('/')
                    .map_or("", |(dir, _)| dir)
                    != path.rsplit_once('/').map_or("", |(dir, _)| dir)
                    || self.files[&self.nodes[owner].path].bindings.package
                        != file.bindings.package)
            {
                continue;
            }
            found.extend(self.members(owner, &reference.target, &mut HashSet::new()));
        }
        found.sort_unstable();
        found.dedup();
        Some((
            found,
            if method == "name_match" {
                "name_match"
            } else {
                "receiver_match"
            },
        ))
    }
    pub(super) fn metadata(&self, index: usize) -> Option<&Node> {
        let node = &self.nodes[index];
        self.files[&node.path]
            .analysis
            .extraction
            .as_ref()?
            .nodes
            .get(node.ordinal)
    }

    pub(super) fn compatible_reference(
        &self,
        path: &str,
        from: usize,
        reference: &Reference,
        target: usize,
    ) -> bool {
        let language = self.files[path].analysis.language.as_str();
        let target_language = self.files[&self.nodes[target].path]
            .analysis
            .language
            .as_str();
        let binding = self.canonical_reference(from, reference).and_then(|raw| {
            self.cached_framework_metadata(
                from,
                target,
                &raw.reference_name,
                raw.line as usize,
                Some(raw.column),
            )
        });
        let bridge = binding.is_some_and(|answer| {
            matches!(
                answer.framework,
                "swift-objc-bridge" | "react-native-bridge" | "expo-modules-js"
            )
        });
        if let (Some(from), Some(to)) = (code_family(language), code_family(target_language))
            && from != to
            && !bridge
        {
            return false;
        }
        if self.nodes[target].path != path
            && matches!(
                target_language,
                "kotlin" | "java" | "csharp" | "swift" | "scala" | "php"
            )
            && self
                .metadata(target)
                .is_some_and(|node| node.visibility.as_deref() == Some("private"))
        {
            return false;
        }
        if language == "go" && self.nodes[target].path != path && binding.is_none() {
            let target_path = &self.nodes[target].path;
            let same_package = path.rsplit_once('/').map_or("", |(dir, _)| dir)
                == target_path.rsplit_once('/').map_or("", |(dir, _)| dir)
                && self.files[path].bindings.package == self.files[target_path].bindings.package;
            if !same_package
                && !self.nodes[target]
                    .symbol
                    .name
                    .starts_with(|ch: char| ch.is_ascii_uppercase())
            {
                return false;
            }
            if reference.qualifier.is_none()
                && !same_package
                && !self.files[path].analysis.imports.iter().any(|import| {
                    import.local == "*"
                        && self
                            .module_files(path, &import.module)
                            .contains(target_path)
                })
            {
                return false;
            }
        }
        let kind = self.nodes[target].symbol.kind.as_str();
        if matches!(kind, "file" | "import") {
            return reference.kind == "imports"
                || (kind == "file"
                    && self
                        .canonical_reference(from, reference)
                        .is_some_and(|raw| {
                            self.cached_framework_reference(
                                from,
                                &raw.reference_name,
                                &reference.kind,
                                reference.line,
                                Some(raw.column as usize),
                            )
                            .is_some_and(|targets| targets.contains(&target))
                        }));
        }
        if language == "rust"
            && matches!(reference.kind.as_str(), "calls" | "function_ref")
            && matches!(kind, "module" | "namespace")
        {
            return false;
        }
        if matches!(
            reference.kind.as_str(),
            "extends" | "implements" | "instantiates"
        ) {
            return (language == "erlang" && reference.kind == "implements" && kind == "namespace")
                || matches!(
                    kind,
                    "class"
                        | "struct"
                        | "interface"
                        | "trait"
                        | "protocol"
                        | "enum"
                        | "type_alias"
                        | "union"
                        | "component"
                );
        }
        true
    }

    pub(super) fn resolved_kind<'a>(
        &self,
        path: &str,
        from: usize,
        to: usize,
        reference: &'a Reference,
    ) -> &'a str {
        if let Some(kind) = self
            .canonical_reference(from, reference)
            .and_then(|raw| {
                self.cached_framework_metadata(
                    from,
                    to,
                    &raw.reference_name,
                    raw.line as usize,
                    Some(raw.column),
                )
            })
            .and_then(|metadata| metadata.edge_kind)
        {
            return kind;
        }
        let target = self.nodes[to].symbol.kind.as_str();
        match reference.kind.as_str() {
            "function_ref" => "references",
            "extends"
                if matches!(target, "interface" | "protocol")
                    && !matches!(
                        self.nodes[from].symbol.kind.as_str(),
                        "interface" | "protocol"
                    ) =>
            {
                "implements"
            }
            "calls"
                if self.files[path].analysis.language == "go"
                    && matches!(
                        target,
                        "class" | "struct" | "interface" | "type_alias" | "enum"
                    ) =>
            {
                "references"
            }
            "calls"
                if self.files[path].analysis.language == "pascal"
                    && reference.qualifier.is_none()
                    && matches!(
                        target,
                        "class" | "struct" | "interface" | "type_alias" | "enum"
                    ) =>
            {
                "references"
            }
            "calls" if matches!(target, "class" | "struct" | "union") => "instantiates",
            kind => kind,
        }
    }

    pub(super) fn resolution_references(&self, path: &str) -> Vec<Reference> {
        let file = &self.files[path];
        let mut references = file.analysis.references.clone();
        let Some(extraction) = &file.analysis.extraction else {
            return references;
        };
        let ids: HashMap<_, _> = extraction
            .nodes
            .iter()
            .enumerate()
            .map(|(i, node)| (node.id.as_str(), i))
            .collect();
        let field_claim = |raw: &prometeu_extract::UnresolvedReference| {
            raw.reference_kind == "calls"
                && raw.reference_name.starts_with("this.")
                && matches!(
                    file.analysis.language.as_str(),
                    "typescript" | "tsx" | "javascript" | "jsx"
                )
                && ids
                    .get(raw.from_node_id.as_str())
                    .and_then(|&ordinal| file.nodes.get(ordinal))
                    .is_some_and(|&from| {
                        self.cached_framework_reference(
                            from,
                            &raw.reference_name,
                            &raw.reference_kind,
                            raw.line as usize,
                            Some(raw.column as usize),
                        )
                        .is_some()
                    })
        };
        let field_sites = extraction
            .unresolved_references
            .iter()
            .filter(|raw| field_claim(raw))
            .map(|raw| (raw.reference_name.replace('.', "::"), raw.line as usize))
            .collect::<HashSet<_>>();
        references.retain(|reference| {
            // Calls skipped by the canonical walker inside class fields must
            // not become file-level calls through the legacy projection.
            if reference.kind == "calls"
                && matches!(
                    file.analysis.language.as_str(),
                    "typescript" | "tsx" | "javascript" | "jsx"
                )
                && reference
                    .caller
                    .and_then(|ordinal| extraction.nodes.get(ordinal))
                    .is_some_and(|node| node.kind == "file")
                && extraction.nodes.iter().any(|node| {
                    node.kind == "class"
                        && node.start_line as usize <= reference.line
                        && node.end_line as usize >= reference.line
                })
                && !extraction.unresolved_references.iter().any(|raw| {
                    raw.reference_kind == "calls"
                        && raw.line as usize == reference.line
                        && raw.reference_name.rsplit('.').next() == Some(reference.target.as_str())
                })
            {
                return false;
            }
            let name = reference
                .qualifier
                .as_ref()
                .map(|qualifier| format!("{qualifier}::{}", reference.target).replace('.', "::"));
            reference.kind != "calls"
                || !name.is_some_and(|name| field_sites.contains(&(name, reference.line)))
        });
        for raw in &extraction.unresolved_references {
            let type_binding = raw.reference_kind == "references"
                && matches!(
                    file.analysis.language.as_str(),
                    "typescript" | "tsx" | "javascript" | "jsx"
                )
                && ids
                    .get(raw.from_node_id.as_str())
                    .and_then(|&ordinal| file.nodes.get(ordinal))
                    .is_some_and(|&from| {
                        if self
                            .cached_framework_reference(
                                from,
                                &raw.reference_name,
                                &raw.reference_kind,
                                raw.line as usize,
                                Some(raw.column as usize),
                            )
                            .is_some()
                        {
                            return true;
                        }
                        let (targets, method) = self.lookup(
                            path,
                            &self.nodes[from].symbol.qualified_name,
                            &raw.reference_name.replace('.', "::"),
                        );
                        method != "name_match"
                            && targets.iter().any(|&target| {
                                matches!(
                                    self.nodes[target].symbol.kind.as_str(),
                                    "class" | "struct" | "interface" | "type_alias" | "enum"
                                )
                            })
                    });
            let cpp_binding = matches!(file.analysis.language.as_str(), "c" | "cpp")
                && ids
                    .get(raw.from_node_id.as_str())
                    .and_then(|&ordinal| file.nodes.get(ordinal))
                    .is_some_and(|&from| {
                        self.cached_framework_reference(
                            from,
                            &raw.reference_name,
                            &raw.reference_kind,
                            raw.line as usize,
                            Some(raw.column as usize),
                        )
                        .is_some()
                    });
            if !type_binding
                && !cpp_binding
                && !field_claim(raw)
                && !matches!(raw.reference_kind.as_str(), "instantiates" | "function_ref")
            {
                continue;
            }
            let caller = ids.get(raw.from_node_id.as_str()).copied();
            let normalized = raw.reference_name.replace("->", "::").replace('.', "::");
            let (qualifier, target) = normalized
                .rsplit_once("::")
                .map_or((None, normalized.as_str()), |(owner, name)| {
                    (Some(owner.to_owned()), name)
                });
            let shadowed = raw.reference_kind == "instantiates"
                && references.iter().any(|reference| {
                    reference.caller == caller
                        && reference.line == raw.line as usize
                        && reference.shadowed
                        && (reference.target == target
                            || reference.target == "new"
                                && reference.qualifier.as_deref() == Some(target))
                });
            if raw.reference_kind == "instantiates" {
                references.retain(|reference| {
                    !(!workspace::esm(&file.analysis.language)
                        && reference.caller == caller
                        && reference.line == raw.line as usize
                        && reference.kind == "calls"
                        && (reference.target == target
                            || reference.target == "new"
                                && reference.qualifier.as_deref() == Some(target)))
                });
            }
            if references.iter().any(|reference| {
                reference.caller == caller
                    && reference.target == target
                    && (reference.qualifier == qualifier || raw.reference_kind == "instantiates")
                    && reference.line == raw.line as usize
                    && reference.kind == raw.reference_kind
                    && reference
                        .column
                        .is_none_or(|column| column == raw.column as usize)
            }) {
                continue;
            }
            references.push(Reference {
                column: Some(raw.column as usize),
                caller,
                target: target.into(),
                line: raw.line as usize,
                qualifier,
                kind: raw.reference_kind.clone(),
                receiver_type: None,
                shadowed,
                owner: None,
            });
        }
        let mut positions = HashMap::<_, Vec<usize>>::new();
        for raw in &extraction.unresolved_references {
            let name = raw.reference_name.replace("->", "::").replace('.', "::");
            positions
                .entry((
                    ids.get(raw.from_node_id.as_str()).copied(),
                    name,
                    raw.reference_kind.as_str(),
                    raw.line as usize,
                ))
                .or_default()
                .push(raw.column as usize);
        }
        let mut consumed = HashMap::new();
        for reference in &mut references {
            if reference.column.is_some() {
                continue;
            }
            let name = reference
                .qualifier
                .as_ref()
                .map_or_else(
                    || reference.target.clone(),
                    |qualifier| format!("{qualifier}::{}", reference.target),
                )
                .replace("->", "::")
                .replace('.', "::");
            let mut key = (
                reference.caller,
                name,
                reference.kind.as_str(),
                reference.line,
            );
            if !positions.contains_key(&key)
                && reference.qualifier.as_deref() == Some("this")
                && workspace::esm(&file.analysis.language)
            {
                key.1 = reference.target.clone();
            }
            let cursor = consumed.entry(key.clone()).or_insert(0usize);
            reference.column = positions
                .get(&key)
                .and_then(|columns| columns.get(*cursor).copied());
            *cursor += 1;
        }
        self.append_react_references(path, &mut references);
        if matches!(file.analysis.language.as_str(), "c" | "cpp") {
            let mut seen = HashSet::new();
            references.retain(|reference| {
                seen.insert((
                    reference.caller,
                    reference.target.clone(),
                    reference.qualifier.clone(),
                    reference.kind.clone(),
                    reference.line,
                    reference.column,
                ))
            });
        }
        references
    }

    pub(super) fn build_import_edges(&mut self) {
        let paths: Vec<_> = self.files.keys().cloned().collect();
        for path in paths {
            let file = &self.files[&path];
            let Some(extraction) = &file.analysis.extraction else {
                continue;
            };
            let ids: HashMap<_, _> = extraction
                .nodes
                .iter()
                .zip(&file.nodes)
                .map(|(node, &i)| (node.id.as_str(), i))
                .collect();
            let refs: Vec<_> = extraction
                .unresolved_references
                .iter()
                .filter(|r| r.reference_kind == "imports")
                .filter_map(|r| Some((*ids.get(r.from_node_id.as_str())?, r.clone())))
                .collect();
            let mut seen = HashSet::new();
            for (from, raw) in refs {
                let name = &raw.reference_name;
                let language = self.files[&path].analysis.language.as_str();
                let mut found = Vec::new();
                let mut method = "import_match";
                let reference = Reference {
                    column: Some(raw.column as usize),
                    caller: None,
                    target: name.clone(),
                    line: raw.line as usize,
                    qualifier: None,
                    kind: "imports".into(),
                    receiver_type: None,
                    shadowed: false,
                    owner: None,
                };
                if matches!(language, "c" | "cpp") {
                    for target in self.module_files(&path, name) {
                        found.extend(
                            self.files[&target]
                                .nodes
                                .iter()
                                .copied()
                                .filter(|&i| self.nodes[i].symbol.kind == "file"),
                        );
                    }
                    if found.is_empty()
                        && let Some(target) = self.cpp_imports.fallback(name, &path)
                    {
                        found.extend(
                            self.files[&target]
                                .nodes
                                .iter()
                                .copied()
                                .filter(|&i| self.nodes[i].symbol.kind == "file"),
                        );
                        method = "file-path";
                    }
                }
                if let Some((targets, binding_method)) = self
                    .infrastructure_reference(&path, &reference)
                    .or_else(|| self.framework_reference(&path, from, &reference))
                {
                    found = targets;
                    method = binding_method;
                }
                let authoritative = method == "framework_match"
                    || self
                        .cached_framework_reference(
                            from,
                            name,
                            "imports",
                            raw.line as usize,
                            Some(raw.column as usize),
                        )
                        .is_some();
                if authoritative && found.is_empty() {
                    continue;
                }
                for import in &self.files[&path].analysis.imports {
                    if !authoritative
                        && (import.local == *name || import.reexport && import.imported == *name)
                    {
                        found.extend(self.imported(&path, import, name));
                    }
                }
                if found.is_empty()
                    && matches!(
                        language,
                        "typescript"
                            | "tsx"
                            | "javascript"
                            | "jsx"
                            | "arkts"
                            | "svelte"
                            | "vue"
                            | "astro"
                            | "python"
                    )
                {
                    for target in self.module_files(&path, name) {
                        found.extend(
                            self.files[&target]
                                .nodes
                                .iter()
                                .copied()
                                .filter(|&i| self.nodes[i].symbol.kind == "file"),
                        );
                    }
                }
                if found.is_empty() && language == "rust" && name.contains("::") {
                    let (module, imported) = name.rsplit_once("::").unwrap();
                    for target in self.module_files(&path, module) {
                        found.extend(self.exported(&target, imported, &mut HashSet::new()));
                    }
                }
                if found.is_empty()
                    && matches!(
                        language,
                        "java" | "kotlin" | "scala" | "csharp" | "php" | "vbnet"
                    )
                {
                    let qualified = name.replace(['.', '\\'], "::");
                    for &index in self.names.get(&qualified).into_iter().flatten() {
                        let node = &self.nodes[index];
                        if node.symbol.kind == "import" || node.symbol.kind == "file" {
                            continue;
                        }
                        let Some(metadata) = self.metadata(index) else {
                            continue;
                        };
                        let symbol = metadata
                            .qualified_name
                            .strip_prefix(&format!("{}::", metadata.file_path))
                            .unwrap_or(&metadata.qualified_name);
                        if symbol == qualified {
                            found.push(index);
                        }
                    }
                    if found.len() > 1 {
                        method = "name_match";
                    }
                }
                found.sort_unstable();
                found.dedup();
                if found.len() > super::MAX_CANDIDATES {
                    self.resolution_truncated.set(true);
                }
                let ambiguous = found.len() > 1;
                for to in found.into_iter().take(super::MAX_CANDIDATES) {
                    if !seen.insert((from, to, raw.line, raw.column)) {
                        continue;
                    }
                    self.push_edge(Edge {
                        column: Some(raw.column),
                        from,
                        to: Some(to),
                        target: name.clone(),
                        line: raw.line as usize,
                        kind: "imports".into(),
                        method,
                        confidence: if ambiguous
                            || method == "file-path" && !self.nodes[to].path.ends_with(name)
                        {
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

#[cfg(test)]
mod tests {
    use super::super::Graph;
    use crate::{Direction, Engine};
    use serde_json::Value;
    use std::collections::{BTreeSet, HashMap};

    #[test]
    fn persisted_multifile_graphs_match_independent_reference() {
        let corpus: Value = serde_json::from_str(include_str!(
            "../tests/fixtures/upstream/resolution/cases.json"
        ))
        .unwrap();
        let mut differences = Vec::new();
        for case in corpus["cases"].as_array().unwrap() {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().join("project");
            std::fs::create_dir(&root).unwrap();
            for (path, source) in case["files"].as_object().unwrap() {
                let path = root.join(path);
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(path, source.as_str().unwrap()).unwrap();
            }
            let mut engine = Engine::open(&root, Some(&temp.path().join("cache.sqlite"))).unwrap();
            engine.sync().unwrap();
            let graph = Graph::load(&engine.connection, 1).unwrap();
            for file in case["extraction"].as_array().unwrap() {
                let path = file["filePath"].as_str().unwrap();
                let actual = &graph.files[path]
                    .analysis
                    .extraction
                    .as_ref()
                    .unwrap()
                    .nodes;
                let mut actual: Vec<_> = actual
                    .iter()
                    .map(|node| {
                        let mut value = serde_json::to_value(node).unwrap();
                        value.as_object_mut().unwrap().remove("updatedAt");
                        value.sort_all_objects();
                        serde_json::to_string(&value).unwrap()
                    })
                    .collect();
                let mut expected: Vec<_> = file["result"]["nodes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|node| {
                        let mut value = node.clone();
                        value.sort_all_objects();
                        serde_json::to_string(&value).unwrap()
                    })
                    .collect();
                actual.sort();
                expected.sort();
                assert_eq!(
                    actual, expected,
                    "{} {path}: canonical node records",
                    case["name"]
                );
            }
            let canonical: HashMap<_, _> = graph
                .files
                .values()
                .flat_map(|file| {
                    file.analysis
                        .extraction
                        .iter()
                        .flat_map(|extraction| extraction.nodes.iter().zip(&file.nodes))
                })
                .map(|(node, &index)| (index, node.id.as_str()))
                .collect();
            let actual: BTreeSet<_> = graph
                .edges
                .iter()
                .filter_map(|edge| {
                    Some((
                        canonical.get(&edge.from)?.to_string(),
                        canonical.get(&edge.to?)?.to_string(),
                        edge.kind.clone(),
                    ))
                })
                .collect();
            let expected: BTreeSet<_> = case["edges"]
                .as_array()
                .unwrap()
                .iter()
                .map(|edge| {
                    (
                        edge["source"].as_str().unwrap().to_owned(),
                        edge["target"].as_str().unwrap().to_owned(),
                        edge["kind"].as_str().unwrap().to_owned(),
                    )
                })
                .collect();
            let uncertain: BTreeSet<_> = graph
                .edges
                .iter()
                .filter(|edge| edge.ambiguous && edge.confidence == "low")
                .filter_map(|edge| {
                    let from = graph.metadata(edge.from)?;
                    let to = graph.metadata(edge.to?)?;
                    case["uncertainty"]
                        .as_array()?
                        .iter()
                        .any(|rule| {
                            rule["sourceFile"] == from.file_path
                                && rule["targetQualifiedName"] == to.qualified_name
                                && rule["kind"] == edge.kind
                        })
                        .then(|| (from.id.clone(), to.id.clone(), edge.kind.clone()))
                })
                .collect();
            let labels: HashMap<_, _> = case["nodes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|node| {
                    (
                        node["id"].as_str().unwrap(),
                        format!(
                            "{}:{}",
                            node["filePath"].as_str().unwrap(),
                            node["name"].as_str().unwrap()
                        ),
                    )
                })
                .collect();
            for (name, records) in [
                ("missing", expected.difference(&actual)),
                ("extra", actual.difference(&expected)),
            ] {
                for (from, to, kind) in records {
                    if name == "extra"
                        && uncertain.contains(&(from.clone(), to.clone(), kind.clone()))
                    {
                        continue;
                    }
                    differences.push(format!(
                        "{} {name} {kind} {} -> {}",
                        case["name"],
                        labels.get(from.as_str()).unwrap_or(from),
                        labels.get(to.as_str()).unwrap_or(to)
                    ));
                }
            }
            let by_id: HashMap<_, _> = canonical.iter().map(|(&index, &id)| (id, index)).collect();
            for query in case["queries"].as_array().unwrap() {
                let id = query["nodeId"].as_str().unwrap();
                let index = by_id[id];
                for (direction, field) in [
                    (Direction::Callers, "callers"),
                    (Direction::Callees, "callees"),
                ] {
                    let neighbors: BTreeSet<_> = graph
                        .edges
                        .iter()
                        .filter(|edge| super::call_edge(&edge.kind))
                        .filter_map(|edge| {
                            let other = match direction {
                                Direction::Callers if edge.to == Some(index) => edge.from,
                                Direction::Callees if edge.from == index => edge.to?,
                                _ => return None,
                            };
                            (other != index)
                                .then(|| canonical.get(&other).copied())
                                .flatten()
                        })
                        .collect();
                    let expected: BTreeSet<_> = query[field]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|node| node.as_str().unwrap())
                        .collect();
                    let extra: BTreeSet<_> = neighbors.difference(&expected).copied().collect();
                    let permitted: BTreeSet<_> = uncertain
                        .iter()
                        .filter_map(|(from, to, kind)| {
                            if !super::call_edge(kind) {
                                return None;
                            }
                            match direction {
                                Direction::Callers if to == id => Some(from.as_str()),
                                Direction::Callees if from == id => Some(to.as_str()),
                                _ => None,
                            }
                        })
                        .collect();
                    assert!(
                        expected.is_subset(&neighbors) && extra.is_subset(&permitted),
                        "{} {id} {field}: expected {expected:?}; actual {neighbors:?}",
                        case["name"]
                    );
                    // Public queries must consume the same resolved graph. Qualified
                    // names shared by multiple namespace blocks remain ambiguous.
                    let name = &graph.nodes[index].symbol.qualified_name;
                    if graph.nodes[index].symbol.kind != "file"
                        && graph.names.get(name).is_some_and(|nodes| nodes.len() == 1)
                    {
                        let result = engine.related(name, direction, 50, 65536).unwrap();
                        let actual: BTreeSet<_> = result
                            .relations
                            .iter()
                            .filter_map(|relation| {
                                relation.candidate.as_ref()?;
                                let sql_id = match direction {
                                    Direction::Callers => relation.caller.id,
                                    Direction::Callees => relation.candidate.as_ref()?.id,
                                };
                                canonical.get(graph.ids.get(&sql_id)?).copied()
                            })
                            .collect();
                        assert_eq!(actual, neighbors, "{} {name} public {field}", case["name"]);
                    }
                }
            }
            drop(engine);
            for path in case["files"].as_object().unwrap().keys() {
                std::fs::write(root.join(path), "live source changed after indexing\n").unwrap();
            }
            let engine = Engine::open(&root, Some(&temp.path().join("cache.sqlite"))).unwrap();
            let reopened = Graph::load(&engine.connection, 1).unwrap();
            assert_eq!(
                reopened.edges.len(),
                graph.edges.len(),
                "{} reopened graph",
                case["name"]
            );
            assert_eq!(
                reopened.canonical_edges, graph.canonical_edges,
                "{} canonical graph survives changed live sources",
                case["name"]
            );
        }
        assert!(differences.is_empty(), "{}", differences.join("\n"));
    }
}
