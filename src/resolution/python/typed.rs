//! Evidence-based Python attributes, annotated locals, and factory results.
use super::*;
use std::collections::{BTreeSet, HashSet};
use std::rc::Rc;
use value::Evidence;

#[path = "typed/classes.rs"]
mod classes;
#[path = "typed/readers.rs"]
mod readers;
#[path = "typed/selection.rs"]
mod selection;
#[path = "typed/tests.rs"]
#[cfg(test)]
mod tests;

#[derive(Clone, Copy, PartialEq)]
enum Type<'a> {
    Unknown,
    Runtime,
    Class(&'a Node),
    Absent,
}
#[derive(Clone)]
pub(super) struct Answer<'a> {
    pub node: &'a Node,
    pub confidence: f64,
    pub resolved_by: &'static str,
    pub metadata: Option<serde_json::Value>,
}
#[derive(Clone)]
pub(super) enum Match<'a> {
    Unhandled,
    Rejected,
    Resolved(Answer<'a>),
}
type Read = BTreeSet<String>;
type Memo<'a> = (Type<'a>, Read);

pub(super) struct Typed<'a, 'env> {
    files: HashMap<&'a str, Vec<&'a Node>>,
    names: HashMap<&'a str, Vec<&'a Node>>,
    known: HashSet<&'a str>,
    lines: HashMap<&'a str, Vec<String>>,
    original: HashMap<&'a str, Vec<&'a str>>,
    mappings: HashMap<&'a str, Vec<imports::Mapping>>,
    module: &'env dyn Fn(&str, &str) -> Option<String>,
    scopes: &'env scope::Scopes<'a>,
    imports: &'env imports::Imports<'a, 'env>,
    outside: RefCell<HashMap<(String, String), bool>>,
    bases: RefCell<HashMap<&'a str, Vec<Option<&'a Node>>>>,
    attributes: RefCell<HashMap<(String, String), Memo<'a>>>,
    factories: RefCell<HashMap<(String, String, bool), Memo<'a>>>,
    locals: RefCell<HashMap<(String, String), Memo<'a>>>,
    statements: RefCell<HashMap<(String, String), Rc<readers::Attribute>>>,
    work: Cell<usize>,
}
impl<'a, 'env> Typed<'a, 'env> {
    pub(super) fn new(
        nodes: &'a [Node],
        sources: &BTreeMap<&'a str, &'a str>,
        module: &'env dyn Fn(&str, &str) -> Option<String>,
        scopes: &'env scope::Scopes<'a>,
        imports: &'env imports::Imports<'a, 'env>,
    ) -> Result<Self> {
        let mut files: HashMap<&str, Vec<&Node>> = HashMap::new();
        let mut names: HashMap<&str, Vec<&Node>> = HashMap::new();
        for node in nodes.iter().filter(|node| node.language == "python") {
            files.entry(&node.file_path).or_default().push(node);
            names.entry(&node.name).or_default().push(node);
        }
        let mut lines = HashMap::new();
        let mut original = HashMap::new();
        let mut mappings = HashMap::new();
        for &path in files.keys() {
            if let Some(&source) = sources.get(path) {
                lines.insert(
                    path,
                    crate::synthesis::comments::strip_for_regex(source, "python")
                        .split('\n')
                        .map(str::to_owned)
                        .collect(),
                );
                original.insert(
                    path,
                    source
                        .split('\n')
                        .map(|line| line.strip_suffix('\r').unwrap_or(line))
                        .collect(),
                );
                mappings.insert(path, imports::mappings(source)?);
            }
        }
        Ok(Self {
            files,
            names,
            known: nodes.iter().map(|node| node.name.as_str()).collect(),
            lines,
            original,
            mappings,
            module,
            scopes,
            imports,
            outside: RefCell::new(HashMap::new()),
            bases: RefCell::new(HashMap::new()),
            attributes: RefCell::new(HashMap::new()),
            factories: RefCell::new(HashMap::new()),
            locals: RefCell::new(HashMap::new()),
            statements: RefCell::new(HashMap::new()),
            work: Cell::new(0),
        })
    }
    pub(super) fn charge(&self, amount: usize) -> Result<()> {
        let next = self.work.get().saturating_add(amount);
        ensure!(
            next <= 8_000_000,
            "Python type evidence work limit exceeded"
        );
        self.work.set(next);
        Ok(())
    }
    fn bound(
        &self,
        lines: &[String],
        definition: usize,
        upto: usize,
        name: &str,
        params: bool,
    ) -> Result<bool> {
        self.charge(upto.saturating_sub(definition).saturating_add(1))?;
        readers::bound(lines, definition, upto, name, params)
    }
    pub(super) fn in_file(&self, path: &str) -> &[&'a Node] {
        self.files.get(path).map(Vec::as_slice).unwrap_or_default()
    }
    pub(super) fn named(&self, name: &str) -> &[&'a Node] {
        self.names.get(name).map(Vec::as_slice).unwrap_or_default()
    }
    fn line_text(&self, path: &str) -> &[String] {
        self.lines.get(path).map(Vec::as_slice).unwrap_or_default()
    }
    pub(super) fn outside_name(&self, name: &str, path: &str) -> Result<bool> {
        let key = (path.to_owned(), name.to_owned());
        if let Some(value) = self.outside.borrow().get(&key) {
            return Ok(*value);
        }
        let mut outside = regex(r"^\w+$")?.is_match(name)?;
        if outside {
            let keys = self.scopes.all_import_keys(name, path)?;
            outside = !keys.is_empty()
                && !self.scopes.global_rebound(name, path)?
                && !self.in_file(path).iter().any(|node| {
                    node.name == name && matches!(node.kind.as_str(), "class" | "function")
                });
            if outside {
                for key in keys {
                    let module = key.split(':').next().unwrap_or_default();
                    if module.starts_with('.') {
                        outside = false;
                        break;
                    }
                    let parts = module.split('.').collect::<Vec<_>>();
                    if (0..parts.len())
                        .any(|at| (self.module)(&parts[at..].join("."), path).is_some())
                    {
                        outside = false;
                        break;
                    }
                }
            }
        }
        self.outside.borrow_mut().insert(key, outside);
        Ok(outside)
    }
    fn evidence(
        &self,
        ev: &Evidence,
        path: &str,
        read: &mut Read,
        depth: usize,
        receiver: Option<&'a Node>,
    ) -> Result<Type<'a>> {
        Ok(match ev {
            Evidence::Runtime(_) => Type::Runtime,
            Evidence::Arms { arms } => agree(&[
                self.evidence(&arms[0], path, read, depth, receiver)?,
                self.evidence(&arms[1], path, read, depth, receiver)?,
            ]),
            Evidence::Type { name, class_level } => match self.class_named(name, path, "class")? {
                Some(node) => Type::Class(node),
                None if *class_level != Some(true)
                    && self.outside_name(name.split('.').next().unwrap_or_default(), path)? =>
                {
                    Type::Runtime
                }
                _ => Type::Unknown,
            },
            Evidence::Factory { factory, awaited } if factory == "cls" => {
                if *awaited {
                    Type::Unknown
                } else {
                    receiver.map_or(Type::Unknown, Type::Class)
                }
            }
            Evidence::Factory { factory, awaited } => {
                self.factory(factory, *awaited, path, read, depth)?
            }
        })
    }
    fn factory(
        &self,
        callee: &str,
        awaited: bool,
        path: &str,
        read: &mut Read,
        depth: usize,
    ) -> Result<Type<'a>> {
        if depth > 0 {
            return self.factory_uncached(callee, awaited, path, read, depth);
        }
        let key = (path.to_owned(), callee.to_owned(), awaited);
        let cached = self.factories.borrow().get(&key).cloned();
        let (answer, own) = if let Some(cached) = cached {
            cached
        } else {
            let mut own = Read::new();
            let answer = self.factory_uncached(callee, awaited, path, &mut own, 0)?;
            self.factories
                .borrow_mut()
                .insert(key, (answer, own.clone()));
            (answer, own)
        };
        read.extend(own);
        Ok(answer)
    }
    fn factory_uncached(
        &self,
        callee: &str,
        awaited: bool,
        path: &str,
        read: &mut Read,
        depth: usize,
    ) -> Result<Type<'a>> {
        self.charge(1)?;
        if depth > 3 {
            return Ok(Type::Unknown);
        }
        let (prefix, name) = callee.rsplit_once('.').unwrap_or(("", callee));
        let receiver =
            if !prefix.is_empty() && capital(prefix.rsplit('.').next().unwrap_or_default()) {
                self.class_named(prefix, path, "class")?
            } else {
                None
            };
        let function =
            if !prefix.is_empty() && capital(prefix.rsplit('.').next().unwrap_or_default()) {
                match receiver {
                    Some(receiver) => self
                        .find_method(receiver, name, read, &mut HashSet::new())?
                        .map(|(node, _)| node),
                    None => None,
                }
            } else {
                self.class_named(callee, path, "function")?
            };
        let Some(function) = function else {
            return Ok(Type::Unknown);
        };
        read.insert(function.file_path.clone());
        let lines = self.line_text(&function.file_path);
        if lines.is_empty() {
            return Ok(Type::Unknown);
        }
        if regex(r"^async\s+def\b")?.is_match(trim(
            lines
                .get(function.start_line as usize - 1)
                .map(String::as_str)
                .unwrap_or_default(),
        ))? != awaited
        {
            return Ok(Type::Unknown);
        }
        if let Some(returns) =
            regex(r"->\s*(.+)$")?.captures(function.signature.as_deref().unwrap_or_default())?
        {
            return Ok(match value::annotation(&returns[1])? {
                Some(Evidence::Runtime(_)) => Type::Runtime,
                Some(Evidence::Type { name, .. })
                    if regex(r"^(?:typing(?:_extensions)?\.)?Self$")?.is_match(&name)? =>
                {
                    receiver.map_or(Type::Unknown, Type::Class)
                }
                Some(Evidence::Type { name, .. }) => self
                    .class_named(&name, &function.file_path, "class")?
                    .map_or(Type::Unknown, Type::Class),
                _ => Type::Unknown,
            });
        }
        let nested = self
            .in_file(&function.file_path)
            .iter()
            .copied()
            .filter(|node| {
                node.id != function.id
                    && node.start_line > function.start_line
                    && node.end_line <= function.end_line
                    && matches!(node.kind.as_str(), "function" | "method" | "class")
            })
            .collect::<Vec<_>>();
        let mut decorated = String::new();
        for at in (0..function.start_line.saturating_sub(1) as usize).rev() {
            let line = lines.get(at).map(String::as_str).unwrap_or_default();
            if !regex(r"^\s*@")?.is_match(line)? {
                break;
            }
            decorated.push_str(line);
        }
        let classmethod = regex(r"@classmethod\b")?.is_match(&decorated)?;
        let mut answers = Vec::new();
        let mut ln = function.start_line as usize + 1;
        while ln <= function.end_line as usize {
            self.charge(1)?;
            if nested
                .iter()
                .any(|node| ln >= node.start_line as usize && ln <= node.end_line as usize)
            {
                ln += 1;
                continue;
            }
            let text = trim(lines.get(ln - 1).map(String::as_str).unwrap_or_default());
            let code = statements::blank_strings(text);
            if regex(r"^yield\b|[=(\s]yield\b")?.is_match(&code)? {
                return Ok(Type::Unknown);
            }
            let Some(ret) = regex(r"(?:^|[:;]\s*)return\b\s*")?.find(&code)? else {
                ln += 1;
                continue;
            };
            let at = ln;
            let start = utf16_offset(text, code[..ret.end()].encode_utf16().count());
            let (expr, end) =
                readers::joined(lines, ln, &text[start..], function.end_line as usize)?;
            ln = end + 1;
            let expr = trim(&expr);
            if expr.is_empty() || expr == "None" {
                continue;
            }
            let Some(ev) = value::value(expr, None)? else {
                return Ok(Type::Unknown);
            };
            for callee in readers::callees(expr)? {
                if callee != "cls"
                    && self.bound(
                        lines,
                        function.start_line as usize,
                        at,
                        callee.split('.').next().unwrap_or_default(),
                        true,
                    )?
                {
                    return Ok(Type::Unknown);
                }
            }
            answers.push(self.evidence(
                &ev,
                &function.file_path,
                read,
                depth + 1,
                if classmethod { receiver } else { None },
            )?);
        }
        Ok(agree(&answers))
    }
    fn attr_class(&self, owner: &'a Node, attr: &str, read: &mut Read) -> Result<Type<'a>> {
        let key = (owner.id.clone(), attr.to_owned());
        let cached = self.attributes.borrow().get(&key).cloned();
        let (answer, own) = if let Some(cached) = cached {
            cached
        } else {
            let mut own = Read::new();
            let answer = self.attr_walk(owner, attr, &mut own, &mut HashSet::new())?;
            self.attributes
                .borrow_mut()
                .insert(key, (answer, own.clone()));
            (answer, own)
        };
        read.extend(own);
        Ok(answer)
    }
    fn attr_walk(
        &self,
        owner: &'a Node,
        attr: &str,
        read: &mut Read,
        seen: &mut HashSet<&'a str>,
    ) -> Result<Type<'a>> {
        self.charge(1)?;
        if seen.len() >= 5 || !seen.insert(&owner.id) {
            return Ok(Type::Absent);
        }
        read.insert(owner.file_path.clone());
        let statements = self.attr_statements(owner, attr)?;
        if statements.mentioned {
            let mut answers = Vec::new();
            for ev in &statements.evidence {
                let answer = self.evidence(ev, &owner.file_path, read, 0, None)?;
                if answer != Type::Unknown || !matches!(ev, Evidence::Factory { .. }) {
                    answers.push(answer)
                }
            }
            return Ok(agree(&answers));
        }
        let mut answers = Vec::new();
        for base in self.class_bases(owner)?.into_iter().flatten() {
            let answer = self.attr_walk(base, attr, read, seen)?;
            if answer != Type::Absent {
                answers.push(answer)
            }
        }
        if !answers.is_empty() {
            return Ok(agree(&answers));
        }
        for base in self.base_names(owner)? {
            if !regex(r"^(?:ABC|Protocol|NamedTuple|TypedDict|Enum|IntEnum|StrEnum|Flag|IntFlag|\w*(?:Error|Exception|Warning))$")?.is_match(base.rsplit('.').next().unwrap_or_default())?&&self.outside_name(base.split('.').next().unwrap_or_default(),&owner.file_path)? {return Ok(Type::Runtime)}
        }
        Ok(Type::Absent)
    }
    pub(super) fn attribute(
        &self,
        receiver: &str,
        method: &str,
        reference: &UnresolvedReference,
    ) -> Result<Match<'a>> {
        let path = reference.file_path.as_deref().unwrap_or_default();
        let segs = receiver.split('.').collect::<Vec<_>>();
        let root = segs[0];
        if (self.outside_name(root, path)?
            || self
                .mappings
                .get(path)
                .into_iter()
                .flatten()
                .any(|mapping| mapping.is_namespace && mapping.local_name == root))
            && !self.scopes.locally_bound(root, reference)?
        {
            return Ok(Match::Rejected);
        }
        if segs.len() != 2 {
            return Ok(Match::Unhandled);
        }
        let owner = if matches!(root, "self" | "cls") {
            self.in_file(path)
                .iter()
                .copied()
                .filter(|node| {
                    node.kind == "class"
                        && node.start_line <= reference.line
                        && node.end_line >= reference.line
                })
                .max_by_key(|node| node.start_line)
        } else if capital(root) {
            self.class_named(root, path, "class")?
        } else {
            match self.infer_local(root, reference)? {
                Some(name) => self.class_named(&name, path, "class")?,
                None => None,
            }
        };
        let Some(owner) = owner else {
            return Ok(Match::Unhandled);
        };
        let mut read = Read::new();
        Ok(match self.attr_class(owner, segs[1], &mut read)? {
            Type::Runtime => Match::Rejected,
            Type::Unknown | Type::Absent => Match::Unhandled,
            Type::Class(class) => {
                let answer = if let Some(answer) = self.method(class, method, &mut read)? {
                    Some(answer)
                } else {
                    self.subclass_method(class, method, reference)?
                };
                if let Some(answer) = answer {
                    Match::Resolved(with_read(answer, read, path))
                } else if self.hierarchy_known(class, &mut HashSet::new())? {
                    Match::Rejected
                } else {
                    Match::Unhandled
                }
            }
        })
    }
    pub(super) fn attribute_dependencies(
        &self,
        receiver: &str,
        reference: &UnresolvedReference,
    ) -> Result<Option<serde_json::Value>> {
        let Some((root, attribute)) = receiver.split_once('.') else {
            return Ok(None);
        };
        if attribute.contains('.') {
            return Ok(None);
        }
        let path = reference.file_path.as_deref().unwrap_or_default();
        let owner = if matches!(root, "self" | "cls") {
            self.in_file(path)
                .iter()
                .copied()
                .filter(|node| {
                    node.kind == "class"
                        && node.start_line <= reference.line
                        && node.end_line >= reference.line
                })
                .max_by_key(|node| node.start_line)
        } else if capital(root) {
            self.class_named(root, path, "class")?
        } else {
            match self.infer_local(root, reference)? {
                Some(name) => self.class_named(&name, path, "class")?,
                None => None,
            }
        };
        let Some(owner) = owner else {
            return Ok(None);
        };
        let mut read = Read::new();
        if let Type::Class(class) = self.attr_class(owner, attribute, &mut read)? {
            self.method(
                class,
                reference
                    .reference_name
                    .rsplit('.')
                    .next()
                    .unwrap_or_default(),
                &mut read,
            )?;
        }
        read.remove(path);
        Ok((!read.is_empty()).then(|| serde_json::json!({"typeFrom":read})))
    }
    pub(super) fn local(&self, reference: &UnresolvedReference) -> Result<Option<Answer<'a>>> {
        if reference.language.as_deref() != Some("python") || reference.reference_kind != "calls" {
            return Ok(None);
        }
        let Some(capture) =
            regex(r"^([A-Za-z_]\w*)\.(\w+)$")?.captures(&reference.reference_name)?
        else {
            return Ok(None);
        };
        let receiver = &capture[1];
        if matches!(receiver, "self" | "cls") {
            return Ok(None);
        }
        let Some((scope, bindings)) = self.scopes.binding_scope(receiver, reference)? else {
            return Ok(None);
        };
        let mut read = Read::new();
        let key = (scope.id.clone(), receiver.to_owned());
        let cached = self.locals.borrow().get(&key).cloned();
        let (class, own) = if let Some(cached) = cached {
            cached
        } else {
            let mut own = Read::new();
            let class = self.local_class(scope, receiver, reference, &bindings, &mut own)?;
            self.locals.borrow_mut().insert(key, (class, own.clone()));
            (class, own)
        };
        read.extend(own);
        let Type::Class(class) = class else {
            return Ok(None);
        };
        Ok(self.method(class, &capture[2], &mut read)?.map(|answer| {
            with_read(
                answer,
                read,
                reference.file_path.as_deref().unwrap_or_default(),
            )
        }))
    }
    fn local_class(
        &self,
        scope: &'a Node,
        name: &str,
        reference: &UnresolvedReference,
        bindings: &scope::LocalBindings,
        read: &mut Read,
    ) -> Result<Type<'a>> {
        let mut evidence = Vec::new();
        if let Some(param) = &bindings.param {
            let Some(ev) = param
                .as_deref()
                .map(value::annotation)
                .transpose()?
                .flatten()
            else {
                return Ok(Type::Unknown);
            };
            evidence.push(ev);
        }
        for binding in &bindings.bindings {
            let scope::LineBinding::Assign {
                ty,
                value: assigned,
                line,
            } = binding
            else {
                return Ok(Type::Unknown);
            };
            let ev = if let Some(ty) = ty {
                if !assigned.is_empty()
                    && assigned != "None"
                    && let Some(constructor) = readers::constructor(assigned)?
                    && constructor.rsplit('.').next() != ty.rsplit('.').next()
                {
                    return Ok(Type::Unknown);
                }
                value::annotation(ty)?
            } else if assigned == "None" {
                continue;
            } else if regex(r"^[A-Za-z_]\w*$")?.is_match(assigned)? {
                let copied = self.scopes.scope_bindings(scope, assigned)?;
                let Some(Some(param)) = &copied.param else {
                    return Ok(Type::Unknown);
                };
                if param.is_empty() || !copied.bindings.is_empty() {
                    return Ok(Type::Unknown);
                }
                value::annotation(param)?
            } else {
                let mut at = reference.clone();
                at.line = (*line + 1) as u32;
                for callee in readers::callees(assigned)? {
                    if self
                        .scopes
                        .locally_bound(callee.split('.').next().unwrap_or_default(), &at)?
                    {
                        return Ok(Type::Unknown);
                    }
                }
                value::value(assigned, None)?
            };
            let Some(ev) = ev else {
                return Ok(Type::Unknown);
            };
            evidence.push(ev);
        }
        if evidence.iter().all(|ev| matches!(ev, Evidence::Runtime(_))) {
            return Ok(if evidence.is_empty() {
                Type::Unknown
            } else {
                Type::Runtime
            });
        }
        if self.in_file(&scope.file_path).iter().any(|node| {
            node.id != scope.id
                && node.name == name
                && node.start_line > scope.start_line
                && node.end_line <= scope.end_line
                && matches!(node.kind.as_str(), "function" | "method" | "class")
        }) {
            return Ok(Type::Unknown);
        }
        let nonlocal = regex(&format!(
            r"^\s*nonlocal\b.*(?<![\w.]){}\b",
            regex::escape(name)
        ))?;
        let tuple = regex(&format!(
            r"\bas\s*\([^)]*(?<![\w.]){}\b",
            regex::escape(name)
        ))?;
        for line in self
            .line_text(&scope.file_path)
            .iter()
            .take(scope.end_line as usize)
            .skip(scope.start_line as usize - 1)
        {
            self.charge(1)?;
            if line.contains(name) && (nonlocal.is_match(line)? || tuple.is_match(line)?) {
                return Ok(Type::Unknown);
            }
        }
        let mut answers = Vec::new();
        for ev in evidence {
            answers.push(self.evidence(&ev, &scope.file_path, read, 0, None)?)
        }
        Ok(agree(&answers))
    }
}
fn capital(name: &str) -> bool {
    name.as_bytes().first().is_some_and(u8::is_ascii_uppercase)
}
fn agree<'a>(answers: &[Type<'a>]) -> Type<'a> {
    answers
        .first()
        .copied()
        .filter(|first| *first != Type::Unknown && answers.iter().all(|answer| answer == first))
        .unwrap_or(Type::Unknown)
}
fn with_read<'a>(mut answer: Answer<'a>, mut read: Read, path: &str) -> Answer<'a> {
    read.remove(path);
    if !read.is_empty() {
        answer.metadata = Some(serde_json::json!({"typeFrom":read}));
    }
    answer
}
fn utf16_offset(text: &str, units: usize) -> usize {
    let mut seen = 0;
    for (at, ch) in text.char_indices() {
        if seen >= units {
            return at;
        }
        seen += ch.len_utf16();
    }
    text.len()
}
