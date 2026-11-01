//! C and C++ declared types, aliases, and receiver ownership.
use anyhow::{Result, ensure};
#[path = "cpp/bindings.rs"]
mod bindings;
#[path = "cpp/macros.rs"]
mod macros;
#[path = "cpp/namespaces.rs"]
mod namespaces;
#[path = "cpp/receivers.rs"]
mod receivers;
pub(super) use bindings::bindings;
use prometeu_extract::{Node, UnresolvedReference};
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

fn regex(pattern: &str) -> Result<Arc<fancy_regex::Regex>> {
    ensure!(
        pattern.len() <= 65_536,
        "C/C++ lookup pattern exceeds graph budget"
    );
    thread_local! {
        static PATTERNS: RefCell<HashMap<String, Arc<fancy_regex::Regex>>> = RefCell::new(HashMap::new());
    }
    PATTERNS.with(|patterns| {
        if let Some(found) = patterns.borrow().get(pattern) {
            return Ok(found.clone());
        }
        let compiled = Arc::new(crate::frameworks::source_pattern(pattern)?);
        let mut patterns = patterns.borrow_mut();
        if patterns.len() >= 1024 {
            patterns.clear();
        }
        patterns.insert(pattern.to_string(), compiled.clone());
        Ok(compiled)
    })
}

const DECLARING: &[&str] = &[
    "class",
    "struct",
    "union",
    "enum",
    "interface",
    "namespace",
    "module",
];
const CLASSES: &[&str] = &["class", "struct", "union"];
fn family(node: &Node) -> bool {
    matches!(node.language.as_str(), "c" | "cpp")
}
fn parent(name: &str) -> &str {
    name.rsplit_once("::").map_or("", |(parent, _)| parent)
}
fn scopes(name: &str) -> Vec<String> {
    let mut result = vec![name.to_string()];
    let mut name = name;
    while !name.is_empty() {
        name = parent(name);
        result.push(name.to_string());
    }
    result
}
fn trim(text: &str) -> &str {
    text.trim_matches(crate::search::query_whitespace)
}
fn byte(text: &str, column: u32) -> usize {
    let mut units = 0;
    for (at, c) in text.char_indices() {
        if units >= column {
            return at;
        }
        units += c.len_utf16() as u32;
    }
    text.len()
}
fn strip_arguments(text: &str) -> String {
    let mut depth = 0usize;
    text.chars()
        .filter(|&c| {
            if c == '<' {
                depth += 1;
                false
            } else if c == '>' {
                depth = depth.saturating_sub(1);
                false
            } else {
                depth == 0
            }
        })
        .collect()
}
fn split(text: &str) -> Vec<&str> {
    let mut result = Vec::new();
    let mut depth = 0usize;
    let mut from = 0;
    for (at, c) in text.char_indices() {
        if "<([{".contains(c) {
            depth += 1;
        } else if ">)]}".contains(c) {
            depth = depth.saturating_sub(1);
        } else if c == ',' && depth == 0 {
            result.push(&text[from..at]);
            from = at + 1;
        }
    }
    result.push(&text[from..]);
    result
}
#[derive(Clone)]
struct TypeName {
    names: Vec<String>,
    templated: Vec<bool>,
    global: bool,
}
fn read_type(raw: &str) -> Result<Option<TypeName>> {
    let text = regex(r"\b(?:const|volatile|mutable|typename|template|class|struct|union|enum)\b")?
        .replace_all(raw, " ");
    let text = regex(r"[&*]+")?.replace_all(&text, " ");
    let mut names = Vec::new();
    let mut templated = Vec::new();
    let mut part = String::new();
    let mut depth = 0usize;
    let mut parens = 0usize;
    let mut args = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if depth > 0 && matches!(c, '(' | ')') {
            if c == '(' {
                parens += 1;
            } else {
                parens = parens.saturating_sub(1);
            }
        } else if parens > 0 {
            continue;
        } else if c == '<' {
            depth += 1;
            args = true;
        } else if c == '>' {
            depth = depth.saturating_sub(1);
        } else if depth > 0 {
            continue;
        } else if c == ':' && chars.peek() == Some(&':') {
            chars.next();
            names.push(trim(&part).to_string());
            templated.push(args);
            part.clear();
            args = false;
        } else {
            part.push(c);
        }
    }
    names.push(trim(&part).to_string());
    templated.push(args);
    let global = names.first().is_some_and(String::is_empty);
    if global {
        names.remove(0);
        templated.remove(0);
    }
    if names.is_empty() {
        return Ok(None);
    }
    for name in &names {
        if !regex(r"^[A-Za-z_]\w*$")?.is_match(name)? {
            return Ok(None);
        }
    }
    Ok(Some(TypeName {
        names,
        templated,
        global,
    }))
}
fn aliased(text: &str, name: &str) -> Result<Option<String>> {
    let name = ::regex::escape(name);
    if let Some(found) = regex(&format!(
        r"\busing\s+{name}\s*(?:\[\[[^\]]*\]\]\s*)?=\s*([^;]+)"
    ))?
    .captures(text)?
    {
        return Ok(Some(trim(&found[1]).to_string()));
    }
    let Some(found) = regex(r"\btypedef\b([^;]+)")?.captures(text)? else {
        return Ok(None);
    };
    let parts = split(&found[1]);
    Ok(regex(&format!(r"^([\s\S]*?)\b{name}\s*$"))?
        .captures(parts[0])?
        .map(|found| trim(&found[1]).to_string())
        .filter(|name| !name.is_empty()))
}

