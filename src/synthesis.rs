//! Derived graph edges, evaluated from one committed source snapshot.
// Ported from the pinned MIT-licensed reference; see THIRD_PARTY_NOTICES.md.

use anyhow::{Result, ensure};
use prometeu_extract::{Edge, Node, UnresolvedReference};
use serde_json::{Value, json};
use std::cell::{Cell, Ref, RefCell};
use std::collections::{BTreeMap, HashMap, HashSet};

mod angular;
mod arkui;
mod c_fnptr;
mod callbacks;
pub(crate) mod comments;
mod const_alias;
mod conventions;
mod dispatch;
mod erlang;
mod go;
mod go_dispatch;
mod go_func_field;
mod tauri;
pub(crate) use go_func_field::PREDECLARED as GO_PREDECLARED;
pub(crate) use go_func_field::imports as go_imports;
pub(crate) use go_func_field::result_types as go_result_types;
pub(crate) use go_func_field::scope::{
    Declaration as GoDeclaration, Scopes as GoScopes, blank as blank_go,
};
mod jobs;
mod jsx;
pub(crate) use jsx::react::references as react_references;
mod navigation;
mod next_proxy;
mod overrides;
mod parameters;
mod registry;
mod rn;
mod stores;
mod structural;
mod tier;
mod vue;

const CALLBACK_CAP: usize = 40;
const JS: &[&str] = &["typescript", "javascript", "tsx", "jsx"];

#[derive(Clone, Debug, serde::Deserialize)]
pub(crate) struct ImportBinding {
    pub file: String,
    pub local_name: String,
    pub resolved_path: Option<String>,
    #[serde(default)]
    pub mapped_path: Option<String>,
    pub external: bool,
    #[serde(default)]
    pub imported_name: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub is_namespace: bool,
    #[serde(default)]
    pub is_default: bool,
}

pub(crate) type ModuleResolver<'a> = dyn Fn(&str, &str, &str) -> Option<String> + 'a;

pub(crate) type GoPackageResolver<'a> = dyn Fn(&str, &str) -> Option<String> + 'a;

pub(crate) type ImportResolver<'a> = dyn Fn(&UnresolvedReference) -> Result<Option<String>> + 'a;

pub(crate) struct SynthesisEnvironment<'a> {
    pub imports: &'a [ImportBinding],
    pub manifests: &'a BTreeMap<&'a str, &'a str>,
    pub auxiliary_sources: &'a BTreeMap<&'a str, &'a str>,
    pub frameworks: &'a [String],
    pub resolve_import: &'a ImportResolver<'a>,
    pub go_package_dir: &'a GoPackageResolver<'a>,
    pub resolve_module: &'a ModuleResolver<'a>,
}

#[cfg(test)]
pub(crate) fn synthesize(
    nodes: &[Node],
    base_edges: &[Edge],
    sources: &BTreeMap<&str, &str>,
    unresolved: &[UnresolvedReference],
) -> Result<Vec<Edge>> {
    synthesize_with_workspace(nodes, base_edges, sources, unresolved, &[])
}

#[cfg(test)]
pub(crate) fn synthesize_with_workspace(
    nodes: &[Node],
    base_edges: &[Edge],
    sources: &BTreeMap<&str, &str>,
    unresolved: &[UnresolvedReference],
    module_dirs: &[String],
) -> Result<Vec<Edge>> {
    synthesize_with_imports(
        nodes,
        base_edges,
        sources,
        unresolved,
        module_dirs,
        &SynthesisEnvironment {
            imports: &[],
            manifests: &BTreeMap::new(),
            auxiliary_sources: &BTreeMap::new(),
            frameworks: &[],
            resolve_import: &|_| Ok(None),
            go_package_dir: &|_, _| None,
            resolve_module: &|_, _, _| None,
        },
    )
}

