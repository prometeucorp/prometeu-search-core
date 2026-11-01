//! Cross-file React route tables and constant path finalization from committed sources.
use super::{
    object::{matching, skip_string},
    react_exports,
    react_routes::{self, Name, Options, Part},
    regex,
};
use crate::graph::js_imports::{self, Mapping};
use anyhow::{Result, ensure};
use prometeu_extract::{Node, UnresolvedReference};
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::rc::Rc;
pub(crate) struct Context<'a> {
    pub nodes: &'a [Node],
    pub sources: &'a BTreeMap<&'a str, &'a str>,
    pub resolve_module: &'a super::ModuleResolver<'a>,
    files: HashMap<&'a str, Vec<&'a Node>>,
    imports: HashMap<&'a str, Vec<Mapping>>,
    safe: RefCell<HashMap<String, Rc<str>>>,
    work: Cell<usize>,
}
#[derive(Default)]
pub(crate) struct Finalized {
    pub updates: Vec<Node>,
    pub nodes: Vec<Node>,
    pub references: Vec<UnresolvedReference>,
}
#[derive(Clone)]
struct Table {
    file: String,
    at: usize,
}
#[derive(Debug, serde::Serialize)]
#[serde(untagged)]
pub(crate) enum Site {
    Literal(usize),
    Name(Name),
}
impl<'a> Context<'a> {
    pub(crate) fn new(
        nodes: &'a [Node],
        sources: &'a BTreeMap<&'a str, &'a str>,
        resolve_module: &'a super::ModuleResolver<'a>,
    ) -> Result<Self> {
        let mut files = HashMap::<&str, Vec<&Node>>::new();
        for node in nodes {
            files.entry(&node.file_path).or_default().push(node);
        }
        let mut imports = HashMap::new();
        for (&file, &source) in sources {
            if [".ts", ".tsx", ".mts", ".cts", ".js", ".jsx", ".mjs", ".cjs"]
                .iter()
                .any(|suffix| file.ends_with(suffix))
            {
                imports.insert(file, js_imports::extract(source)?);
            }
        }
        Ok(Self {
            nodes,
            sources,
            resolve_module,
            files,
            imports,
            safe: RefCell::new(HashMap::new()),
            work: Cell::new(0),
        })
    }
    fn step(&self) -> Result<()> {
        let work = self.work.get() + 1;
        self.work.set(work);
        ensure!(work < 1_000_000, "React table traversal limit exceeded");
        Ok(())
    }
    fn in_file(&self, file: &str) -> impl Iterator<Item = &'a Node> + '_ {
        self.files.get(file).into_iter().flatten().copied()
    }
    fn imported(&self, file: &str, name: &str) -> Option<&Mapping> {
        self.imports
            .get(file)?
            .iter()
            .find(|mapping| mapping.local_name == name)
    }
    fn safe(&self, file: &str) -> Rc<str> {
        if let Some(source) = self.safe.borrow().get(file) {
            return source.clone();
        }
        let source: Rc<str> = crate::synthesis::comments::strip_for_regex(
            self.sources.get(file).copied().unwrap_or(""),
            "typescript",
        )
        .into();
        self.safe.borrow_mut().insert(file.into(), source.clone());
        source
    }
    fn named(&self, named: &Name, file: &str, depth: usize) -> Result<Vec<Table>> {
        self.step()?;
        if !self.sources.contains_key(file) {
            return Ok(Vec::new());
        }
        let safe = self.safe(file);
        let local = self.declared(&safe, &named.name, named.call, Some(named.at), file)?;
        if !local.is_empty() {
            return self.at(local, file, depth);
        }
        let Some(mapping) = self.imported(file, &named.name) else {
            return Ok(Vec::new());
        };
        if mapping.is_namespace || depth > 3 {
            return Ok(Vec::new());
        }
        let Some(target) = (self.resolve_module)(&mapping.source, file, language(file))? else {
            return Ok(Vec::new());
        };
        self.exported(
            &target,
            if mapping.is_default {
                "default"
            } else {
                &mapping.exported_name
            },
            named.call,
            depth + 1,
        )
    }
    fn at(&self, sites: Vec<Site>, file: &str, depth: usize) -> Result<Vec<Table>> {
        let mut result = Vec::new();
        for site in sites {
            self.step()?;
            match site {
                Site::Literal(at) => result.push(Table {
                    file: file.into(),
                    at,
                }),
                Site::Name(name) if depth <= 3 => {
                    result.extend(self.named(&name, file, depth + 1)?)
                }
                _ => {}
            }
        }
        Ok(result)
    }
    fn exported(&self, file: &str, exported: &str, call: bool, depth: usize) -> Result<Vec<Table>> {
        self.step()?;
        if !self.sources.contains_key(file) {
            return Ok(Vec::new());
        }
        let safe = self.safe(file);
        let mut local = exported.to_owned();
        if exported == "default" {
            if let Some(m) = regex(r"\bexport\s+default\s+")?.find(&safe)? {
                let after = m.end();
                let inline = if call {
                    returned(&safe, after)?
                } else {
                    held(&safe, after, 0)?
                };
                if !inline.is_empty() {
                    return self.at(inline, file, depth);
                }
                local = regex(r"^([A-Za-z_$][\w$]*)\s*(?:;|\n|$)")?
                    .captures(&safe[after..])?
                    .map(|m| m[1].to_owned())
                    .unwrap_or_default();
            } else {
                local = regex(r"\bexport\s*\{[^}]*?\b([A-Za-z_$][\w$]*)\s+as\s+default\b")?
                    .captures(&safe)?
                    .map(|m| m[1].to_owned())
                    .unwrap_or_default();
            }
        } else if let Some(alias) = regex(&format!(
            r"\bexport\s*\{{[^}}]*?\b([A-Za-z_$][\w$]*)\s+as\s+{}\b",
            regex::escape(exported)
        ))?
        .captures(&safe)?
        {
            local = alias[1].into();
        }
        let declared = if local.is_empty() {
            Vec::new()
        } else {
            self.declared(&safe, &local, call, None, file)?
        };
        if !declared.is_empty() {
            return self.at(declared, file, depth);
        }
        if depth > 3 {
            return Ok(Vec::new());
        }
        for re in react_exports::reexports(self.sources.get(file).copied().unwrap_or(""))? {
            if re.kind == "named" && re.exported_name.as_deref() != Some(exported) {
                continue;
            }
            let Some(target) = (self.resolve_module)(&re.source, file, language(file))? else {
                continue;
            };
            let found = self.exported(
                &target,
                if re.kind == "named" {
                    re.original_name.as_deref().unwrap()
                } else {
                    exported
                },
                call,
                depth + 1,
            )?;
            if !found.is_empty() {
                return Ok(found);
            }
        }
        Ok(Vec::new())
    }
    fn declared(
        &self,
        safe: &str,
        name: &str,
        call: bool,
        use_at: Option<usize>,
        file: &str,
    ) -> Result<Vec<Site>> {
        let id = regex::escape(name);
        let declarations = regex(&format!(
            r"(?:^|[^\w$.])((?:const|let|var)\s+{id}\s*(?::[^=;]*)?=\s*)"
        ))?
        .captures_iter(safe)
        .collect::<std::result::Result<Vec<_>, _>>()?;
        let keywords = declarations
            .iter()
            .map(|d| d.get(1).unwrap().start())
            .collect::<Vec<_>>();
        if let Some(chosen) = self.visible(use_at, safe, file, &keywords) {
            let after = declarations[chosen].get(0).unwrap().end();
            return if call {
                returned(safe, after)
            } else {
                held(safe, after, 0)
            };
        }
        if !call {
            return Ok(Vec::new());
        }
        let Some(function) = regex(&format!(r"(?:^|[^\w$.])function\s+{id}\s*\("))?.find(safe)?
        else {
            return Ok(Vec::new());
        };
        returned(
            safe,
            function.start() + function.as_str().find("function").unwrap(),
        )
    }
    fn visible(
        &self,
        use_at: Option<usize>,
        safe: &str,
        file: &str,
        keywords: &[usize],
    ) -> Option<usize> {
        let starts = std::iter::once(0)
            .chain(safe.match_indices('\n').map(|(at, _)| at + 1))
            .collect::<Vec<_>>();
        let offset = |line: u32, column: u32| {
            let start = starts
                .get(line.saturating_sub(1) as usize)
                .copied()
                .unwrap_or(safe.len());
            let original = self.sources.get(file).copied().unwrap_or(safe);
            let line = original[start..].split('\n').next().unwrap_or("");
            start + byte(line, column as usize)
        };
        let functions = self
            .in_file(file)
            .filter(|node| matches!(node.kind.as_str(), "function" | "method" | "component"))
            .map(|node| {
                (
                    offset(node.start_line, node.start_column),
                    offset(node.end_line, node.end_column),
                )
            })
            .collect::<Vec<_>>();
        let scopes = functions
            .iter()
            .enumerate()
            .filter(|(_, span)| use_at.is_some_and(|at| span.0 < at && span.1 > at))
            .map(|(i, _)| i)
            .collect::<HashSet<_>>();
        let mut best = None;
        let mut depth = None;
        for (index, &at) in keywords.iter().enumerate() {
            let enclosing = functions
                .iter()
                .enumerate()
                .filter(|(_, span)| span.0 < at && span.1 > at)
                .map(|(i, _)| i)
                .collect::<Vec<_>>();
            if !enclosing.iter().all(|i| scopes.contains(i)) {
                continue;
            }
            if depth.is_none_or(|depth| {
                enclosing.len() > depth
                    || enclosing.len() == depth && use_at.is_some_and(|use_at| at < use_at)
            }) {
                best = Some(index);
                depth = Some(enclosing.len());
            }
        }
        best
    }
    pub(crate) fn constant(&self, expression: &str, from: &str) -> Result<Option<String>> {
        let mut parts = expression.split('.');
        let root = parts.next().unwrap_or("");
        let keys = parts.collect::<Vec<_>>();
        if root.is_empty() || keys.is_empty() {
            return Ok(None);
        }
        let Some((text, at)) = self.object(root, from)? else {
            return Ok(None);
        };
        let Some(value) = read_value(&text, at, &keys)? else {
            return Ok(None);
        };
        Ok(regex(r#"^(["'])((?:\\.|(?!\1).)*)\1"#)?
            .captures(&value)?
            .map(|m| m[2].into()))
    }
    fn object(&self, root: &str, from: &str) -> Result<Option<(String, usize)>> {
        let mut file = from.to_owned();
        let mut name = root.to_owned();
        if let Some(mapping) = self.imported(from, root) {
            let Some(resolved) = (self.resolve_module)(&mapping.source, from, "typescript")? else {
                return Ok(None);
            };
            file = resolved;
            if !mapping.exported_name.is_empty()
                && !matches!(mapping.exported_name.as_str(), "default" | "*")
            {
                name = mapping.exported_name.clone();
            }
        }
        let Some(decl) = self.in_file(&file).find(|node| {
            node.name == name && matches!(node.kind.as_str(), "constant" | "variable")
        }) else {
            return Ok(None);
        };
        let text = self
            .sources
            .get(file.as_str())
            .copied()
            .unwrap_or("")
            .split('\n')
            .skip(decl.start_line.saturating_sub(1) as usize)
            .take(decl.end_line.saturating_sub(decl.start_line) as usize + 1)
            .collect::<Vec<_>>()
            .join("\n");
        let from = regex(&format!(r"\b{}\b", regex::escape(&name)))?
            .find(&text)?
            .map_or(0, |m| m.start());
        let Some(at) = text[from..].find('{').map(|at| at + from) else {
            return Ok(None);
        };
        Ok(Some((text, at)))
    }
    fn values(&self, parts: &[Part], from: &str, fallback: bool) -> Result<Option<Vec<String>>> {
        let mut values = Vec::new();
        for part in parts {
            let value = if let Some(lit) = &part.lit {
                Some(lit.clone())
            } else if let Some(expr) = &part.expr {
                self.constant(expr, from)?
            } else {
                None
            };
            if let Some(value) = value {
                values.push(value);
            } else if fallback {
                values.push(format!(
                    "{{{}}}",
                    part.expr.as_deref().unwrap_or("undefined")
                ));
            } else {
                return Ok(None);
            }
        }
        Ok(Some(values))
    }
    pub(crate) fn finalize(&self) -> Result<Finalized> {
        let mut output = Finalized::default();
        for route in self.nodes.iter().filter(|node| node.kind == "route") {
            let Some(parts) = route
                .signature
                .as_deref()
                .and_then(|signature| signature.strip_prefix("route-parts:"))
            else {
                continue;
            };
            let Ok(parts) = serde_json::from_str::<Vec<Part>>(parts) else {
                continue;
            };
            let Some(values) = self.values(&parts, &route.file_path, false)? else {
                continue;
            };
            let name = react_routes::compose(values);
            if name != route.name {
                let mut updated = route.clone();
                updated.name = name;
                output.updates.push(updated);
            }
        }
        let mut consumers = self
            .nodes
            .iter()
            .filter(|node| {
                node.kind == "import"
                    && ["react-router-dom", "react-router", "react-router-native"]
                        .contains(&node.name.as_str())
                    && !crate::classify::is_test_path(&node.file_path)
            })
            .map(|node| node.file_path.as_str())
            .collect::<Vec<_>>();
        consumers.sort_by(|left, right| left.encode_utf16().cmp(right.encode_utf16()));
        consumers.dedup();
        let mut queue = VecDeque::new();
        let mut queued = HashSet::new();
        let enqueue = |table: Table,
                       prefix: Vec<String>,
                       from: &str,
                       layouts: Vec<String>,
                       queue: &mut VecDeque<(Table, Vec<String>, Vec<String>)>,
                       queued: &mut HashSet<(String, usize, Vec<String>)>| {
            if queued.len() >= 512 || !queued.insert((table.file.clone(), table.at, prefix.clone()))
            {
                return;
            }
            let layouts = if table.file == from {
                layouts
            } else {
                Vec::new()
            };
            queue.push_back((table, prefix, layouts));
        };
        for file in consumers {
            self.step()?;
            let Some(&source) = self.sources.get(file) else {
                continue;
            };
            if source.is_empty()
                || !regex(r"\buseRoutes\b|\bcreate(?:Browser|Hash|Memory)Router\b|<Route\b")?
                    .is_match(source)?
            {
                continue;
            }
            let scan = react_routes::scan(
                source,
                jsx(file),
                Options {
                    objects: router_call(source)?,
                    uses: true,
                    table: None,
                },
            )?;
            for site in scan.uses {
                let tables = if let Some(at) = site.literal {
                    vec![Table {
                        file: file.into(),
                        at,
                    }]
                } else {
                    self.named(site.name.as_ref().unwrap(), file, 0)?
                };
                let prefix = self.values(&site.prefix, file, true)?.unwrap();
                for table in tables {
                    enqueue(
                        table,
                        prefix.clone(),
                        file,
                        site.layouts.clone(),
                        &mut queue,
                        &mut queued,
                    );
                }
            }
        }
        let mut made = HashSet::new();
        while let Some((table, prefix, layouts)) = queue.pop_front() {
            self.step()?;
            let file = &table.file;
            let Some(&source) = self.sources.get(file.as_str()) else {
                continue;
            };
            let scan = react_routes::scan(
                source,
                jsx(file),
                Options {
                    table: Some((
                        table.at,
                        prefix
                            .into_iter()
                            .map(|lit| Part {
                                lit: Some(lit),
                                expr: None,
                            })
                            .collect(),
                        layouts,
                    )),
                    ..Options::default()
                },
            )?;
            for named in scan.names {
                let prefix = self.values(&named.prefix, file, true)?.unwrap();
                for table in self.named(&named.name, file, 0)? {
                    enqueue(
                        table,
                        prefix.clone(),
                        file,
                        named.layouts.clone(),
                        &mut queue,
                        &mut queued,
                    );
                }
            }
            if router_call(source)? {
                continue;
            }
            for route in scan.routes {
                let line = source[..route.at].bytes().filter(|&b| b == b'\n').count() as u32 + 1;
                let id = format!("route:{file}:{line}:table:{}", route.path);
                if !made.insert(id.clone()) {
                    continue;
                }
                let mut node = prometeu_extract::supplemental::Context::new(
                    file,
                    source,
                    if file.ends_with(".tsx") { "tsx" } else { "jsx" },
                )
                .result
                .nodes
                .remove(0);
                node.id = id;
                node.kind = "route".into();
                node.is_exported = None;
                node.name = self
                    .values(&route.parts, file, false)?
                    .map(react_routes::compose)
                    .unwrap_or_else(|| route.path.clone());
                node.qualified_name = format!("{file}::route:{}", route.path);
                node.start_line = line;
                node.end_line = line;
                node.start_column = 0;
                node.end_column = 0;
                for name in route.component.or(route.lazy).into_iter().chain(
                    route
                        .layouts
                        .into_iter()
                        .map(|layout| format!("layout:{layout}")),
                ) {
                    output.references.push(UnresolvedReference {
                        from_node_id: node.id.clone(),
                        reference_name: name,
                        reference_kind: "references".into(),
                        line,
                        column: 0,
                        file_path: Some(file.clone()),
                        language: Some(node.language.clone()),
                        candidates: None,
                    });
                }
                output.nodes.push(node);
            }
        }
        Ok(output)
    }
}
fn language(file: &str) -> &'static str {
    if file.ends_with(".tsx") {
        "tsx"
    } else if [".ts", ".mts", ".cts"]
        .iter()
        .any(|suffix| file.ends_with(suffix))
    {
        "typescript"
    } else if file.ends_with(".jsx") {
        "jsx"
    } else {
        "javascript"
    }
}
fn jsx(file: &str) -> bool {
    ![".ts", ".mts", ".cts"]
        .iter()
        .any(|suffix| file.ends_with(suffix))
}
fn router_call(source: &str) -> Result<bool> {
    Ok(regex(r"\b(?:createBrowserRouter|createHashRouter|createMemoryRouter|createRoutesFromElements|useRoutes)\b")?.is_match(source)?)
}
fn byte(source: &str, units: usize) -> usize {
    let mut seen = 0;
    for (at, ch) in source.char_indices() {
        if seen >= units {
            return at;
        }
        seen += ch.len_utf16();
    }
    source.len()
}
fn skip_space(source: &str, at: usize) -> usize {
    at + source
        .get(at..)
        .unwrap_or("")
        .chars()
        .take_while(|&ch| crate::search::query_whitespace(ch))
        .map(char::len_utf8)
        .sum::<usize>()
}
fn simple_string(source: &str, at: usize) -> usize {
    let bytes = source.as_bytes();
    let quote = bytes[at];
    let mut i = at + 1;
    while i < bytes.len() && bytes[i] != quote {
        if bytes[i] == b'\\' {
            i += 1;
        }
        i += 1;
    }
    (i + 1).min(bytes.len())
}
fn skip_value(source: &str, at: usize) -> usize {
    let mut depth = 0;
    let mut i = at;
    while i < source.len() {
        let ch = source.as_bytes()[i];
        if matches!(ch, b'\'' | b'"' | b'`') {
            i = simple_string(source, i);
            continue;
        }
        if matches!(ch, b'{' | b'[' | b'(') {
            depth += 1;
        } else if matches!(ch, b'}' | b']' | b')') {
            if depth == 0 {
                return i;
            }
            depth -= 1;
        } else if ch == b',' && depth == 0 {
            return i;
        }
        i += 1;
    }
    source.len()
}
fn read_value(source: &str, at: usize, keys: &[&str]) -> Result<Option<String>> {
    let mut at = at;
    for (index, key) in keys.iter().enumerate() {
        let mut i = at + 1;
        let mut found = None;
        while i < source.len() {
            let field = regex(r#"^\s*(?:([A-Za-z_$][\w$]*)|["']([^"']+)["'])\s*:\s*"#)?
                .captures(&source[i..])?;
            if let Some(field) = field {
                let name = field.get(1).or_else(|| field.get(2)).unwrap().as_str();
                let value = i + field.get(0).unwrap().end();
                let end = skip_value(source, value);
                if name == *key {
                    found = Some((value, end));
                    break;
                }
                if source.as_bytes().get(end) != Some(&b',') {
                    return Ok(None);
                }
                i = end + 1;
            } else {
                let next = skip_value(source, i);
                if source.as_bytes().get(next) != Some(&b',') {
                    return Ok(None);
                }
                i = next + 1;
            }
        }
        let Some((value, end)) = found else {
            return Ok(None);
        };
        if index + 1 == keys.len() {
            return Ok(Some(
                source[value..end]
                    .trim_matches(crate::search::query_whitespace)
                    .into(),
            ));
        }
        if source.as_bytes().get(value) != Some(&b'{') {
            return Ok(None);
        }
        at = value;
    }
    Ok(None)
}

pub(crate) fn held(safe: &str, mut at: usize, depth: usize) -> Result<Vec<Site>> {
    ensure!(depth < 128, "React table expression nesting limit exceeded");
    while safe.as_bytes().get(at) == Some(&b'(') {
        at = skip_space(safe, at + 1);
    }
    if matches!(safe.as_bytes().get(at), Some(b'[' | b'{')) {
        return Ok(vec![Site::Literal(at)]);
    }
    if let Some(memo) =
        regex(r"^(?:React\s*\.\s*)?useMemo\s*(?:<[^()]*>)?\s*\(\s*")?.find(&safe[at..])?
    {
        let after = at + memo.end();
        if let Some(function) = regex(r"^([A-Za-z_$][\w$]*)\s*,")?.captures(&safe[after..])? {
            return Ok(vec![Site::Name(Name {
                name: function[1].into(),
                call: true,
                at: after,
            })]);
        }
        return returned_inner(safe, after, depth + 1);
    }
    Ok(regex(r"^(await\s+)?([A-Za-z_$][\w$]*)\s*\(")?
        .captures(&safe[at..])?
        .map(|m| {
            vec![Site::Name(Name {
                name: m[2].into(),
                call: true,
                at: at + m.get(1).map_or(0, |m| m.as_str().len()),
            })]
        })
        .unwrap_or_default())
}
pub(crate) fn returned(safe: &str, at: usize) -> Result<Vec<Site>> {
    returned_inner(safe, at, 0)
}
fn returned_inner(safe: &str, at: usize, depth: usize) -> Result<Vec<Site>> {
    ensure!(depth < 128, "React returned-table nesting limit exceeded");
    let Some(body) = function_body(safe, at)? else {
        return Ok(Vec::new());
    };
    if !body.block {
        return held(safe, body.start, depth + 1);
    }
    let mut out = Vec::new();
    for value in own_returns(safe, body.start, body.end)? {
        out.extend(held(safe, value, depth + 1)?);
    }
    Ok(out)
}
#[derive(Debug, serde::Serialize)]
pub(crate) struct Body {
    pub block: bool,
    pub start: usize,
    pub end: usize,
}
pub(crate) fn function_body(safe: &str, at: usize) -> Result<Option<Body>> {
    let mut i = skip_space(safe, at);
    if let Some(word) = regex(r"^async\b")?.find(&safe[i..])? {
        i = skip_space(safe, i + word.end());
    }
    let arrow = if let Some(word) = regex(r"^function\b")?.find(&safe[i..])? {
        i = skip_space(safe, i + word.end());
        false
    } else {
        true
    };
    if !arrow && let Some(name) = regex(r"^\*?\s*[A-Za-z_$][\w$]*")?.find(&safe[i..])? {
        i = skip_space(safe, i + name.end());
    }
    if safe.as_bytes().get(i) == Some(&b'(') {
        let Some(close) = matching(safe, i)? else {
            return Ok(None);
        };
        i = skip_space(safe, close + 1);
    } else {
        if !arrow {
            return Ok(None);
        }
        let Some(param) = regex(r"^[A-Za-z_$][\w$]*")?.find(&safe[i..])? else {
            return Ok(None);
        };
        i = skip_space(safe, i + param.end());
    }
    if safe.as_bytes().get(i) == Some(&b':') {
        let Some(stop) = safe[i..].find(if arrow { "=>" } else { "{" }) else {
            return Ok(None);
        };
        i += stop;
    }
    if arrow {
        if !safe[i..].starts_with("=>") {
            return Ok(None);
        }
        i = skip_space(safe, i + 2);
        if safe.as_bytes().get(i) != Some(&b'{') {
            return Ok(Some(Body {
                block: false,
                start: i,
                end: i,
            }));
        }
    }
    if safe.as_bytes().get(i) != Some(&b'{') {
        return Ok(None);
    }
    Ok(block_end(safe, i)?.map(|end| Body {
        block: true,
        start: i,
        end,
    }))
}
fn id(ch: u8) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, b'_' | b'$')
}
fn opens_string(safe: &str, at: usize) -> bool {
    safe.as_bytes()[at] == b'`'
        || matches!(safe.as_bytes()[at], b'\'' | b'"')
            && !at.checked_sub(1).is_some_and(|i| id(safe.as_bytes()[i]))
}
fn past_body(safe: &str, paren: Option<usize>) -> Result<Option<usize>> {
    let Some(paren) = paren else { return Ok(None) };
    let Some(params) = matching(safe, paren)? else {
        return Ok(None);
    };
    let mut i = skip_space(safe, params + 1);
    if safe.as_bytes().get(i) == Some(&b':') {
        let Some(start) = safe[i..].find('{') else {
            return Ok(None);
        };
        i += start;
    }
    if safe.as_bytes().get(i) == Some(&b'{') {
        block_end(safe, i)
    } else {
        Ok(None)
    }
}
pub(crate) fn own_returns(safe: &str, open: usize, close: usize) -> Result<Vec<usize>> {
    let mut values = Vec::new();
    let mut i = open + 1;
    while i < close && i < safe.len() {
        let ch = safe.as_bytes()[i];
        if opens_string(safe, i) {
            let Some(end) = skip_string(safe, i)? else {
                break;
            };
            i = end;
        } else if ch == b'=' && safe.as_bytes().get(i + 1) == Some(&b'>') {
            let body = skip_space(safe, i + 2);
            let end = if safe.as_bytes().get(body) == Some(&b'{') {
                block_end(safe, body)?
            } else {
                None
            };
            i = end.unwrap_or(i + 1);
        } else if (ch.is_ascii_alphabetic() || matches!(ch, b'_' | b'$'))
            && !(i > 0 && (id(safe.as_bytes()[i - 1]) || safe.as_bytes()[i - 1] == b'.'))
        {
            let word = regex(r"^[A-Za-z_$][\w$]*")?.find(&safe[i..])?.unwrap();
            let name = word.as_str();
            let word_end = i + word.end();
            let next = skip_space(safe, word_end);
            if name == "return" {
                values.push(next);
            } else if name == "function" {
                let paren = safe[next..].find('(').map(|at| at + next);
                if let Some(end) = past_body(safe, paren)?
                    && end > 0
                {
                    i = end + 1;
                    continue;
                }
            } else if safe.as_bytes().get(next) == Some(&b'(')
                && !matches!(name, "if" | "for" | "while" | "switch" | "catch" | "with")
                && let Some(end) = past_body(safe, Some(next))?
                && end > 0
            {
                i = end + 1;
                continue;
            }
            i = word_end - 1;
        }
        i += 1;
    }
    Ok(values)
}
pub(crate) fn block_end(safe: &str, open: usize) -> Result<Option<usize>> {
    let mut depth = 0;
    let mut i = open;
    while i < safe.len() {
        if opens_string(safe, i) {
            let Some(end) = skip_string(safe, i)? else {
                return Ok(None);
            };
            i = end + 1;
            continue;
        }
        let ch = safe.as_bytes()[i];
        if matches!(ch, b'{' | b'(' | b'[') {
            depth += 1;
        } else if matches!(ch, b'}' | b')' | b']') {
            depth -= 1;
            if depth == 0 {
                return Ok(Some(i));
            }
        }
        i += 1;
    }
    Ok(None)
}