#[derive(Clone)]
enum Declared<'a> {
    Class(String),
    Enums(Vec<&'a Node>),
    Alias(&'a Node),
    Ambiguous(Vec<&'a Node>),
    Other,
}
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Aliased {
    target: Vec<String>,
    rest: Vec<String>,
    scope: String,
    pointer: bool,
    class_template: bool,
}
pub(super) struct Types<'a> {
    nodes: &'a [Node],
    namespaces: namespaces::Namespaces,
    names: HashMap<&'a str, Vec<&'a Node>>,
    qualified: HashMap<&'a str, Vec<&'a Node>>,
    ids: HashMap<&'a str, &'a Node>,
    lines: HashMap<&'a str, Vec<&'a str>>,
    bases: RefCell<HashMap<String, Vec<String>>>,
    parameters: RefCell<HashMap<String, HashSet<String>>>,
    work: Cell<usize>,
    visible: RefCell<Option<HashSet<String>>>,
}
impl<'a> Types<'a> {
    pub(super) fn new(nodes: &'a [Node], sources: &'a BTreeMap<&'a str, &'a str>) -> Result<Self> {
        let mut names: HashMap<_, Vec<_>> = HashMap::new();
        let mut qualified: HashMap<_, Vec<_>> = HashMap::new();
        for node in nodes {
            names.entry(node.name.as_str()).or_default().push(node);
            qualified
                .entry(node.qualified_name.as_str())
                .or_default()
                .push(node);
        }
        let mut count = 0;
        let mut lines = HashMap::new();
        for (&file, &source) in sources {
            let mut rows = Vec::new();
            for row in source.split('\n') {
                count += 1;
                ensure!(count <= 2_000_000, "C++ source lines exceed graph budget");
                rows.push(row.trim_end_matches('\r'));
            }
            lines.insert(file, rows);
        }
        Ok(Self {
            nodes,
            namespaces: namespaces::Namespaces::read(sources)?,
            names,
            qualified,
            ids: nodes.iter().map(|node| (node.id.as_str(), node)).collect(),
            lines,
            bases: RefCell::new(HashMap::new()),
            parameters: RefCell::new(HashMap::new()),
            work: Cell::new(0),
            visible: RefCell::new(None),
        })
    }
    fn step(&self, work: usize) -> Result<()> {
        let used = self.work.get().saturating_add(work);
        ensure!(
            used <= 10_000_000,
            "C++ type lookup work exceeds graph budget"
        );
        self.work.set(used);
        Ok(())
    }
    fn named(&self, name: &str) -> impl Iterator<Item = &'a Node> {
        self.names.get(name).into_iter().flatten().copied()
    }
    fn qualified(&self, name: &str) -> impl Iterator<Item = &'a Node> {
        self.qualified.get(name).into_iter().flatten().copied()
    }
    fn text(&self, node: &Node) -> String {
        let rows: Vec<_> = self
            .lines
            .get(node.file_path.as_str())
            .into_iter()
            .flatten()
            .skip(node.start_line.saturating_sub(1) as usize)
            .take(node.end_line.saturating_sub(node.start_line) as usize + 1)
            .copied()
            .collect();
        if rows.is_empty() {
            return String::new();
        }
        let mut cut = rows
            .iter()
            .map(|line| (*line).to_string())
            .collect::<Vec<_>>();
        let last = cut.len() - 1;
        let end = byte(&cut[last], node.end_column);
        cut[last].truncate(end);
        if cut[0].len() >= byte(rows[0], node.start_column) {
            cut[0].drain(..byte(rows[0], node.start_column));
        }
        let cut = cut.join("\n");
        if cut.contains("using") || cut.contains("typedef") {
            cut
        } else {
            rows.join("\n")
        }
    }
    fn before(&self, node: &Node, preceding: usize) -> String {
        let Some(rows) = self.lines.get(node.file_path.as_str()) else {
            return String::new();
        };
        let at = node.start_line.saturating_sub(1) as usize;
        let mut parts = rows
            .get(at.saturating_sub(preceding)..at.min(rows.len()))
            .unwrap_or(&[])
            .to_vec();
        if let Some(line) = rows.get(at) {
            parts.push(&line[..byte(line, node.start_column)]);
        }
        parts.join("\n")
    }
    fn alias_template(&self, node: &Node) -> Result<bool> {
        Ok(regex(r"\btemplate\s*<[^;{}]*>\s*$")?.is_match(&self.before(node, 3))?)
    }
    fn template_header(&self, node: &Node, names: &mut HashSet<String>) -> Result<()> {
        let before = self.before(node, 8);
        let Some(at) = before.rfind("template") else {
            return Ok(());
        };
        if let Some(header) = regex(r"^template\s*<([\s\S]*)>(?:\s*(?:\[\[[^\]]*\]\]|(?:static|inline|constexpr|consteval|virtual|explicit|friend|extern)\b|[A-Z][A-Z0-9]*_[A-Z0-9_]*\b(?:\s*\([^()]*\))?))*\s*$")?.captures(&before[at..])? {
            for item in split(&header[1]) {
                let stripped = strip_arguments(item); let parameter = stripped.split('=').next().unwrap_or("");
                if let Some(found) = regex(r"([A-Za-z_]\w*)\s*$")?.captures(parameter)? && !matches!(&found[1], "typename" | "class") { names.insert(found[1].to_string()); }
            }
        }
        Ok(())
    }
    fn parameters(&self, node: &Node) -> Result<HashSet<String>> {
        if let Some(names) = self.parameters.borrow().get(&node.id) {
            return Ok(names.clone());
        }
        let mut names = HashSet::new();
        self.template_header(node, &mut names)?;
        let mut scope = parent(&node.qualified_name);
        while !scope.is_empty() {
            for class in self.qualified(scope).filter(|class| {
                family(class)
                    && CLASSES.contains(&class.kind.as_str())
                    && class.file_path == node.file_path
                    && class.start_line <= node.start_line
                    && class.end_line >= node.start_line
            }) {
                self.template_header(class, &mut names)?;
            }
            scope = parent(scope);
        }
        self.parameters
            .borrow_mut()
            .insert(node.id.clone(), names.clone());
        Ok(names)
    }
    fn pick_alias(
        &self,
        aliases: &[&'a Node],
        at: &UnresolvedReference,
    ) -> Result<Option<&'a Node>> {
        if aliases.len() == 1 {
            return Ok(Some(aliases[0]));
        }
        let file = at.file_path.as_deref().unwrap_or("");
        let own: Vec<_> = aliases
            .iter()
            .copied()
            .filter(|node| node.file_path == file)
            .collect();
        if own.len() == 1 {
            return Ok(Some(own[0]));
        }
        let stem = |path: &str| -> String {
            regex(r"\.[^./\\]+$")
                .unwrap()
                .replace(path, "")
                .into_owned()
        };
        let header: Vec<_> = aliases
            .iter()
            .copied()
            .filter(|node| stem(&node.file_path) == stem(file))
            .collect();
        if header.len() == 1 {
            return Ok(Some(header[0]));
        }
        let mut names = HashSet::new();
        for alias in aliases {
            let written = aliased(&self.text(alias), &alias.name)?;
            let name = match written {
                Some(written) => {
                    Some(read_type(&written)?.map_or(written, |name| name.names.join("::")))
                }
                None => None,
            };
            names.insert(name);
        }
        Ok((names.len() == 1 && !names.contains(&None)).then_some(aliases[0]))
    }
    fn declared(
        &self,
        scope: &str,
        name: &str,
        templated: bool,
        at: &UnresolvedReference,
    ) -> Result<Option<Declared<'a>>> {
        let qualified = if scope.is_empty() {
            name.to_string()
        } else {
            format!("{scope}::{name}")
        };
        let mut here = Vec::new();
        for node in self.named(name).filter(|node| {
            family(node)
                && (node.kind == "type_alias" || DECLARING.contains(&node.kind.as_str()))
                && node.qualified_name == qualified
                && self
                    .visible
                    .borrow()
                    .as_ref()
                    .is_none_or(|visible| visible.contains(&node.id))
        }) {
            self.step(1)?;
            if !templated && node.kind == "type_alias" && self.alias_template(node)? {
                continue;
            }
            here.push(node);
        }
        if here.is_empty() {
            return Ok(None);
        }
        if here
            .iter()
            .any(|node| CLASSES.contains(&node.kind.as_str()))
        {
            return Ok(Some(Declared::Class(qualified)));
        }
        let enums: Vec<_> = here
            .iter()
            .copied()
            .filter(|node| node.kind == "enum")
            .collect();
        if !enums.is_empty() {
            return Ok(Some(Declared::Enums(enums)));
        }
        if here.iter().any(|node| node.kind != "type_alias") {
            return Ok(Some(Declared::Other));
        }
        Ok(Some(match self.pick_alias(&here, at)? {
            Some(node) => Declared::Alias(node),
            None => Declared::Ambiguous(here),
        }))
    }
}

