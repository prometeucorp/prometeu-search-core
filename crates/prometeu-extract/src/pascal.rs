//! Pascal declarations, calls, receiver evidence, and Delphi form bindings.
mod calls;
mod forms;
mod preparse;

use crate::supplemental::{children, docstring, Context};
use crate::{Edge, ExtractionResult, UnresolvedReference};
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::LazyLock;
use tree_sitter::Node as SyntaxNode;

macro_rules! re {
    ($pattern:literal) => {{
        static VALUE: LazyLock<Regex> = LazyLock::new(|| Regex::new($pattern).unwrap());
        &*VALUE
    }};
}
pub(super) use re;

fn child<'t>(node: SyntaxNode<'t>, kind: &str) -> Option<SyntaxNode<'t>> {
    children(node).into_iter().find(|n| n.kind() == kind)
}
fn last(node: SyntaxNode<'_>) -> Option<SyntaxNode<'_>> {
    node.named_child(node.named_child_count().checked_sub(1)?)
}
fn no_generics(name: &str) -> String {
    re!(r"<[^>]*>").replace_all(name, "").into_owned()
}
fn visibility(mut node: SyntaxNode<'_>) -> Option<String> {
    while let Some(parent) = node.parent() {
        if parent.kind() == "declSection" {
            for i in 0..parent.child_count() {
                let value = match parent.child(i)?.kind() {
                    "kPublic" | "kPublished" => "public",
                    "kPrivate" => "private",
                    "kProtected" => "protected",
                    _ => continue,
                };
                return Some(value.into());
            }
        }
        node = parent;
    }
    None
}

#[derive(Clone, Default)]
struct Scope {
    locals: Rc<HashMap<String, Option<String>>>,
    class: Option<String>,
    routine: Option<String>,
    line: u32,
}
struct Walker<'s, 't> {
    context: Context<'s>,
    original: &'s str,
    original_bytes: Vec<usize>,
    prepared_units: Vec<usize>,
    scopes: Vec<Scope>,
    with_depth: usize,
    method_index: Option<HashMap<String, usize>>,
    syntax: Vec<(usize, SyntaxNode<'t>)>,
    targets: HashMap<String, (usize, usize)>,
    reads: Vec<(UnresolvedReference, String, String)>,
    assigned: HashMap<String, HashSet<String>>,
    data: HashMap<String, HashSet<String>>,
    function_refs: Vec<(usize, String, SyntaxNode<'t>)>,
    text_routines: Option<Vec<Scope>>,
    broken: bool,
}

pub(crate) fn extract(path: &str, source: &str) -> Result<ExtractionResult, String> {
    if path.to_ascii_lowercase().ends_with(".dfm") || path.to_ascii_lowercase().ends_with(".fmx") {
        return Ok(forms::extract(path, source));
    }
    let prepared = preparse::prepare(source);
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_pascal::LANGUAGE.into())
        .map_err(|e| e.to_string())?;
    let tree = parser
        .parse(&prepared, None)
        .ok_or("Pascal parser returned no tree")?;
    let mut original_bytes = Vec::new();
    for (i, c) in source.char_indices() {
        original_bytes.extend(std::iter::repeat_n(i, c.len_utf16()));
    }
    original_bytes.push(source.len());
    let mut prepared_units = Vec::new();
    let mut units = 0;
    for c in prepared.chars() {
        prepared_units.extend(std::iter::repeat_n(units, c.len_utf8()));
        units += c.len_utf16();
    }
    prepared_units.push(units);
    let mut walker = Walker {
        context: Context::new(path, &prepared, "pascal"),
        original: source,
        original_bytes,
        prepared_units,
        scopes: Vec::new(),
        with_depth: 0,
        method_index: None,
        syntax: Vec::new(),
        targets: HashMap::new(),
        reads: Vec::new(),
        assigned: HashMap::new(),
        data: HashMap::new(),
        function_refs: Vec::new(),
        text_routines: None,
        broken: tree.root_node().has_error(),
    };
    crate::stack::run_guarded(|| {
        walker.visit(tree.root_node());
        Ok(())
    })?;
    for (reference, scope, name) in walker.reads.drain(..) {
        if !walker
            .assigned
            .get(&scope)
            .is_some_and(|set| set.contains(&name))
        {
            walker.context.result.unresolved_references.push(reference);
        }
    }
    walker.flush_functions();
    walker.flush_values(tree.root_node());
    Ok(walker.context.finish(tree.root_node().has_error()))
}

impl<'s, 't> Walker<'s, 't> {
    fn text(&self, node: SyntaxNode<'_>) -> &'s str {
        self.context.text(node)
    }
    fn written(&self, node: SyntaxNode<'_>) -> &str {
        &self.original[self.original_bytes[self.prepared_units[node.start_byte()]]
            ..self.original_bytes[self.prepared_units[node.end_byte()]]]
    }
    fn signature(&self, node: SyntaxNode<'_>) -> Option<String> {
        let args = node.child_by_field_name("args");
        let ty = child(node, "typeref");
        if args.is_none() && ty.is_none() {
            return None;
        }
        let mut text = args.map(|n| self.written(n).to_owned()).unwrap_or_default();
        if let Some(ty) = ty {
            text.push_str(": ");
            text.push_str(self.written(ty));
        }
        (!text.is_empty()).then_some(text)
    }
    fn create(
        &mut self,
        kind: &str,
        name: &str,
        syntax: SyntaxNode<'t>,
        extra: impl FnOnce(&mut crate::Node),
    ) -> Option<usize> {
        let parent = self.context.current();
        let index = self.context.create_node(kind, name, syntax, extra)?;
        if kind == "constant"
            && name.len() >= 3
            && name.bytes().any(|b| b.is_ascii_uppercase() || b == b'_')
            && parent.is_some_and(|p| {
                matches!(
                    self.context.result.nodes[p].kind.as_str(),
                    "file" | "class" | "module" | "struct" | "enum"
                )
            })
        {
            let entry = self.targets.entry(name.into()).or_insert((index, 0));
            entry.0 = index;
            entry.1 += 1;
        }
        if matches!(
            kind,
            "function" | "method" | "constant" | "variable" | "component"
        ) {
            self.syntax.push((index, syntax));
        }
        Some(index)
    }
    fn reference(&mut self, name: &str, kind: &str, node: SyntaxNode<'_>) {
        if let Some(from) = self.context.current() {
            self.context.reference(from, name, kind, node);
        }
    }
    fn visit(&mut self, node: SyntaxNode<'t>) {
        stack_guard!();
        match node.kind() {
            "unit" | "program" | "library" => {
                let name = child(node, "moduleName")
                    .map(|n| self.text(n))
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| {
                        self.context
                            .path
                            .rsplit('/')
                            .next()
                            .unwrap_or(self.context.path)
                            .rsplit_once('.')
                            .map_or(self.context.path, |p| p.0)
                    })
                    .to_owned();
                self.create("module", &name, node, |_| {});
            }
            "declType" => {
                self.decl_type(node);
                return;
            }
            "declUses" => {
                for n in children(node)
                    .into_iter()
                    .filter(|n| n.kind() == "moduleName")
                {
                    let name = self.text(n).to_owned();
                    let signature = self.text(node).trim().to_owned();
                    self.create("import", &name, n, |n| n.signature = Some(signature));
                    self.reference(&name, "imports", n);
                }
                return;
            }
            "declConsts" => {
                for c in children(node)
                    .into_iter()
                    .filter(|c| c.kind() == "declConst")
                {
                    self.constant(c);
                }
                return;
            }
            "declConst" => {
                self.constant(node);
                return;
            }
            "declVars" => {
                for n in children(node).into_iter().filter(|n| n.kind() == "declVar") {
                    if let Some(name) = n.child_by_field_name("name") {
                        self.create("variable", self.text(name), n, |_| {});
                    }
                }
                return;
            }
            "defProc" => {
                self.definition(node);
                return;
            }
            "declProp" | "declField" => {
                if let Some(name) = node.child_by_field_name("name") {
                    self.create(
                        if node.kind() == "declProp" {
                            "property"
                        } else {
                            "field"
                        },
                        self.text(name),
                        node,
                        |n| n.visibility = visibility(node),
                    );
                }
                return;
            }
            "declProc" => {
                self.declaration(node);
                return;
            }
            "exprCall" => {
                self.call(node, false);
                return;
            }
            "block" | "blockTr" | "initialization" | "finalization" => {
                let outside = self.scopes.is_empty();
                if outside {
                    self.scopes.push(Scope {
                        locals: Rc::new(HashMap::from([("result".into(), None)])),
                        ..Scope::default()
                    });
                }
                self.block(node, node.kind() == "blockTr");
                if outside {
                    self.scopes.pop();
                }
                return;
            }
            _ => {}
        }
        for n in children(node) {
            self.visit(n);
        }
    }
    fn decl_type(&mut self, node: SyntaxNode<'t>) {
        let Some(name) = node.child_by_field_name("name") else {
            return;
        };
        let name = self.text(name).to_owned();
        let class = child(node, "declClass");
        let interface = child(node, "declIntf");
        if let Some(inner) = class.or(interface) {
            let Some(index) = self.create(
                if class.is_some() {
                    "class"
                } else {
                    "interface"
                },
                &name,
                node,
                |_| {},
            ) else {
                return;
            };
            if class.is_some() {
                for (i, n) in children(inner)
                    .into_iter()
                    .filter(|n| n.kind() == "typeref")
                    .enumerate()
                {
                    self.context.reference(
                        index,
                        self.text(n),
                        if i == 0 { "extends" } else { "implements" },
                        n,
                    );
                }
            }
            self.context.push_scope(index);
            for n in children(inner) {
                self.visit(n);
            }
            self.context.pop_scope();
        } else if let Some(enumeration) = child(node, "type").and_then(|n| child(n, "declEnum")) {
            let Some(index) = self.create("enum", &name, node, |_| {}) else {
                return;
            };
            self.context.push_scope(index);
            for n in children(enumeration)
                .into_iter()
                .filter(|n| n.kind() == "declEnumValue")
            {
                if let Some(name) = n.child_by_field_name("name") {
                    self.create("enum_member", self.text(name), n, |_| {});
                }
            }
            self.context.pop_scope();
        } else {
            self.create("type_alias", &name, node, |_| {});
        }
    }
    fn constant(&mut self, node: SyntaxNode<'t>) {
        if let Some(name) = node.child_by_field_name("name") {
            let signature = child(node, "defaultValue").map(|n| self.text(n).to_owned());
            self.create("constant", self.text(name), node, |n| {
                n.signature = signature
            });
        }
    }
    fn declaration(&mut self, node: SyntaxNode<'t>) {
        let Some(name) = node.child_by_field_name("name") else {
            return;
        };
        let method = self.context.scopes.iter().any(|&i| {
            matches!(
                self.context.result.nodes[i].kind.as_str(),
                "class" | "interface" | "struct" | "trait" | "enum"
            )
        });
        let signature = self.signature(node);
        let doc = docstring(node, self.context.source);
        let return_type = child(node, "typeref")
            .map(|ty| self.text(child(ty, "identifier").unwrap_or(ty)).trim())
            .filter(|s| re!(r"^[A-Za-z_][A-Za-z0-9_]*$").is_match(s))
            .map(str::to_owned);
        self.create(
            if method { "method" } else { "function" },
            self.text(name),
            node,
            |n| {
                n.signature = signature;
                n.docstring = doc;
                n.visibility = visibility(node);
                n.return_type = return_type;
                n.is_static = Some(
                    (0..node.child_count())
                        .any(|i| node.child(i).is_some_and(|c| c.kind() == "kClass")),
                );
                if !method {
                    n.is_exported = Some(false);
                }
            },
        );
    }
    fn definition(&mut self, node: SyntaxNode<'t>) {
        let Some(header) = child(node, "declProc") else {
            return;
        };
        let Some(name) = header.child_by_field_name("name") else {
            return;
        };
        let full = self.text(name).trim().to_owned();
        let short = full.rsplit('.').next().unwrap_or(&full).to_owned();
        if self.method_index.is_none() {
            let mut map = HashMap::new();
            for (i, n) in self
                .context
                .result
                .nodes
                .iter()
                .enumerate()
                .filter(|(_, n)| matches!(n.kind.as_str(), "method" | "function"))
            {
                map.entry(n.name.to_lowercase()).or_insert(i);
                if n.kind == "method" {
                    let parts: Vec<_> = n.qualified_name.split("::").collect();
                    for j in 0..parts.len().saturating_sub(1) {
                        map.insert(parts[j..].join(".").to_lowercase(), i);
                    }
                }
            }
            self.method_index = Some(map);
        }
        let mut index = self
            .method_index
            .as_ref()
            .and_then(|m| {
                m.get(&full.to_lowercase())
                    .or_else(|| m.get(&short.to_lowercase()))
            })
            .copied();
        if index.is_none() && !full.contains('.') {
            let signature = self.signature(header);
            index = self.create("function", &full, header, |n| {
                n.signature = signature;
                n.visibility = visibility(header);
            });
            if let Some(i) = index {
                let map = self.method_index.as_mut().unwrap();
                map.insert(full.to_lowercase(), i);
                map.entry(short.to_lowercase()).or_insert(i);
            }
        }
        let Some(index) = index.or_else(|| self.context.current()) else {
            return;
        };
        let plain = no_generics(&full);
        let owner = plain.rsplit_once('.').map(|(p, _)| p.replace('.', "::"));
        let n = &self.context.result.nodes[index];
        let qualified = if n.kind == "file" {
            plain.replace('.', "::")
        } else {
            n.qualified_name.clone()
        };
        self.routine(
            node,
            index,
            owner,
            short.split('<').next().unwrap().to_lowercase(),
            qualified,
        );
    }
    fn routine(
        &mut self,
        node: SyntaxNode<'t>,
        index: usize,
        owner: Option<String>,
        routine: String,
        qualified: String,
    ) {
        stack_guard!();
        self.scopes.push(Scope {
            locals: Rc::new(self.routine_locals(node, true)),
            class: owner.clone(),
            routine: Some(routine),
            line: node.start_position().row as u32 + 1,
        });
        self.context.push_scope(index);
        let mut block = None;
        let mut misplaced = vec![];
        for c in children(node) {
            match c.kind() {
                "declVars" | "declConsts" => {
                    for decl in children(c) {
                        let values = self.declare_local(decl);
                        Rc::make_mut(&mut self.scopes.last_mut().unwrap().locals).extend(values);
                    }
                }
                "defProc" => {
                    if block.is_some()
                        || node.has_error()
                            && c.start_position().column <= node.start_position().column
                    {
                        misplaced.push(c);
                    } else if let Some(header) = child(c, "declProc") {
                        if let Some(name) = header
                            .child_by_field_name("name")
                            .filter(|n| n.kind() == "identifier")
                        {
                            let name = self.text(name).to_owned();
                            let signature = self.signature(header);
                            let qn = format!("{qualified}::{name}");
                            if let Some(i) = self.create("function", &name, c, |n| {
                                n.signature = signature;
                                n.qualified_name = qn.clone();
                            }) {
                                self.routine(c, i, owner.clone(), name.to_lowercase(), qn);
                            }
                        }
                    }
                }
                "block" if block.is_none() => block = Some(c),
                _ => {}
            }
        }
        if let Some(block) = block {
            self.block(block, false);
        }
        self.context.pop_scope();
        self.scopes.pop();
        for c in misplaced {
            self.definition(c);
        }
    }
    fn type_name(&self, node: Option<SyntaxNode<'_>>) -> Option<String> {
        let mut n = node?;
        if n.kind() == "type" {
            n = n.named_child(0)?;
        }
        if n.kind() != "typeref" {
            return None;
        }
        n = n.named_child(0)?;
        if n.kind() == "typerefDot" {
            n = last(n)?;
        } else if n.kind() == "typerefTpl" {
            n = n.named_child(0)?;
        }
        (n.kind() == "identifier").then(|| self.text(n).to_owned())
    }
    fn local_type_name(&self, n: Option<SyntaxNode<'_>>) -> Option<String> {
        if n.is_some_and(|n| {
            n.kind() == "type" && n.named_child(0).is_some_and(|c| c.kind() == "declProcRef")
        }) {
            Some("procedure".into())
        } else {
            self.type_name(n)
        }
    }
    fn declare_local(&self, decl: SyntaxNode<'_>) -> HashMap<String, Option<String>> {
        let ty = self.local_type_name(
            children(decl)
                .into_iter()
                .find(|c| matches!(c.kind(), "type" | "typeref")),
        );
        children(decl)
            .into_iter()
            .filter(|c| c.kind() == "identifier")
            .map(|c| (self.text(c).to_lowercase(), ty.clone()))
            .collect()
    }
    fn routine_locals(
        &self,
        node: SyntaxNode<'_>,
        header_only: bool,
    ) -> HashMap<String, Option<String>> {
        let header = if node.kind() == "lambda" {
            Some(node)
        } else {
            child(node, "declProc")
        };
        let mut locals = HashMap::from([(
            "result".into(),
            self.type_name(header.and_then(|h| child(h, "typeref"))),
        )]);
        if let Some(args) = header.and_then(|h| child(h, "declArgs")) {
            for a in children(args).into_iter().filter(|a| a.kind() == "declArg") {
                locals.extend(self.declare_local(a));
            }
        }
        if !header_only {
            for c in children(node)
                .into_iter()
                .filter(|c| matches!(c.kind(), "declVars" | "declConsts"))
            {
                for d in children(c) {
                    locals.extend(self.declare_local(d));
                }
            }
        }
        locals
    }
    fn inline(&mut self, node: SyntaxNode<'_>) {
        if self.scopes.is_empty() {
            return;
        }
        let (mut id, mut ty) = (None, None);
        match node.kind() {
            "varAssignDef" | "varDef" => {
                ty = self.local_type_name(
                    children(node)
                        .into_iter()
                        .find(|c| matches!(c.kind(), "type" | "typeref")),
                );
                if ty.is_none() {
                    if let Some(value) = node
                        .parent()
                        .filter(|p| p.kind() == "assignment")
                        .and_then(last)
                        .filter(|v| v.kind() == "exprDot")
                    {
                        if let (Some(cls), Some(ctor)) = (value.named_child(0), last(value)) {
                            if cls.kind() == "identifier"
                                && ctor.kind() == "identifier"
                                && self.text(ctor).eq_ignore_ascii_case("create")
                            {
                                ty = Some(self.text(cls).into());
                            }
                        }
                    }
                }
                id = child(node, "identifier");
            }
            "foreach" => id = node.child_by_field_name("iterator"),
            "for" => id = child(node, "assignment").and_then(|n| n.named_child(0)),
            "exceptionHandler" => {
                id = node.child_by_field_name("variable");
                ty = self.type_name(child(node, "typeref"));
            }
            _ => {}
        }
        if let Some(id) = id.filter(|n| n.kind() == "identifier") {
            let name = self.text(id).to_lowercase();
            let locals = Rc::make_mut(&mut self.scopes.last_mut().unwrap().locals);
            if ty.is_some() || !locals.contains_key(&name) {
                locals.insert(name, ty);
            }
        }
    }
    fn local(&self, name: &str, scope: &Scope) -> Option<Option<String>> {
        self.scopes
            .iter()
            .rev()
            .find_map(|s| s.locals.get(name))
            .or_else(|| scope.locals.get(name))
            .cloned()
    }
    fn data_names(&mut self, class: Option<&str>) -> &HashSet<String> {
        let key = class.unwrap_or("").to_owned();
        self.data.entry(key).or_insert_with(|| {
            let prefix = class.map(|c| format!("{}::", c.to_lowercase()));
            self.context
                .result
                .nodes
                .iter()
                .filter(|n| {
                    if n.kind == "enum_member" {
                        return true;
                    }
                    if !matches!(
                        n.kind.as_str(),
                        "field" | "property" | "constant" | "variable"
                    ) {
                        return false;
                    }
                    let qn = no_generics(&n.qualified_name).to_lowercase();
                    !qn.contains("::")
                        || prefix
                            .as_ref()
                            .is_some_and(|p| qn.starts_with(p) && !qn[p.len()..].contains("::"))
                })
                .map(|n| n.name.to_lowercase())
                .collect()
        })
    }
    fn capture(&mut self, node: SyntaxNode<'t>) {
        let values = match node.kind() {
            "exprArgs" => children(node),
            "assignment" => {
                let Some(rhs) = node.child_by_field_name("rhs") else {
                    return;
                };
                let lhs = node
                    .child_by_field_name("left")
                    .or_else(|| node.child_by_field_name("lhs"))
                    .or_else(|| node.child_by_field_name("target"))
                    .or_else(|| node.named_child(0));
                if lhs.is_some_and(|n| {
                    re!(r"([A-Za-z_$][A-Za-z0-9_$]*)\s*$")
                        .captures(self.text(n))
                        .is_some_and(|m| &m[1] == self.text(rhs).trim())
                }) {
                    return;
                }
                vec![rhs]
            }
            _ => return,
        };
        for mut n in values {
            if n.kind() == "identifier" {
                continue;
            }
            let mut depth = 0;
            while n.kind() == "exprUnary" && depth <= 4 {
                let Some(inner) = n.child_by_field_name("operand") else {
                    break;
                };
                n = inner;
                depth += 1;
            }
            if n.kind() == "identifier" && depth <= 4 {
                let name = self.text(n);
                if ![
                    "this",
                    "self",
                    "super",
                    "null",
                    "nil",
                    "true",
                    "false",
                    "undefined",
                ]
                .contains(&name)
                {
                    if let Some(from) = self.context.current() {
                        self.function_refs.push((from, name.to_owned(), n));
                    }
                }
            }
        }
    }
    fn flush_functions(&mut self) {
        if crate::textutil::is_generated_file(self.context.path) {
            return;
        }
        let names: HashSet<_> = self
            .context
            .result
            .nodes
            .iter()
            .filter(|n| matches!(n.kind.as_str(), "function" | "method"))
            .map(|n| n.name.clone())
            .chain(
                self.context
                    .result
                    .unresolved_references
                    .iter()
                    .filter(|r| r.reference_kind == "imports")
                    .map(|r| {
                        r.reference_name
                            .rsplit(['.', '\\'])
                            .next()
                            .unwrap()
                            .to_owned()
                    }),
            )
            .collect();
        let mut seen = HashSet::new();
        for (from, name, node) in std::mem::take(&mut self.function_refs) {
            if names.contains(&name) && seen.insert((from, name.clone())) {
                self.context.reference(from, &name, "function_ref", node);
            }
        }
    }
    fn flush_values(&mut self, root: SyntaxNode<'t>) {
        if crate::textutil::is_generated_file(self.context.path) {
            return;
        }
        if std::env::var("PROMETEU_VALUE_REFS").ok().as_deref() == Some("0")
            || self.targets.is_empty()
        {
            return;
        }
        let mut counts = HashMap::<String, usize>::new();
        let mut stack = vec![root];
        let mut visited = 0;
        while let Some(n) = stack.pop() {
            if visited >= 20000 {
                break;
            }
            visited += 1;
            if matches!(n.kind(), "declConst" | "declVar") {
                if let Some(name) = n.child_by_field_name("name") {
                    *counts.entry(self.text(name).into()).or_default() += 1;
                }
            } else if n.kind() == "assignment" {
                if let Some(lhs) = n.named_child(0) {
                    if lhs.kind() == "identifier" {
                        *counts.entry(self.text(lhs).into()).or_default() += 1;
                    }
                }
            }
            stack.extend(children(n));
        }
        self.targets
            .retain(|name, (_, count)| counts.get(name).copied().unwrap_or(0) <= *count);
        for &(index, node) in &self.syntax {
            let mut stack = vec![node];
            if node.kind() != "defProc" {
                if let Some(sib) = node.next_named_sibling().filter(|n| n.kind() == "block") {
                    stack.push(sib);
                }
            }
            let mut seen = HashSet::new();
            let mut visited = 0;
            while let Some(n) = stack.pop() {
                if visited >= 20000 {
                    break;
                }
                visited += 1;
                if n.kind() == "identifier" {
                    let name = self.text(n);
                    if let Some(&(target, _)) = self.targets.get(name) {
                        if target != index
                            && name != self.context.result.nodes[index].name
                            && seen.insert(target)
                        {
                            self.context.result.edges.push(Edge {
                                source: self.context.result.nodes[index].id.clone(),
                                target: self.context.result.nodes[target].id.clone(),
                                kind: "references".into(),
                                metadata: Some(serde_json::json!({"valueRef":true})),
                                line: None,
                                column: None,
                                provenance: None,
                            });
                        }
                    }
                }
                stack.extend(children(n));
            }
        }
    }
}
