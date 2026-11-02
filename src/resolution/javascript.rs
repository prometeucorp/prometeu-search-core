//! Declared JavaScript/TypeScript class-field receivers from the committed snapshot.
//! Ported from the pinned resolver; see THIRD_PARTY_NOTICES.md.
#[path = "javascript/ambient.rs"]
mod ambient;
#[path = "javascript/fields.rs"]
mod fields;
#[path = "javascript/locals.rs"]
mod locals;
#[path = "javascript/method_names.rs"]
mod method_names;
#[path = "javascript/scopes.rs"]
mod scopes;
#[path = "javascript/types.rs"]
mod types;
#[path = "javascript/values.rs"]
mod values;
use crate::synthesis::{DirectObjectMembers, ImportBinding, ImportResolver};
use anyhow::{Result, ensure};
use fields::Field;
use prometeu_extract::{Edge, Node, UnresolvedReference};
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, HashMap, VecDeque},
    rc::Rc,
};
const JS: &[&str] = &["typescript", "tsx", "javascript", "jsx"];
fn family(a: &str, b: &str) -> bool {
    a == b
        || [
            "typescript",
            "tsx",
            "javascript",
            "jsx",
            "arkts",
            "vue",
            "svelte",
            "astro",
        ]
        .contains(&a)
            && [
                "typescript",
                "tsx",
                "javascript",
                "jsx",
                "arkts",
                "vue",
                "svelte",
                "astro",
            ]
            .contains(&b)
}
fn regex(pattern: &str) -> Result<std::sync::Arc<fancy_regex::Regex>> {
    thread_local! { static PATTERNS: RefCell<HashMap<String,std::sync::Arc<fancy_regex::Regex>>> = RefCell::new(HashMap::new()); }
    PATTERNS.with(|patterns| {
        if let Some(cached) = patterns.borrow().get(pattern) {
            return Ok(cached.clone());
        }
        let compiled = std::sync::Arc::new(fancy_regex::Regex::new(
            &crate::parser::javascript_regex::pattern(pattern),
        )?);
        if pattern.len() <= 32_768 {
            let mut patterns = patterns.borrow_mut();
            if patterns.len() >= 256 {
                patterns.clear();
            }
            patterns.insert(pattern.to_string(), compiled.clone());
        }
        Ok(compiled)
    })
}
fn test(pattern: &str, source: &str) -> Result<bool> {
    Ok(regex(pattern)?.is_match(source)?)
}
fn trim(source: &str) -> &str {
    source.trim_matches(crate::search::query_whitespace)
}
fn byte(source: &str, column: u32) -> usize {
    let mut units = 0;
    for (at, ch) in source.char_indices() {
        if units >= column {
            return at;
        }
        units += ch.len_utf16() as u32;
    }
    source.len()
}
fn within(inner: &Node, outer: &Node) -> bool {
    (inner.start_line, inner.start_column) >= (outer.start_line, outer.start_column)
        && (inner.end_line, inner.end_column) <= (outer.end_line, outer.end_column)
}

pub(super) struct Binding {
    pub reference: UnresolvedReference,
    pub target: Option<String>,
    pub confidence: f64,
    pub by_name: bool,
    pub resolved_by: &'static str,
}

