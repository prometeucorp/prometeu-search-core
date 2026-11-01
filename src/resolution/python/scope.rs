use super::*;
use serde::Serialize;
use std::rc::Rc;

#[path = "bindings.rs"]
mod bindings;
pub(super) use bindings::assignments;
pub(super) use bindings::{LineBinding, LocalBindings};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub(super) enum Binding {
    Scope(Scope),
    ImportedName {
        source: String,
        #[serde(rename = "exportedName")]
        exported_name: String,
    },
    ImportedModule {
        module: String,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum Scope {
    Module,
    Local,
}

struct File<'a> {
    nodes: Vec<&'a Node>,
    lines: Vec<String>,
    starts: Vec<bool>,
    functions: Vec<&'a Node>,
    innermost: Vec<Option<usize>>,
    parents: Vec<Option<usize>>,
}
#[derive(Default)]
struct ScopeText {
    params: Vec<String>,
    statements: Vec<String>,
    nested: Vec<String>,
}

pub(super) struct Scopes<'a> {
    files: HashMap<&'a str, File<'a>>,
    text: RefCell<HashMap<&'a str, Rc<ScopeText>>>,
    bindings: RefCell<HashMap<(&'a str, String), Option<Binding>>>,
    local_bindings: RefCell<HashMap<(&'a str, String), Rc<LocalBindings>>>,
    work: Cell<usize>,
}
impl<'a> Scopes<'a> {
    pub(super) fn new(nodes: &'a [Node], sources: &BTreeMap<&str, &str>) -> Self {
        let mut files = HashMap::new();
        for node in nodes.iter().filter(|node| node.language == "python") {
            files.entry(node.file_path.as_str()).or_insert_with(|| {
                let source = sources
                    .get(node.file_path.as_str())
                    .copied()
                    .unwrap_or_default();
                let lines = crate::synthesis::comments::strip_for_regex(source, "python")
                    .split('\n')
                    .map(str::to_owned)
                    .collect::<Vec<_>>();
                let starts = statement_starts(&lines);
                File {
                    nodes: Vec::new(),
                    lines,
                    starts,
                    functions: Vec::new(),
                    innermost: Vec::new(),
                    parents: Vec::new(),
                }
            });
            files
                .get_mut(node.file_path.as_str())
                .unwrap()
                .nodes
                .push(node);
            if matches!(node.kind.as_str(), "function" | "method") {
                files
                    .get_mut(node.file_path.as_str())
                    .unwrap()
                    .functions
                    .push(node);
            }
        }
        for file in files.values_mut() {
            file.functions
                .sort_by_key(|node| (node.start_line, std::cmp::Reverse(node.end_line)));
            file.innermost.resize(file.lines.len() + 2, None);
            for (index, node) in file.functions.iter().enumerate() {
                let start = (node.start_line as usize).min(file.innermost.len());
                let end = (node.end_line as usize + 1).min(file.innermost.len());
                file.parents
                    .push(file.innermost.get(start).copied().flatten());
                if start <= end {
                    file.innermost[start..end].fill(Some(index));
                }
            }
        }
        Self {
            files,
            text: RefCell::new(HashMap::new()),
            bindings: RefCell::new(HashMap::new()),
            local_bindings: RefCell::new(HashMap::new()),
            work: Cell::new(0),
        }
    }
    fn charge(&self, count: usize) -> Result<()> {
        let total = self.work.get().saturating_add(count);
        ensure!(total <= 8_000_000, "Python scope work limit exceeded");
        self.work.set(total);
        Ok(())
    }
    pub(super) fn receiver(&self, name: &str, reference: &UnresolvedReference) -> Result<Binding> {
        if reference.language.as_deref() != Some("python")
            || !regex(r"^[A-Za-z_]\w*$")?.is_match(name)?
        {
            return Ok(Binding::Scope(Scope::Module));
        }
        let Some(file) = reference
            .file_path
            .as_deref()
            .and_then(|path| self.files.get(path))
        else {
            return Ok(Binding::Scope(Scope::Module));
        };
        if self.statement_binds_at(file, name, reference)? {
            return Ok(Binding::Scope(Scope::Local));
        }
        let mut index = file
            .innermost
            .get(reference.line as usize)
            .copied()
            .flatten();
        while let Some(at) = index {
            self.charge(1)?;
            let scope = file.functions[at];
            let key = (scope.id.as_str(), name.to_owned());
            let cached = self.bindings.borrow().get(&key).cloned();
            let binding = if let Some(cached) = cached {
                cached
            } else {
                let binding = self.scope_binding(scope, file, name)?;
                self.bindings.borrow_mut().insert(key, binding.clone());
                binding
            };
            if let Some(binding) = binding {
                return Ok(binding);
            }
            index = file.parents[at];
        }
        Ok(Binding::Scope(Scope::Module))
    }
    fn scope_binding(
        &self,
        scope: &'a Node,
        file: &File<'a>,
        name: &str,
    ) -> Result<Option<Binding>> {
        let info = self.scope_text(scope, file)?;
        for statement in &info.statements {
            self.charge(1)?;
            if statements::global(statement, name)? {
                return Ok(Some(Binding::Scope(Scope::Module)));
            }
        }
        if info
            .params
            .iter()
            .chain(&info.nested)
            .any(|parameter| parameter == name)
        {
            return Ok(Some(Binding::Scope(Scope::Local)));
        }
        let mut imported = None;
        for statement in &info.statements {
            self.charge(1)?;
            match statements::import_binding(statement, name)? {
                None if statements::binds(statement, name)? => {
                    return Ok(Some(Binding::Scope(Scope::Local)));
                }
                Some(Some(binding)) => {
                    if imported.as_ref().is_some_and(|prior| prior != &binding) {
                        return Ok(Some(Binding::Scope(Scope::Local)));
                    }
                    imported = Some(binding);
                }
                _ => {}
            }
        }
        Ok(imported)
    }
    fn scope_text(&self, scope: &'a Node, file: &File<'a>) -> Result<Rc<ScopeText>> {
        if let Some(cached) = self.text.borrow().get(scope.id.as_str()) {
            return Ok(cached.clone());
        }
        let mut out = ScopeText::default();
        let mut header = None;
        let mut current: Option<String> = None;
        let mut depth = 0usize;
        let mut continued = false;
        let mut nested_indent = None;
        let flush =
            |current: &mut Option<String>, header: &mut Option<String>, out: &mut ScopeText| {
                if let Some(text) = current.take() {
                    if header.is_none() {
                        *header = Some(text);
                    } else {
                        out.statements.extend(
                            text.split(';')
                                .map(trim)
                                .filter(|text| !text.is_empty())
                                .map(str::to_owned),
                        );
                    }
                }
            };
        for line in scope.start_line as usize..=(scope.end_line as usize).min(file.lines.len()) {
            self.charge(1)?;
            let raw = &file.lines[line - 1];
            let blanked = statements::blank_strings(raw);
            let inside = depth > 0 || continued;
            for byte in blanked.bytes() {
                match byte {
                    b'(' | b'[' | b'{' => depth += 1,
                    b')' | b']' | b'}' => depth = depth.saturating_sub(1),
                    _ => {}
                }
            }
            continued = trim(&blanked).ends_with('\\');
            if inside {
                if let Some(current) = current.as_mut() {
                    current.push(' ');
                    current.push_str(trim(&blanked).strip_suffix('\\').unwrap_or(trim(&blanked)));
                }
                continue;
            }
            if trim(&blanked).is_empty() {
                continue;
            }
            flush(&mut current, &mut header, &mut out);
            let indentation = raw
                .chars()
                .take_while(|ch| crate::search::query_whitespace(*ch))
                .map(char::len_utf16)
                .sum::<usize>();
            if let Some(indent) = nested_indent {
                if indentation > indent {
                    continue;
                }
                nested_indent = None;
            }
            let text = trim(&blanked).strip_suffix('\\').unwrap_or(trim(&blanked));
            if line != scope.start_line as usize
                && let Some(capture) =
                    regex(r"^(?:async\s+)?(?:def|class)\s+(\w+)")?.captures(text)?
            {
                out.nested.push(capture[1].into());
                nested_indent = Some(indentation);
                continue;
            }
            current = Some(text.into());
        }
        flush(&mut current, &mut header, &mut out);
        let header = header.unwrap_or_default();
        if let Some(found) = regex(r"\bdef\s+\w+\s*(?:\[[^\]]*\]\s*)?\(")?.find(&header)? {
            out.params = statements::params(
                &header[found.end()..statements::closing(&header, found.end() - 1)],
            )?;
        }
        let out = Rc::new(out);
        self.text
            .borrow_mut()
            .insert(scope.id.as_str(), out.clone());
        Ok(out)
    }
    fn statement_binds_at(
        &self,
        file: &File<'_>,
        name: &str,
        reference: &UnresolvedReference,
    ) -> Result<bool> {
        let Some(at) = reference
            .line
            .checked_sub(1)
            .map(|line| line as usize)
            .filter(|&line| line < file.lines.len())
        else {
            return Ok(false);
        };
        let mut first = at;
        while first > 0 && !file.starts[first] {
            first -= 1;
        }
        let mut end = at;
        while end + 1 < file.lines.len() && !file.starts[end + 1] {
            end += 1;
        }
        self.charge(end - first + 1)?;
        let mut text = String::new();
        let mut offset = 0;
        for line in first..=end {
            let blanked = statements::blank_strings(&file.lines[line]);
            if line == at {
                offset = text.len() + byte_column(&blanked, reference.column);
            }
            text.push_str(&blanked);
            text.push('\n');
        }
        if !regex(r"\b(?:lambda|for)\b")?.is_match(&text)? {
            return Ok(false);
        }
        for found in regex(r"\blambda\b")?.find_iter(&text) {
            let from = found?.end();
            let mut depth = 0usize;
            let mut colon = None;
            for (at, byte) in text.bytes().enumerate().skip(from) {
                match byte {
                    b'(' | b'[' | b'{' => depth += 1,
                    b')' | b']' | b'}' => {
                        if depth == 0 {
                            break;
                        }
                        depth -= 1;
                    }
                    b':' if depth == 0 => {
                        colon = Some(at);
                        break;
                    }
                    _ => {}
                }
            }
            let Some(colon) = colon else {
                continue;
            };
            if offset <= colon
                || !statements::params(&text[from..colon])?
                    .iter()
                    .any(|parameter| parameter == name)
            {
                continue;
            }
            let mut stop = text.len();
            depth = 0;
            for (at, byte) in text.bytes().enumerate().skip(colon + 1) {
                match byte {
                    b'(' | b'[' | b'{' => depth += 1,
                    b')' | b']' | b'}' => {
                        if depth == 0 {
                            stop = at;
                            break;
                        }
                        depth -= 1;
                    }
                    b',' if depth == 0 => {
                        stop = at;
                        break;
                    }
                    _ => {}
                }
            }
            if offset < stop {
                return Ok(true);
            }
        }
        let mut opens = Vec::new();
        let mut pairs = Vec::new();
        for (at, byte) in text.bytes().enumerate() {
            match byte {
                b'(' | b'[' | b'{' => opens.push(at),
                b')' | b']' | b'}' => {
                    if let Some(open) = opens.pop() {
                        pairs.push((open, at));
                    }
                }
                _ => {}
            }
        }
        pairs.extend(opens.into_iter().map(|open| (open, text.len())));
        for found in regex(r"\bfor\b")?.find_iter(&text) {
            let found = found?;
            let around = pairs
                .iter()
                .filter(|(open, close)| *open < found.start() && *close > found.start())
                .max_by_key(|(open, _)| open);
            let Some(&(open, close)) = around else {
                continue;
            };
            if offset <= open || offset >= close {
                continue;
            }
            if let Some(capture) =
                regex(r"^\s*([\s\S]+?)\s+in\b")?.captures(&text[found.end()..close])?
                && statements::target_binds(&capture[1], name)?
            {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

fn byte_column(text: &str, column: u32) -> usize {
    let mut units = 0;
    for (at, ch) in text.char_indices() {
        if units >= column {
            return at;
        }
        units += ch.len_utf16() as u32;
    }
    text.len()
}

fn statement_starts(lines: &[String]) -> Vec<bool> {
    let mut out = Vec::with_capacity(lines.len());
    let mut depth = 0usize;
    let mut continued = false;
    for line in lines {
        out.push(depth == 0 && !continued);
        let mut quote = None;
        let bytes = line.as_bytes();
        let mut at = 0;
        while at < bytes.len() {
            let byte = bytes[at];
            if let Some(active) = quote {
                if byte == b'\\' {
                    at += 1;
                } else if byte == active {
                    quote = None;
                }
            } else {
                match byte {
                    b'\'' | b'"' => quote = Some(byte),
                    b'(' | b'[' | b'{' => depth += 1,
                    b')' | b']' | b'}' => depth = depth.saturating_sub(1),
                    _ => {}
                }
            }
            at += 1;
        }
        continued = trim(line).ends_with('\\');
    }
    out
}