pub(crate) fn synthesize_with_imports(
    nodes: &[Node],
    base_edges: &[Edge],
    sources: &BTreeMap<&str, &str>,
    unresolved: &[UnresolvedReference],
    module_dirs: &[String],
    environment: &SynthesisEnvironment<'_>,
) -> Result<Vec<Edge>> {
    let initial = Context::new(nodes, base_edges, sources, unresolved);
    let mut prerequisites = structural::go_method_contains(&initial)?;
    drop(initial);
    let mut with_prerequisites = base_edges.to_vec();
    with_prerequisites.extend(prerequisites.iter().cloned());
    let implicit = go::implements(&Context::new(
        nodes,
        &with_prerequisites,
        sources,
        unresolved,
    ))?;
    with_prerequisites.extend(implicit.iter().cloned());
    prerequisites.extend(implicit);
    let mut ctx = Context::new(nodes, &with_prerequisites, sources, unresolved);
    ctx.manifests = environment.manifests;
    ctx.auxiliary_sources = environment.auxiliary_sources;
    ctx.frameworks = environment.frameworks;
    ctx.resolve_import = environment.resolve_import;
    ctx.go_package_dir = environment.go_package_dir;
    ctx.resolve_module = environment.resolve_module;
    ctx.module_dirs = module_dirs.to_vec();
    ctx.module_dirs
        .sort_by_key(|dir| std::cmp::Reverse(dir.encode_utf16().count()));
    let mut module_seen = HashSet::new();
    ctx.module_dirs
        .retain(|dir| module_seen.insert(dir.clone()));
    for binding in environment.imports {
        ctx.imports
            .entry((binding.file.as_str(), binding.local_name.as_str()))
            .or_insert(binding);
        ctx.file_imports
            .entry(binding.file.as_str())
            .or_default()
            .push(binding);
    }

    // Registry order controls first-seen source/target deduplication.
    let passes = [
        callbacks::fields(&ctx)?,
        callbacks::collections(&ctx)?,
        tier::cross_tier(&ctx)?,
        tauri::edges(&ctx)?,
        tier::test_requests(&ctx)?,
        callbacks::emitter(&ctx)?,
        structural::react_render(&ctx)?,
        jsx::edges(&ctx)?,
        vue::templates(&ctx)?,
        conventions::svelte_load(&ctx)?,
        conventions::pascal_forms(&ctx)?,
        structural::flutter_build(&ctx)?,
        structural::arkui_state(&ctx)?,
        arkui::emitter(&ctx)?,
        arkui::router(&ctx)?,
        overrides::cpp(&ctx)?,
        overrides::interfaces(&ctx)?,
        structural::kotlin_expect_actual(&ctx)?,
        structural::go_grpc(&ctx)?,
        rn::events(&ctx)?,
        structural::fabric_native(&ctx)?,
        structural::expo_cross_platform(&ctx)?,
        structural::rn_cross_platform(&ctx)?,
        structural::mybatis(&ctx)?,
        go_dispatch::gin(&ctx)?,
        conventions::redux_thunks(&ctx)?,
        const_alias::edges(&ctx)?,
        registry::objects(&ctx)?,
        stores::rtk_query(&ctx)?,
        stores::pinia(&ctx)?,
        stores::vuex(&ctx)?,
        jobs::celery(&ctx)?,
        dispatch::spring(&ctx)?,
        dispatch::mediatr(&ctx)?,
        jobs::sidekiq(&ctx)?,
        erlang::behaviour(&ctx)?,
        jobs::laravel(&ctx)?,
        c_fnptr::edges(&ctx)?,
        go_dispatch::goframe(&ctx)?,
        go_func_field::edges(&ctx)?,
        navigation::expo_returns(&ctx)?,
        navigation::next_links(&ctx)?,
        next_proxy::edges(&ctx)?,
        navigation::react_links(&ctx)?,
        navigation::tanstack_links(&ctx)?,
        navigation::vue_links(&ctx)?,
        angular::edges(&ctx)?,
        navigation::svelte_pages(&ctx)?,
        navigation::svelte_links(&ctx)?,
        structural::nix_options(&ctx)?,
    ];
    let mut output = prerequisites;
    let mut seen = HashSet::new();
    for edge in passes.into_iter().flatten() {
        if !seen.insert((edge.source.clone(), edge.target.clone())) {
            continue;
        }
        if ctx
            .node(&edge.target)
            .is_some_and(|n| manual_mock(&n.file_path))
            && ctx
                .node(&edge.source)
                .is_some_and(|n| !crate::classify::is_test_path(&n.file_path))
        {
            continue;
        }
        output.push(edge);
    }
    Ok(output)
}