impl<'a> Types<'a> {
    fn class_named(&self, names: &[String], in_scopes: &[String]) -> Option<String> {
        let spelled = names.join("::");
        in_scopes.iter().find_map(|scope| {
            let qualified = if scope.is_empty() {
                spelled.clone()
            } else {
                format!("{scope}::{spelled}")
            };
            let exists = self
                .qualified(&qualified)
                .any(|node| family(node) && CLASSES.contains(&node.kind.as_str()));
            exists.then_some(qualified)
        })
    }
    fn base_classes(&self, class: &str) -> Result<Vec<String>> {
        if let Some(bases) = self.bases.borrow().get(class) {
            return Ok(bases.clone());
        }
        self.bases
            .borrow_mut()
            .insert(class.to_string(), Vec::new());
        let mut bases = Vec::new();
        for node in self
            .qualified(class)
            .filter(|node| family(node) && CLASSES.contains(&node.kind.as_str()))
        {
            let mut head = self
                .lines
                .get(node.file_path.as_str())
                .into_iter()
                .flatten()
                .skip(node.start_line.saturating_sub(1) as usize)
                .take(12)
                .copied()
                .collect::<Vec<_>>();
            if head.is_empty() {
                continue;
            }
            head[0] = &head[0][byte(head[0], node.start_column)..];
            let text = head.join("\n");
            let Some(brace) = text.find('{') else {
                continue;
            };
            let head = &text[..brace];
            if head.contains(';') {
                continue;
            }
            let Some(colon) = regex(r"(?<!:):(?!:)")?.find(head)? else {
                continue;
            };
            for part in split(&head[colon.end()..]) {
                let cleaned =
                    regex(r"\b(?:public|protected|private|virtual)\b")?.replace_all(part, " ");
                let Some(name) = read_type(&cleaned)? else {
                    continue;
                };
                let in_scopes = if name.global {
                    vec![String::new()]
                } else {
                    scopes(parent(class))
                };
                if let Some(base) = self.class_named(&name.names, &in_scopes)
                    && base != class
                    && !bases.contains(&base)
                {
                    bases.push(base);
                }
            }
        }
        self.bases
            .borrow_mut()
            .insert(class.to_string(), bases.clone());
        Ok(bases)
    }
    fn member(
        &self,
        class: &str,
        name: &str,
        templated: bool,
        at: &UnresolvedReference,
        depth: usize,
        seen: &mut HashSet<String>,
    ) -> Result<Option<Declared<'a>>> {
        self.step(1)?;
        let own = self.declared(class, name, templated, at)?;
        if own.is_some() || depth >= 4 {
            return Ok(own);
        }
        seen.insert(class.to_string());
        for base in self.base_classes(class)? {
            if seen.contains(&base) {
                continue;
            }
            if let Some(found) = self.member(&base, name, templated, at, depth + 1, seen)? {
                return Ok(Some(found));
            }
        }
        Ok(None)
    }
    fn unqualified(
        &self,
        name: &str,
        templated: bool,
        in_scopes: &[String],
        at: &UnresolvedReference,
    ) -> Result<Option<Declared<'a>>> {
        for scope in in_scopes {
            let found = if scope.is_empty() {
                self.declared(scope, name, templated, at)?
            } else {
                self.member(scope, name, templated, at, 0, &mut HashSet::new())?
            };
            if found.is_some() {
                return Ok(found);
            }
        }
        Ok(None)
    }
    fn lookup(
        &self,
        name: &TypeName,
        in_scopes: &[String],
        at: &UnresolvedReference,
    ) -> Result<(Option<Declared<'a>>, usize)> {
        let mut found = self.unqualified(&name.names[0], name.templated[0], in_scopes, at)?;
        let mut next = 1;
        if found.is_none() && name.names.len() > 1 {
            for size in (2..=name.names.len()).rev() {
                if found.is_some() {
                    break;
                }
                let prefix = name.names[..size - 1].join("::");
                for scope in in_scopes {
                    let from = if scope.is_empty() {
                        prefix.clone()
                    } else {
                        format!("{scope}::{prefix}")
                    };
                    found =
                        self.declared(&from, &name.names[size - 1], name.templated[size - 1], at)?;
                    if found.is_some() {
                        break;
                    }
                }
                next = size;
            }
        }
        while let Some(Declared::Class(class)) = &found {
            if next >= name.names.len() {
                break;
            }
            found = self.member(
                class,
                &name.names[next],
                name.templated[next],
                at,
                0,
                &mut HashSet::new(),
            )?;
            next += 1;
        }
        Ok((found, next))
    }
    fn caller_scope(&self, at: &UnresolvedReference) -> &str {
        let Some(node) = self.ids.get(at.from_node_id.as_str()) else {
            return "";
        };
        if node.kind == "file" {
            ""
        } else if DECLARING.contains(&node.kind.as_str()) {
            &node.qualified_name
        } else {
            parent(&node.qualified_name)
        }
    }
    fn local_alias(&self, name: &str, at: &UnresolvedReference) -> Result<Option<Option<String>>> {
        let Some(node) = self.ids.get(at.from_node_id.as_str()) else {
            return Ok(None);
        };
        if !matches!(node.kind.as_str(), "function" | "method")
            || Some(node.file_path.as_str()) != at.file_path.as_deref()
        {
            return Ok(None);
        }
        let Some(lines) = self.lines.get(node.file_path.as_str()) else {
            return Ok(None);
        };
        let escaped = ::regex::escape(name);
        let declares = regex(&format!(
            r"\b(?:using\s+{escaped}\s*=|typedef\b[^;]*\b{escaped}\s*[;,])"
        ))?;
        for line in lines
            .iter()
            .take(at.line.min(node.end_line) as usize)
            .skip(node.start_line.saturating_sub(1) as usize)
            .rev()
        {
            self.step(1)?;
            if line.contains(name) && declares.is_match(line)? {
                return Ok(Some(aliased(line, name)?));
            }
        }
        Ok(None)
    }
    fn follow(
        &self,
        name: &TypeName,
        scope: &str,
        at: &UnresolvedReference,
        hops: usize,
    ) -> Result<Option<Option<Aliased>>> {
        let in_scopes = if name.global {
            vec![String::new()]
        } else {
            scopes(scope)
        };
        let (found, next) = self.lookup(name, &in_scopes, at)?;
        let Some(Declared::Alias(alias)) = found else {
            return Ok(None);
        };
        if hops >= 4 {
            return Ok(Some(None));
        }
        Ok(Some(self.expand(
            aliased(&self.text(alias), &alias.name)?.as_deref(),
            &name.names[next..],
            &name.templated[next..],
            parent(&alias.qualified_name),
            at,
            hops + 1,
            &self.parameters(alias)?,
        )?))
    }
    #[allow(clippy::too_many_arguments)]
    fn expand(
        &self,
        written: Option<&str>,
        rest: &[String],
        rest_templated: &[bool],
        scope: &str,
        at: &UnresolvedReference,
        hops: usize,
        parameters: &HashSet<String>,
    ) -> Result<Option<Aliased>> {
        let Some(written) = written else {
            return Ok(None);
        };
        if regex(r"\btypename\b")?.is_match(written)? {
            return Ok(None);
        }
        let Some(target) = read_type(written)? else {
            return Ok(None);
        };
        if parameters.contains(&target.names[0]) {
            return Ok(None);
        }
        let pointer = strip_arguments(written).contains('*');
        let joined = TypeName {
            names: target.names.iter().chain(rest).cloned().collect(),
            templated: target
                .templated
                .iter()
                .chain(rest_templated)
                .copied()
                .collect(),
            global: target.global,
        };
        if let Some(further) = self.follow(&joined, scope, at, hops)? {
            return Ok(further.map(|mut further| {
                further.pointer |= pointer;
                further
            }));
        }
        let scope = if target.global { "" } else { scope };
        let mut class_template = false;
        if let Some(class) = self.class_named(&joined.names, &scopes(scope)) {
            for node in self
                .qualified(&class)
                .filter(|node| family(node) && CLASSES.contains(&node.kind.as_str()))
            {
                class_template |= !self.parameters(node)?.is_empty();
            }
        }
        Ok(Some(Aliased {
            target: target.names,
            rest: rest.to_vec(),
            scope: scope.to_string(),
            pointer,
            class_template,
        }))
    }
    fn alias(&self, raw: &str, at: &UnresolvedReference) -> Result<Option<Option<Aliased>>> {
        let Some(name) = read_type(raw)? else {
            return Ok(None);
        };
        let scope = self.caller_scope(at);
        let local = if name.global {
            None
        } else {
            self.local_alias(&name.names[0], at)?
        };
        let mut resolved = if let Some(local) = local {
            let parameters = self
                .ids
                .get(at.from_node_id.as_str())
                .map(|node| self.parameters(node))
                .transpose()?
                .unwrap_or_default();
            Some(self.expand(
                local.as_deref(),
                &name.names[1..],
                &name.templated[1..],
                scope,
                at,
                1,
                &parameters,
            )?)
        } else {
            self.follow(&name, scope, at, 0)?
        };
        if let Some(Some(resolved)) = &mut resolved {
            resolved.pointer |= strip_arguments(raw).contains('*');
        }
        Ok(resolved)
    }
    fn reached(
        &self,
        names: &[String],
        scope: &str,
        at: &UnresolvedReference,
    ) -> Result<Option<String>> {
        let in_scopes = scopes(scope);
        if let Some(class) = self.class_named(names, &in_scopes) {
            return Ok(Some(class));
        }
        let name = TypeName {
            names: names.to_vec(),
            templated: vec![false; names.len()],
            global: false,
        };
        Ok(match self.lookup(&name, &in_scopes, at)?.0 {
            Some(Declared::Class(class)) => Some(class),
            _ => None,
        })
    }
    fn class_found(
        &self,
        name: &TypeName,
        found: Option<Declared<'a>>,
        next: usize,
        at: &UnresolvedReference,
    ) -> Result<Option<Option<Declared<'a>>>> {
        let Some(found) = found else {
            return Ok(None);
        };
        let alias = match found {
            Declared::Class(class) => return Ok(Some(Some(Declared::Class(class)))),
            Declared::Enums(_) | Declared::Other => return Ok(Some(None)),
            Declared::Ambiguous(mut aliases) => {
                aliases.sort_by(|a, b| {
                    a.file_path
                        .encode_utf16()
                        .cmp(b.file_path.encode_utf16())
                        .then(a.start_line.cmp(&b.start_line))
                });
                return Ok(Some(
                    (next == name.names.len()).then_some(Declared::Alias(aliases[0])),
                ));
            }
            Declared::Alias(alias) => alias,
        };
        let resolved = self.expand(
            aliased(&self.text(alias), &alias.name)?.as_deref(),
            &name.names[next..],
            &name.templated[next..],
            parent(&alias.qualified_name),
            at,
            1,
            &self.parameters(alias)?,
        )?;
        if let Some(resolved) = resolved
            && !resolved.pointer
        {
            let names: Vec<_> = resolved
                .target
                .iter()
                .chain(&resolved.rest)
                .cloned()
                .collect();
            if let Some(class) = self.reached(&names, &resolved.scope, at)? {
                return Ok(Some(Some(Declared::Class(class))));
            }
        }
        Ok(Some(
            (next == name.names.len()).then_some(Declared::Alias(alias)),
        ))
    }
    fn class(
        &self,
        written: &str,
        in_scopes: &[String],
        at: &UnresolvedReference,
        type_reference: bool,
    ) -> Result<Option<Option<Declared<'a>>>> {
        let Some(mut name) = read_type(written)? else {
            return Ok(Some(None));
        };
        if type_reference {
            name.templated.fill(true);
        }
        let (found, next) = self.lookup(&name, in_scopes, at)?;
        if type_reference
            && (matches!(found, Some(Declared::Enums(_)))
                || matches!(found, Some(Declared::Alias(_))) && next == name.names.len())
        {
            return Ok(Some(found));
        }
        self.class_found(&name, found, next, at)
    }
    #[cfg(test)]
    fn alias_written(
        &self,
        written: &str,
        in_scopes: &[String],
        at: &UnresolvedReference,
    ) -> Result<Option<Option<(String, &'a Node)>>> {
        let Some(name) = read_type(written)? else {
            return Ok(None);
        };
        let in_scopes = if name.global {
            &[String::new()]
        } else {
            in_scopes
        };
        let (found, next) = self.lookup(&name, in_scopes, at)?;
        if matches!(found, Some(Declared::Ambiguous(_))) {
            return Ok(Some(None));
        }
        let Some(Declared::Alias(alias)) = found else {
            return Ok(None);
        };
        if next != name.names.len() || self.alias_template(alias)? {
            return Ok(Some(None));
        }
        let Some(text) = aliased(&self.text(alias), &alias.name)? else {
            return Ok(Some(None));
        };
        if regex(r"\b(?:typename|decltype)\b")?.is_match(&text)? {
            return Ok(Some(None));
        }
        Ok(Some(Some((text, alias))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    fn normalized(mut value: Value) -> Value {
        match &mut value {
            Value::Object(object) if object.contains_key("id") && object.contains_key("kind") => {
                json!({"id":object["id"]})
            }
            Value::Object(object) => {
                for item in object.values_mut() {
                    *item = normalized(item.take());
                }
                value
            }
            Value::Array(array) => {
                for item in array {
                    *item = normalized(item.take());
                }
                value
            }
            _ => value,
        }
    }
    fn output(found: Option<Option<Declared<'_>>>) -> Value {
        match found {
            None => json!({"unclaimed":true}),
            Some(None) => Value::Null,
            Some(Some(Declared::Class(class))) => json!({"cls":class}),
            Some(Some(Declared::Alias(alias))) => json!({"alias":{"id":alias.id}}),
            Some(Some(Declared::Enums(enums))) => {
                json!({"enums":enums.iter().map(|node| json!({"id":node.id})).collect::<Vec<_>>()})
            }
            Some(Some(_)) => panic!("unexpected declaration result"),
        }
    }
    #[test]
    fn cpp_alias_helpers_match_independent_reference() -> Result<()> {
        let corpus: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/upstream/cpp-resolution/cases.json"
        ))?;
        let mut total = 0;
        for case in corpus["cases"].as_array().unwrap() {
            let nodes: Vec<Node> = case["nodes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|node| {
                    let mut node = node.clone();
                    node["updatedAt"] = json!(0);
                    serde_json::from_value(node)
                })
                .collect::<std::result::Result<_, _>>()?;
            let sources: BTreeMap<_, _> = case["sources"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(file, text)| (file.as_str(), text.as_str().unwrap()))
                .collect();
            let types = Types::new(&nodes, &sources)?;
            for answer in case["answers"].as_array().unwrap() {
                let reference = answer
                    .get("reference")
                    .map(|value| serde_json::from_value::<UnresolvedReference>(value.clone()))
                    .transpose()?;
                let scopes: Vec<String> = answer
                    .get("scopes")
                    .map(|value| serde_json::from_value(value.clone()))
                    .transpose()?
                    .unwrap_or_default();
                *types.visible.borrow_mut() = answer
                    .get("visibleIds")
                    .map(|value| serde_json::from_value(value.clone()))
                    .transpose()?;
                let written = answer["written"].as_str().unwrap_or("");
                let actual = match answer["name"].as_str().unwrap() {
                    "resolveCppAliasedType" => {
                        match types.alias(written, reference.as_ref().unwrap())? {
                            None => json!({"unclaimed":true}),
                            Some(alias) => serde_json::to_value(alias)?,
                        }
                    }
                    "cppClassNamed" | "cppTypeNamed" => output(types.class(
                        written,
                        &scopes,
                        reference.as_ref().unwrap(),
                        answer["name"] == "cppTypeNamed",
                    )?),
                    "cppAliasedTypeWritten" => {
                        match types.alias_written(written, &scopes, reference.as_ref().unwrap())? {
                            None => json!({"unclaimed":true}),
                            Some(None) => Value::Null,
                            Some(Some((text, alias))) => {
                                json!({"text":text,"alias":{"id":alias.id}})
                            }
                        }
                    }
                    "cppTemplateParameters" => {
                        let mut names: Vec<_> = types
                            .parameters(types.ids[answer["node"].as_str().unwrap()])?
                            .into_iter()
                            .collect();
                        names.sort();
                        json!(names)
                    }
                    name => panic!("unknown helper {name}"),
                };
                let mut expected = normalized(answer["expected"].clone());
                if answer["name"] == "cppTemplateParameters" {
                    expected
                        .as_array_mut()
                        .unwrap()
                        .sort_by(|a, b| a.as_str().cmp(&b.as_str()));
                }
                assert_eq!(actual, expected, "{} {}", case["origin"], answer);
                total += 1;
            }
        }
        assert_eq!(total, 481);
        Ok(())
    }
    #[test]
    fn cpp_complete_graph_audit() -> Result<()> {
        let corpus: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/upstream/cpp-resolution/cases.json"
        ))?;
        let normalize = |mut value: Value| {
            let object = value.as_object_mut().unwrap();
            object.remove("updatedAt");
            if object.contains_key("qualifiedName") {
                for key in ["isExported", "isAsync", "isStatic", "isAbstract"] {
                    object.entry(key).or_insert(json!(false));
                }
                object.entry("visibility").or_insert(Value::Null);
            }
            value.sort_all_objects();
            value.to_string()
        };
        let difference = |left: &[String], right: &[String]| {
            let mut counts = BTreeMap::new();
            for row in right {
                *counts.entry(row).or_insert(0usize) += 1;
            }
            left.iter()
                .filter_map(|row| {
                    let count = counts.entry(row).or_default();
                    if *count > 0 {
                        *count -= 1;
                        None
                    } else {
                        Some(serde_json::from_str::<Value>(row).unwrap())
                    }
                })
                .collect::<Vec<_>>()
        };
        let mut differences = Vec::new();
        for case in corpus["cases"].as_array().unwrap() {
            let temporary = tempfile::tempdir()?;
            let root = temporary.path().join("project");
            let database = temporary.path().join("index.sqlite");
            for (file, source) in case["sources"].as_object().unwrap() {
                let path = root.join(file);
                std::fs::create_dir_all(path.parent().unwrap())?;
                std::fs::write(path, source.as_str().unwrap())?;
            }
            let mut engine = crate::Engine::open(&root, Some(&database))?;
            engine.sync()?;
            let graph = crate::graph::Graph::load(&engine.connection, 1)?;
            let actual_nodes: Vec<_> = graph
                .files
                .values()
                .filter_map(|file| file.analysis.extraction.as_ref())
                .flat_map(|extraction| &extraction.nodes)
                .map(|node| normalize(serde_json::to_value(node).unwrap()))
                .collect();
            let expected_nodes: Vec<_> = case["nodes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|node| {
                    let mut node = node.clone();
                    node["updatedAt"] = json!(0);
                    let node: Node = serde_json::from_value(node).unwrap();
                    normalize(serde_json::to_value(node).unwrap())
                })
                .collect();
            ensure!(
                difference(&actual_nodes, &expected_nodes).is_empty()
                    && difference(&expected_nodes, &actual_nodes).is_empty(),
                "{} node records differ",
                case["origin"]
            );
            let actual: Vec<_> = graph
                .canonical_edges
                .iter()
                .flatten()
                .map(|edge| normalize(serde_json::to_value(edge).unwrap()))
                .collect();
            let expected: Vec<_> =
                serde_json::from_value::<Vec<prometeu_extract::Edge>>(case["allEdges"].clone())?
                    .into_iter()
                    .map(|edge| normalize(serde_json::to_value(edge).unwrap()))
                    .collect();
            let missing = difference(&expected, &actual);
            let extra = difference(&actual, &expected);
            if !missing.is_empty() || !extra.is_empty() {
                differences.push(json!({"id":case["id"],"origin":case["origin"],"missing":missing,"extra":extra}));
            }
            drop(graph);
            drop(engine);
            for file in case["sources"].as_object().unwrap().keys() {
                std::fs::write(root.join(file), "modified outside committed snapshot")?;
            }
            let reopened = crate::Engine::open(&root, Some(&database))?;
            let graph = crate::graph::Graph::load(&reopened.connection, 1)?;
            let reopened: Vec<_> = graph
                .canonical_edges
                .iter()
                .flatten()
                .map(|edge| normalize(serde_json::to_value(edge).unwrap()))
                .collect();
            ensure!(
                difference(&actual, &reopened).is_empty()
                    && difference(&reopened, &actual).is_empty(),
                "{} graph changed after reopening",
                case["origin"]
            );
        }
        std::fs::write(
            std::env::temp_dir().join("prometeu-cpp-complete-diffs.json"),
            serde_json::to_vec_pretty(&differences)?,
        )?;
        let accepted: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/upstream/cpp-resolution/uncertainty.json"
        ))?;
        let summarize = |entries: &[Value]| {
            entries
                .iter()
                .map(|entry| {
                    let sorted = |field: &str| {
                        let mut rows = entry[field]
                            .as_array()
                            .unwrap()
                            .iter()
                            .map(|row| normalize(row.clone()))
                            .collect::<Vec<_>>();
                        rows.sort();
                        rows
                    };
                    (
                        entry["id"].as_u64().unwrap(),
                        (sorted("missing"), sorted("extra")),
                    )
                })
                .collect::<BTreeMap<_, _>>()
        };
        assert_eq!(
            summarize(&differences),
            summarize(accepted.as_array().unwrap()),
            "complete C/C++ graphs differ beyond documented uncertainty"
        );
        assert!(accepted.as_array().unwrap().iter().all(|entry| {
            entry["reason"]
                .as_str()
                .is_some_and(|reason| !reason.is_empty())
        }));
        Ok(())
    }
}