pub(super) fn bindings<'a>(
    nodes: &'a [Node],
    sources: &'a BTreeMap<&'a str, &'a str>,
    edges: &[Edge],
    references: &[UnresolvedReference],
    imports: &'a [ImportBinding],
    resolve_import: &'a ImportResolver<'a>,
) -> Result<Vec<Binding>> {
    let relevant: Vec<_> = references
        .iter()
        .filter(|reference| {
            JS.contains(&reference.language.as_deref().unwrap_or(""))
                && matches!(
                    reference.reference_kind.as_str(),
                    "calls" | "function_ref" | "references" | "imports" | "instantiates"
                )
        })
        .collect();
    if relevant.is_empty() {
        return Ok(Vec::new());
    }
    let context = Context::new(nodes, sources, edges, imports, resolve_import);
    let scopes = crate::frameworks::python_resolution::Bindings::new(nodes);
    let mut answers = Vec::new();
    for reference in relevant {
        if let Some(answer) = context.import_or_type(reference)? {
            answers.push(answer);
            continue;
        }
        if reference.reference_kind == "function_ref" {
            let (target, confidence, resolved_by) = if reference.reference_name.starts_with("this.")
            {
                let (target, confidence) = context.this_function_ref(reference)?;
                (target, confidence, "function-ref")
            } else {
                context.function_ref(reference, &scopes)?
            };
            answers.push(Binding {
                reference: reference.clone(),
                target: target.map(|node| node.id.clone()),
                confidence,
                by_name: false,
                resolved_by,
            });
            continue;
        }
        if reference.reference_kind != "calls" {
            continue;
        }
        if !reference.reference_name.starts_with("this.")
            && !reference.reference_name.starts_with("window.")
            && test(
                r"^[A-Za-z_$][\w$]*(?:\.[A-Za-z_$][\w$]*){2,}$",
                &reference.reference_name,
            )?
            && !context
                .imported(
                    reference.file_path.as_deref().unwrap_or(""),
                    reference.reference_name.split('.').next().unwrap_or(""),
                )
                .is_some_and(|binding| binding.is_namespace)
        {
            // Untyped receiver chains do not identify a same-named project function.
            answers.push(Binding {
                reference: reference.clone(),
                target: None,
                confidence: 0.0,
                by_name: false,
                resolved_by: "instance-method",
            });
            continue;
        }
        if let Some(parts) = regex(r"^([\w.]+)\.([\w$]+)$")?.captures(&reference.reference_name)?
            && &parts[1] != "this"
            && &parts[1] != "super"
        {
            let answer =
                if let Some(target) = context.local_call(&parts[1], &parts[2], reference)? {
                    Some((target, 0.9))
                } else {
                    context.typed_name_fallback(&parts[1], &parts[2], reference, &scopes)?
                };
            if let Some((target, confidence)) = answer {
                answers.push(Binding {
                    reference: reference.clone(),
                    target: target.map(|node| node.id.clone()),
                    confidence,
                    by_name: confidence < 0.85,
                    resolved_by: "instance-method",
                });
                continue;
            }
        }
        let Some(name) = reference.reference_name.strip_prefix("this.") else {
            continue;
        };
        let Some((field, method)) = name.rsplit_once('.') else {
            continue;
        };
        let element = field.ends_with("[]");
        let field = field.strip_suffix("[]").unwrap_or(field);
        let target = context
            .field_call(field, method, reference, element)?
            .map(|node| node.id.clone());
        answers.push(Binding {
            reference: reference.clone(),
            target,
            confidence: 0.85,
            by_name: false,
            resolved_by: "instance-method",
        });
    }
    Ok(answers)
}
struct Lines<'a> {
    source: &'a str,
    starts: Rc<Vec<u32>>,
    at: usize,
}
impl<'a> Iterator for Lines<'a> {
    type Item = &'a str;
    fn next(&mut self) -> Option<Self::Item> {
        let start = *self.starts.get(self.at)? as usize;
        let end = self
            .starts
            .get(self.at + 1)
            .map_or(self.source.len(), |offset| *offset as usize - 1);
        self.at += 1;
        Some(&self.source[start..end])
    }
    fn nth(&mut self, n: usize) -> Option<Self::Item> {
        self.at = self.at.saturating_add(n);
        self.next()
    }
}
struct ScanState {
    high: u32,
    answer: Option<locals::Reading>,
}
struct Context<'a> {
    supers: HashMap<&'a str, Vec<&'a Node>>,
    sources: &'a BTreeMap<&'a str, &'a str>,
    ids: HashMap<&'a str, &'a Node>,
    names: HashMap<&'a str, Vec<&'a Node>>,
    files: HashMap<&'a str, Vec<&'a Node>>,
    imports: HashMap<(&'a str, &'a str), &'a ImportBinding>,
    resolve_import: &'a ImportResolver<'a>,
    objects: DirectObjectMembers<'a>,
    fields: RefCell<HashMap<(String, String), Option<Field>>>,
    ambient: RefCell<HashMap<String, Vec<ambient::Block>>>,
    line_offsets: RefCell<VecDeque<(String, Rc<Vec<u32>>)>>,
    local_scans: RefCell<HashMap<(String, String, u32), ScanState>>,
    work: Cell<usize>,
}
impl<'a> Context<'a> {
    fn new(
        nodes: &'a [Node],
        sources: &'a BTreeMap<&'a str, &'a str>,
        edges: &[Edge],
        imports: &'a [ImportBinding],
        resolve_import: &'a ImportResolver<'a>,
    ) -> Self {
        let mut names: HashMap<_, Vec<_>> = HashMap::new();
        let mut files: HashMap<_, Vec<_>> = HashMap::new();
        for node in nodes {
            names.entry(node.name.as_str()).or_default().push(node);
            files.entry(node.file_path.as_str()).or_default().push(node);
        }
        for values in names.values_mut() {
            values.sort_by(|a, b| {
                (&a.file_path, a.start_line, &a.id).cmp(&(&b.file_path, b.start_line, &b.id))
            });
        }
        for values in files.values_mut() {
            values.sort_by(|a, b| (a.start_line, &a.id).cmp(&(b.start_line, &b.id)));
        }
        let ids: HashMap<_, _> = nodes.iter().map(|node| (node.id.as_str(), node)).collect();
        let mut supers: HashMap<_, Vec<_>> = HashMap::new();
        let mut hierarchy: Vec<_> = edges
            .iter()
            .filter(|edge| matches!(edge.kind.as_str(), "extends" | "implements"))
            .collect();
        hierarchy.sort_by(|a, b| {
            (&a.kind, &a.target, a.line, a.column).cmp(&(&b.kind, &b.target, b.line, b.column))
        });
        for edge in hierarchy {
            if let (Some(source), Some(target)) =
                (ids.get(edge.source.as_str()), ids.get(edge.target.as_str()))
            {
                let values = supers.entry(source.id.as_str()).or_default();
                if !values.iter().any(|node: &&Node| node.id == target.id) {
                    values.push(*target);
                }
            }
        }
        Self {
            supers,
            sources,
            ids: nodes.iter().map(|node| (node.id.as_str(), node)).collect(),
            names,
            files,
            imports: {
                let mut by_name = HashMap::new();
                for binding in imports {
                    by_name
                        .entry((binding.file.as_str(), binding.local_name.as_str()))
                        .or_insert(binding);
                }
                by_name
            },
            resolve_import,
            objects: DirectObjectMembers::new(nodes, sources),
            fields: RefCell::new(HashMap::new()),
            ambient: RefCell::new(HashMap::new()),
            line_offsets: RefCell::new(VecDeque::new()),
            local_scans: RefCell::new(HashMap::new()),
            work: Cell::new(0),
        }
    }
    fn step(&self) -> Result<()> {
        self.steps(1)
    }
    fn steps(&self, count: usize) -> Result<()> {
        let work = self.work.get().saturating_add(count);
        ensure!(
            work <= 10_000_000,
            "JavaScript field resolution work limit exceeded"
        );
        self.work.set(work);
        Ok(())
    }
    fn named(&self, name: &str) -> impl Iterator<Item = &'a Node> {
        self.names.get(name).into_iter().flatten().copied()
    }
    fn in_file(&self, path: &str) -> impl Iterator<Item = &'a Node> {
        self.files.get(path).into_iter().flatten().copied()
    }
    fn lines(&self, path: &str) -> Result<Lines<'a>> {
        let source = self.sources.get(path).copied().unwrap_or("");
        let cached = {
            let mut cache = self.line_offsets.borrow_mut();
            cache
                .iter()
                .position(|(file, _)| file == path)
                .map(|index| {
                    let entry = cache.remove(index).unwrap();
                    let starts = entry.1.clone();
                    cache.push_back(entry);
                    starts
                })
        };
        let starts = if let Some(starts) = cached {
            starts
        } else {
            self.steps(source.len() / 64 + 1)?;
            let mut starts = vec![0];
            for (at, ch) in source.bytes().enumerate() {
                if ch == b'\n' {
                    ensure!(
                        starts.len() < 2_000_000,
                        "JavaScript source line index exceeds its bound"
                    );
                    starts.push(at as u32 + 1);
                }
            }
            let starts = Rc::new(starts);
            let mut cache = self.line_offsets.borrow_mut();
            while cache.len() >= 32
                || cache
                    .iter()
                    .map(|(_, offsets)| offsets.len())
                    .sum::<usize>()
                    + starts.len()
                    > 2_000_000
            {
                cache.pop_front();
            }
            cache.push_back((path.to_string(), starts.clone()));
            starts
        };
        Ok(Lines {
            source,
            starts,
            at: 0,
        })
    }
    fn source(&self, node: &Node) -> &'a str {
        self.sources
            .get(node.file_path.as_str())
            .copied()
            .unwrap_or("")
    }
    fn imported(&self, path: &str, name: &str) -> Option<&'a ImportBinding> {
        self.imports.get(&(path, name)).copied()
    }
    fn resolve(
        &self,
        name: &str,
        from: &Node,
        at: &UnresolvedReference,
    ) -> Result<Option<&'a Node>> {
        let mut reference = at.clone();
        reference.file_path = Some(from.file_path.clone());
        reference.language = Some(from.language.clone());
        reference.reference_name = name.to_string();
        reference.reference_kind = "references".to_string();
        Ok((self.resolve_import)(&reference)?.and_then(|id| self.ids.get(id.as_str()).copied()))
    }
    fn field_call(
        &self,
        field: &str,
        method: &str,
        at: &UnresolvedReference,
        element: bool,
    ) -> Result<Option<&'a Node>> {
        if field.is_empty() || field.contains('.') {
            return Ok(None);
        }
        let Some(written) = self.ids.get(at.from_node_id.as_str()).copied() else {
            return Ok(None);
        };
        let caller = self.this_caller(written)?;
        let Some((scope, _)) = caller.qualified_name.rsplit_once("::") else {
            return Ok(None);
        };
        let owner = scope.rsplit("::").next().unwrap_or(scope);
        let path = at.file_path.as_deref().unwrap_or(&caller.file_path);
        let language = at.language.as_deref().unwrap_or(&caller.language);
        let mut owners: Vec<_> = self
            .named(owner)
            .filter(|node| {
                matches!(node.kind.as_str(), "class" | "component")
                    && family(&node.language, language)
            })
            .collect();
        owners.sort_by_key(|node| node.file_path != path);
        for class in owners.iter().copied().filter(|node| node.file_path == path) {
            if let Some((declaration, declaring)) = self.field_through_bases(class, field, at, 0)? {
                let held = self.held(declaration, declaring, element)?;
                return match held {
                    Some(declaration) => {
                        self.method_of_field(&declaration, Some(declaring), method, at)
                    }
                    None => Ok(None),
                };
            }
        }
        for class in owners.into_iter().filter(|node| node.file_path != path) {
            if let Some(declaration) = self.field(class, field, at)?
                && let Some(declaration) = self.held(declaration, class, element)?
                && self.type_visible(&declaration.type_name, at)?
            {
                return self.method_of_field(&declaration, None, method, at);
            }
        }
        Ok(None)
    }
    fn held(&self, field: Field, class: &Node, element: bool) -> Result<Option<Field>> {
        if !element {
            return Ok(Some(field));
        }
        if field.value_type {
            return Ok(None);
        }
        let Some(name) = field.element else {
            return Ok(None);
        };
        if self.type_parameter(class, &name)? {
            return Ok(None);
        }
        Ok(Some(Field {
            value_type: false,
            type_name: name,
            array: false,
            element: None,
        }))
    }
    fn field_through_bases(
        &self,
        class: &'a Node,
        field: &str,
        at: &UnresolvedReference,
        depth: usize,
    ) -> Result<Option<(Field, &'a Node)>> {
        self.step()?;
        if let Some(declaration) = self.field(class, field, at)? {
            return Ok(Some((declaration, class)));
        }
        if depth >= 10 {
            return Ok(None);
        }
        for name in self.declared_supers(class)? {
            if let Some(base) = self.named_class(&name, class, at)?
                && base.id != class.id
                && let Some(found) = self.field_through_bases(base, field, at, depth + 1)?
            {
                return Ok(Some(found));
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
#[path = "javascript/tests.rs"]
mod tests;