/// Shared object-literal member index for declared field receiver resolution.
pub(crate) struct DirectObjectMembers<'a> {
    context: Context<'a>,
}
impl<'a> DirectObjectMembers<'a> {
    pub(crate) fn new(nodes: &'a [Node], sources: &'a BTreeMap<&'a str, &'a str>) -> Self {
        Self {
            context: Context::new(nodes, &[], sources, &[]),
        }
    }
    pub(crate) fn member(
        &self,
        container: &Node,
        name: &str,
        at: &UnresolvedReference,
    ) -> Result<Option<&'a Node>> {
        const_alias::object::direct_member(&self.context, container, name, at)
    }
}

struct Context<'a> {
    nodes: &'a [Node],
    unresolved: &'a [UnresolvedReference],
    edges: &'a [Edge],
    sources: &'a BTreeMap<&'a str, &'a str>,
    ids: HashMap<&'a str, usize>,
    names: HashMap<&'a str, Vec<usize>>,
    qualified: HashMap<&'a str, Vec<usize>>,
    files: HashMap<&'a str, Vec<usize>>,
    outgoing: HashMap<&'a str, Vec<usize>>,
    incoming: HashMap<&'a str, Vec<usize>>,
    lines: RefCell<HashMap<String, Vec<u32>>>,
    work: Cell<usize>,
    module_dirs: Vec<String>,
    imports: HashMap<(&'a str, &'a str), &'a ImportBinding>,
    file_imports: HashMap<&'a str, Vec<&'a ImportBinding>>,
    manifests: &'a BTreeMap<&'a str, &'a str>,
    auxiliary_sources: &'a BTreeMap<&'a str, &'a str>,
    frameworks: &'a [String],
    resolve_import: &'a ImportResolver<'a>,
    go_package_dir: &'a GoPackageResolver<'a>,
    resolve_module: &'a ModuleResolver<'a>,
}