impl Context<'_> {
    pub(crate) fn configured_href(&self, expression: &str, from: &str) -> Result<Option<String>> {
        let Some(m)=regex(r"^\s*([A-Za-z_$][\w$]*)((?:\s*\??\.\s*[A-Za-z_$][\w$]*)+)\s*(\((?:[^()]|\([^()]*\))*\))?\s*$")?.captures(expression)?else{return Ok(None)};
        let keys = m[2]
            .split('.')
            .map(|key| {
                key.chars()
                    .filter(|&c| c != '?' && !crate::search::query_whitespace(c))
                    .collect::<String>()
            })
            .filter(|key| !key.is_empty())
            .collect::<Vec<_>>();
        let Some((text, open)) = self.object(&m[1], from)? else {
            return Ok(None);
        };
        let Some(mut value) = read_value(
            &text,
            open,
            &keys.iter().map(String::as_str).collect::<Vec<_>>(),
        )?
        else {
            return Ok(None);
        };
        if m.get(3).is_some() {
            let Some(body)=regex(r"^(?:async\s+)?(?:\([^()]*(?:\([^()]*\)[^()]*)*\)|[A-Za-z_$][\w$]*)\s*(?::\s*[^=]+?)?=>\s*")?.find(&value)?else{return Ok(None)};
            value = value[body.end()..]
                .trim_matches(crate::search::query_whitespace)
                .into();
            if value.starts_with('{') {
                value = regex(r#"\breturn\s+([`'"][\s\S]*?[`'"])\s*;?\s*}"#)?
                    .captures(&value)?
                    .map(|m| m[1].to_owned())
                    .unwrap_or_default();
            }
        }
        if !value.starts_with(['`', '\'', '"']) {
            return Ok(None);
        }
        if !value.starts_with('`') {
            return Ok(Some(value));
        }
        let mut out = String::new();
        let mut i = 0;
        while i < value.len() {
            if value[i..].starts_with("${") {
                let mut depth = 0;
                let mut end = i + 1;
                while end < value.len() {
                    let ch = value.as_bytes()[end];
                    if ch == b'{' {
                        depth += 1;
                    } else if ch == b'}' {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    end += 1;
                }
                let next = value.as_bytes().get(end + 1);
                if out.ends_with('/')
                    && (next.is_none() || matches!(next, Some(b'/' | b'`' | b'?')))
                {
                    out.push_str(&value[i..(end + 1).min(value.len())]);
                }
                i = (end + 1).min(value.len());
            } else {
                let ch = value[i..].chars().next().unwrap();
                out.push(ch);
                i += ch.len_utf8();
            }
        }
        Ok(Some(out))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    #[test]
    fn finalized_tables_match_independent_reference() {
        let corpus: Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/upstream/synthesis/cases.json"
        ))
        .unwrap();
        let mut checked = 0;
        for case in corpus["cases"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|case| case.get("finalization").is_some())
        {
            checked += 1;
            let mut nodes = case["inputNodes"].clone();
            for node in nodes.as_array_mut().unwrap() {
                node["updatedAt"] = json!(0);
            }
            let nodes: Vec<Node> = serde_json::from_value(nodes).unwrap();
            let sources = case["files"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(path, source)| (path.as_str(), source.as_str().unwrap()))
                .collect();
            let resolve = |specifier: &str, from: &str, language: &str| -> Result<Option<String>> {
                Ok(case["modules"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|row| {
                        row["specifier"] == specifier
                            && row["from"] == from
                            && row["language"] == language
                    })
                    .and_then(|row| row["result"].as_str().map(str::to_owned)))
            };
            let result = Context::new(&nodes, &sources, &resolve)
                .unwrap()
                .finalize()
                .unwrap();
            let mut actual = json!({"updates":result.updates,"nodes":result.nodes,"references":result.references});
            for field in ["updates", "nodes"] {
                for node in actual[field].as_array_mut().unwrap() {
                    node.as_object_mut().unwrap().remove("updatedAt");
                }
            }
            let mut expected = case["finalization"].clone();
            for node in expected["updates"].as_array_mut().unwrap() {
                node["updatedAt"] = json!(0);
                let normalized: Node = serde_json::from_value(node.clone()).unwrap();
                *node = serde_json::to_value(normalized).unwrap();
                node.as_object_mut().unwrap().remove("updatedAt");
            }
            assert_eq!(actual, expected, "{} {}", case["id"], case["origin"]);
        }
        assert!(checked > 0);
    }
}