impl<'a> Context<'a> {
    fn new(
        nodes: &'a [Node],
        edges: &'a [Edge],
        sources: &'a BTreeMap<&'a str, &'a str>,
        unresolved: &'a [UnresolvedReference],
    ) -> Self {
        let mut ctx = Self {
            nodes,
            edges,
            sources,
            unresolved,
            ids: HashMap::new(),
            names: HashMap::new(),
            qualified: HashMap::new(),
            files: HashMap::new(),
            outgoing: HashMap::new(),
            incoming: HashMap::new(),
            lines: RefCell::new(HashMap::new()),
            work: Cell::new(0),
            module_dirs: Vec::new(),
            imports: HashMap::new(),
            file_imports: HashMap::new(),
            manifests: {
                static EMPTY: std::sync::LazyLock<BTreeMap<&str, &str>> =
                    std::sync::LazyLock::new(BTreeMap::new);
                &EMPTY
            },
            auxiliary_sources: {
                static EMPTY: std::sync::LazyLock<BTreeMap<&str, &str>> =
                    std::sync::LazyLock::new(BTreeMap::new);
                &EMPTY
            },
            frameworks: &[],
            resolve_import: &|_| Ok(None),
            go_package_dir: &|_, _| None,
            resolve_module: &|_, _, _| None,
        };
        for (i, node) in nodes.iter().enumerate() {
            ctx.ids.insert(&node.id, i);
            ctx.names.entry(&node.name).or_default().push(i);
            ctx.qualified
                .entry(&node.qualified_name)
                .or_default()
                .push(i);
            ctx.files.entry(&node.file_path).or_default().push(i);
        }
        for (i, edge) in edges.iter().enumerate() {
            ctx.outgoing.entry(&edge.source).or_default().push(i);
            ctx.incoming.entry(&edge.target).or_default().push(i);
        }
        ctx
    }
    fn step(&self) -> Result<()> {
        self.steps(1)
    }
    fn steps(&self, count: usize) -> Result<()> {
        ensure!(
            count <= usize::MAX - self.work.get(),
            "synthesis work counter overflow"
        );
        self.work.set(self.work.get() + count);
        Ok(())
    }
    fn emit(&self, output: &mut Vec<Edge>, edge: Edge) -> Result<()> {
        output.push(edge);
        Ok(())
    }
    fn node(&self, id: &str) -> Option<&'a Node> {
        self.ids.get(id).map(|&i| &self.nodes[i])
    }
    fn in_file(&self, path: &str) -> impl Iterator<Item = &'a Node> + '_ {
        self.files
            .get(path)
            .into_iter()
            .flatten()
            .map(|&i| &self.nodes[i])
    }
    fn line(&self, path: &str, line: u32) -> Option<&'a str> {
        let source = self.sources.get(path)?;
        let lines = self.lines(path)?;
        let start = *lines.get(line.checked_sub(1)? as usize)? as usize;
        let end = lines
            .get(line as usize)
            .map_or(source.len(), |&end| end as usize - 1);
        source.get(start..end)
    }
    fn line_at(&self, path: &str, offset: usize) -> u32 {
        self.lines(path).map_or(1, |lines| {
            lines.partition_point(|&start| start as usize <= offset) as u32
        })
    }
    fn enclosing(&self, path: &str, line: u32) -> Result<Option<&'a Node>> {
        let mut best: Option<&Node> = None;
        for node in self.in_file(path) {
            self.step()?;
            if matches!(node.kind.as_str(), "method" | "function" | "component")
                && node.start_line <= line
                && node.end_line >= line
                && best.is_none_or(|best| node.start_line >= best.start_line)
            {
                best = Some(node);
            }
        }
        Ok(best)
    }
    fn lines(&self, path: &str) -> Option<Ref<'_, Vec<u32>>> {
        if !self.lines.borrow().contains_key(path) {
            let (&path, &source) = self.sources.get_key_value(path)?;
            let mut starts = vec![0];
            starts.extend(
                source
                    .bytes()
                    .enumerate()
                    .filter_map(|(i, b)| (b == b'\n').then_some((i + 1) as u32)),
            );
            self.lines.borrow_mut().insert(path.to_owned(), starts);
        }
        Ref::filter_map(self.lines.borrow(), |lines| lines.get(path)).ok()
    }
    fn named(&self, name: &str) -> impl Iterator<Item = &'a Node> + '_ {
        self.names
            .get(name)
            .into_iter()
            .flatten()
            .map(|&i| &self.nodes[i])
    }
    fn qualified(&self, name: &str) -> impl Iterator<Item = &'a Node> + '_ {
        self.qualified
            .get(name)
            .into_iter()
            .flatten()
            .map(|&i| &self.nodes[i])
    }
    fn outgoing(&self, id: &str) -> impl Iterator<Item = &'a Edge> + '_ {
        self.outgoing
            .get(id)
            .into_iter()
            .flatten()
            .map(|&i| &self.edges[i])
    }
    fn incoming(&self, id: &str) -> impl Iterator<Item = &'a Edge> + '_ {
        self.incoming
            .get(id)
            .into_iter()
            .flatten()
            .map(|&i| &self.edges[i])
    }
    fn children(&self, id: &str) -> Vec<&'a Node> {
        self.outgoing(id)
            .filter(|e| e.kind == "contains")
            .filter_map(|e| self.node(&e.target))
            .collect()
    }
    fn methods(&self, id: &str) -> Vec<&'a Node> {
        self.children(id)
            .into_iter()
            .filter(|n| n.kind == "method")
            .collect()
    }
    fn slice(&self, node: &Node) -> Option<&'a str> {
        let source = self.sources.get(node.file_path.as_str())?;
        let lines = self.lines(node.file_path.as_str())?;
        if node.start_line == 0 || node.end_line == 0 {
            return None;
        }
        let start = *lines.get(node.start_line as usize - 1)? as usize;
        let end = lines
            .get(node.end_line as usize)
            .map_or(source.len(), |&i| i.saturating_sub(1) as usize);
        source.get(start..end)
    }
    fn has(&self, languages: &[&str]) -> bool {
        self.nodes
            .iter()
            .any(|n| languages.contains(&n.language.as_str()))
    }
}

fn call(from: &Node, to: &Node, pass: &str, via: &str) -> Edge {
    Edge {
        source: from.id.clone(),
        target: to.id.clone(),
        kind: "calls".into(),
        line: Some(from.start_line),
        column: None,
        provenance: Some("heuristic".into()),
        metadata: Some(
            json!({"synthesizedBy":pass,"via":via,"registeredAt":format!("{}:{}",to.file_path,to.start_line)}),
        ),
    }
}
fn regex(pattern: &str) -> Result<fancy_regex::Regex> {
    Ok(fancy_regex::Regex::new(
        &crate::parser::javascript_regex::pattern(pattern),
    )?)
}
fn matches(pattern: &str, value: &str) -> Result<bool> {
    Ok(regex(pattern)?.is_match(value)?)
}
fn type_kind(kind: &str) -> bool {
    matches!(
        kind,
        "class" | "interface" | "struct" | "enum" | "type_alias"
    )
}
fn decorated(node: &Node, decorator: &str) -> bool {
    node.decorators
        .as_ref()
        .is_some_and(|ds| ds.iter().any(|d| d == decorator))
}
fn directory(path: &str) -> String {
    path.replace('\\', "/")
        .rsplit_once('/')
        .map_or(String::new(), |(directory, _)| directory.to_owned())
}
fn manual_mock(path: &str) -> bool {
    path.replace('\\', "/")
        .to_lowercase()
        .split('/')
        .any(|p| p == "__mocks__")
}

/// C-style comment masking used by the reference's TypeScript passes.
fn strip_comments(source: &str) -> String {
    strip_cstyle(source, true)
}
fn strip_cstyle(source: &str, allow_single: bool) -> String {
    let bytes = source.as_bytes();
    let mut out = bytes.to_vec();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'/' && matches!(bytes.get(i + 1), Some(b'/' | b'*')) {
            let start = i;
            if bytes[i + 1] == b'/' {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            } else {
                i += 2;
                while i + 1 < bytes.len() && &bytes[i..i + 2] != b"*/" {
                    i += 1;
                }
                i = (i + 2).min(bytes.len());
            }
            for b in &mut out[start..i] {
                if *b != b'\n' {
                    *b = b' ';
                }
            }
        } else if matches!(bytes[i], b'"' | b'`') || allow_single && bytes[i] == b'\'' {
            let quote = bytes[i];
            i += 1;
            while i < bytes.len() && bytes[i] != quote {
                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    i += 2;
                    continue;
                }
                if quote != b'`' && bytes[i] == b'\n' {
                    break;
                }
                i += 1;
            }
            if i < bytes.len() && bytes[i] == quote {
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    String::from_utf8(out).expect("masking retains UTF-8 outside comments")
}

#[cfg(test)]
mod tests;
