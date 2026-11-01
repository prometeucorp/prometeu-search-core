use super::*;
use crate::frameworks::python_resolution::Answer;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(super) struct Mapping {
    pub local_name: String,
    pub exported_name: String,
    pub source: String,
    pub is_default: bool,
    pub is_namespace: bool,
}
struct FromImport {
    source: String,
    names: Vec<(String, String)>,
    star: bool,
}
fn from_imports(code: &str) -> Result<Vec<FromImport>> {
    let mut out = Vec::new();
    for capture in regex(
        r"(?m)^[ \t]*from[ \t]+(\.*[\w.]*)[ \t]+import[ \t]*(\([^)]*\)|(?:[^\n\\;]|\\\r?\n)+)",
    )?
    .captures_iter(code)
    {
        let capture = capture?;
        if capture[1].is_empty() {
            continue;
        }
        let list = capture[2].strip_prefix('(').unwrap_or(&capture[2]);
        let list = list.strip_suffix(')').unwrap_or(list);
        let list = regex(r"\\\r?\n")?.try_replacen(list, 0, " ")?;
        let mut names = Vec::new();
        let mut star = false;
        for item in list.split(',').map(trim) {
            if item == "*" {
                star = true;
                continue;
            }
            if let Some(spec) = regex(r"^(\w+)(?:\s+as\s+(\w+))?$")?.captures(item)? {
                names.push((
                    spec[1].to_owned(),
                    spec.get(2).unwrap_or(spec.get(1).unwrap()).as_str().into(),
                ));
            }
        }
        out.push(FromImport {
            source: capture[1].into(),
            names,
            star,
        });
    }
    Ok(out)
}
pub(super) fn mappings(source: &str) -> Result<Vec<Mapping>> {
    let code = crate::synthesis::comments::strip_for_regex(source, "python");
    let mut out = Vec::new();
    for from in from_imports(&code)? {
        for (name, alias) in from.names {
            out.push(Mapping {
                local_name: alias,
                exported_name: name,
                source: from.source.clone(),
                is_default: false,
                is_namespace: false,
            });
        }
    }
    for capture in regex(r"(?m)^import[ \t]+((?:[^\n\\;]|\\\r?\n)+)")?.captures_iter(&code) {
        let capture = capture?;
        let list = regex(r"\\\r?\n")?.try_replacen(&capture[1], 0, " ")?;
        for item in list.split(',').map(trim) {
            if let Some(spec) = regex(r"^([\w.]+)(?:\s+as\s+(\w+))?$")?.captures(item)? {
                out.push(Mapping {
                    local_name: spec
                        .get(2)
                        .map_or_else(
                            || spec[1].rsplit('.').next().unwrap_or_default(),
                            |alias| alias.as_str(),
                        )
                        .into(),
                    exported_name: "*".into(),
                    source: spec[1].into(),
                    is_default: false,
                    is_namespace: true,
                });
            }
        }
    }
    Ok(out)
}

pub(super) struct Imports<'a, 'env> {
    nodes: &'a [Node],
    files: HashMap<&'a str, Vec<&'a Node>>,
    mappings: HashMap<&'a str, Vec<Mapping>>,
    reexports: HashMap<&'a str, Vec<FromImport>>,
    sources: &'env BTreeMap<&'a str, &'a str>,
    module: &'env dyn Fn(&str, &str) -> Option<String>,
    scopes: &'env scope::Scopes<'a>,
    supertypes: &'env dyn Fn(&str) -> Vec<&'a Node>,
    exports: RefCell<HashMap<(String, String), Option<&'a Node>>>,
    stars: RefCell<HashMap<String, Option<HashSet<String>>>>,
    work: Cell<usize>,
}
impl<'a, 'env> Imports<'a, 'env> {
    pub(super) fn new(
        nodes: &'a [Node],
        sources: &'env BTreeMap<&'a str, &'a str>,
        module: &'env dyn Fn(&str, &str) -> Option<String>,
        scopes: &'env scope::Scopes<'a>,
        supertypes: &'env dyn Fn(&str) -> Vec<&'a Node>,
    ) -> Result<Self> {
        let mut files: HashMap<&str, Vec<&Node>> = HashMap::new();
        for node in nodes {
            files.entry(&node.file_path).or_default().push(node);
        }
        let mut parsed = HashMap::new();
        let mut reexports = HashMap::new();
        for (&path, &source) in sources {
            if !files
                .get(path)
                .is_some_and(|nodes| nodes.iter().any(|node| node.language == "python"))
            {
                continue;
            }
            parsed.insert(path, mappings(source)?);
            reexports.insert(
                path,
                from_imports(&crate::synthesis::comments::strip_for_regex(
                    source, "python",
                ))?,
            );
        }
        Ok(Self {
            nodes,
            files,
            mappings: parsed,
            reexports,
            sources,
            module,
            scopes,
            supertypes,
            exports: RefCell::new(HashMap::new()),
            stars: RefCell::new(HashMap::new()),
            work: Cell::new(0),
        })
    }
    fn charge(&self, count: usize) -> Result<()> {
        let work = self.work.get().saturating_add(count);
        ensure!(
            work <= 8_000_000,
            "Python import resolution work limit exceeded"
        );
        self.work.set(work);
        Ok(())
    }
    fn nodes(&self, path: &str) -> &[&'a Node] {
        self.files.get(path).map(Vec::as_slice).unwrap_or_default()
    }
    fn export(&self, path: &str, name: &str) -> Result<Option<&'a Node>> {
        let key = (path.to_owned(), name.to_owned());
        if let Some(cached) = self.exports.borrow().get(&key) {
            return Ok(*cached);
        }
        let found = self.export_walk(path, name, &mut HashSet::new(), 0)?;
        self.exports.borrow_mut().insert(key, found);
        Ok(found)
    }
    fn export_walk(
        &self,
        path: &str,
        name: &str,
        visited: &mut HashSet<String>,
        depth: usize,
    ) -> Result<Option<&'a Node>> {
        if depth > 8 || !visited.insert(path.into()) {
            return Ok(None);
        }
        for &node in self.nodes(path) {
            self.charge(1)?;
            if node.name == name
                && (node.is_exported == Some(true)
                    || path.ends_with(".py")
                        && matches!(
                            node.kind.as_str(),
                            "function" | "class" | "variable" | "constant"
                        )
                        && !node.qualified_name.contains("::"))
            {
                return Ok(Some(node));
            }
        }
        let exports = self
            .reexports
            .get(path)
            .map(Vec::as_slice)
            .unwrap_or_default();
        for from in exports {
            for (original, alias) in &from.names {
                if alias == name
                    && let Some(next) = (self.module)(&from.source, path)
                    && let Some(found) = self.export_walk(&next, original, visited, depth + 1)?
                {
                    return Ok(Some(found));
                }
            }
        }
        for from in exports {
            if from.star
                && let Some(next) = (self.module)(&from.source, path)
                && (!next.ends_with(".py") || self.star_allows(&next, name)?)
                && let Some(found) = self.export_walk(&next, name, visited, depth + 1)?
            {
                return Ok(Some(found));
            }
        }
        Ok(None)
    }
    fn star_allows(&self, path: &str, name: &str) -> Result<bool> {
        if let Some(cached) = self.stars.borrow().get(path) {
            return Ok(cached
                .as_ref()
                .map_or_else(|| !name.starts_with('_'), |names| names.contains(name)));
        }
        let source = self.sources.get(path).copied().unwrap_or_default();
        let mut names = None;
        if source.contains("__all__") {
            let code = crate::synthesis::comments::strip_for_regex(source, "python");
            let mut literal = true;
            let mut collected = HashSet::new();
            for capture in regex(r"(?m)^[ \t]*__all__\b[ \t]*(\.|\+=|=|:[^=\n]*=)?[ \t]*(.?)")?
                .captures_iter(&code)
            {
                let capture = capture?;
                let found = capture.get(0).unwrap();
                let open = found.end().saturating_sub(1);
                let close = match &capture[2] {
                    "[" => code[open..].find(']'),
                    "(" => code[open..].find(')'),
                    _ => None,
                }
                .map(|offset| offset + open);
                let assigns = capture
                    .get(1)
                    .is_some_and(|operation| operation.as_str() != ".");
                let tail = close
                    .map(|close| code[close + 1..].split('\n').next().unwrap_or_default())
                    .unwrap_or_default();
                if !assigns || close.is_none() || !trim(tail).is_empty() {
                    literal = false;
                    break;
                }
                let a = utf16_byte(source, code[..open].encode_utf16().count());
                let b = utf16_byte(source, code[..close.unwrap()].encode_utf16().count());
                for capture in regex(r#"["']([A-Za-z_]\w*)["']"#)?.captures_iter(&source[a..b]) {
                    collected.insert(capture?[1].to_owned());
                }
            }
            if literal && !collected.is_empty() {
                names = Some(collected);
            }
        }
        let allows = names
            .as_ref()
            .map_or_else(|| !name.starts_with('_'), |names| names.contains(name));
        self.stars.borrow_mut().insert(path.into(), names);
        Ok(allows)
    }
    fn package_binds(&self, mapping: &Mapping, from: &str) -> Result<bool> {
        if mapping.is_namespace || mapping.exported_name == "*" {
            return Ok(false);
        }
        let Some(path) = (self.module)(&mapping.source, from)
            .filter(|path| path == "__init__.py" || path.ends_with("/__init__.py"))
        else {
            return Ok(false);
        };
        Ok(self.export(&path, &mapping.exported_name)?.is_some())
    }
    pub(super) fn scoped(&self, reference: &UnresolvedReference) -> Result<Vec<&Mapping>> {
        let path = reference.file_path.as_deref().unwrap_or_default();
        let imports = self
            .mappings
            .get(path)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let Some((receiver, _)) = reference
            .reference_name
            .split_once('.')
            .filter(|(receiver, _)| !receiver.is_empty())
        else {
            return Ok(imports.iter().collect());
        };
        if reference.reference_kind == "imports" {
            return Ok(imports.iter().collect());
        }
        let binds = |mapping: &&Mapping| {
            mapping.local_name == receiver
                || mapping.is_namespace
                    && mapping.source.split('.').next() == Some(receiver)
                    && mapping.source.rsplit('.').next() == Some(mapping.local_name.as_str())
        };
        if !imports.iter().any(|mapping| binds(&mapping)) {
            return Ok(imports.iter().collect());
        }
        let binding = self.scopes.receiver(receiver, reference)?;
        if binding == scope::Binding::Scope(scope::Scope::Module) {
            let key = |mapping: &Mapping| {
                format!(
                    "{}:{}",
                    mapping.source,
                    if mapping.is_namespace {
                        "*"
                    } else {
                        &mapping.exported_name
                    }
                )
            };
            if imports
                .iter()
                .filter(binds)
                .map(key)
                .collect::<HashSet<_>>()
                .len()
                < 2
            {
                return Ok(imports.iter().collect());
            }
            let module = self.scopes.module_import_keys(receiver, path)?;
            let kept = imports
                .iter()
                .filter(|mapping| {
                    !binds(mapping) || mapping.is_namespace || module.contains(&key(mapping))
                })
                .collect::<Vec<_>>();
            return Ok(if kept.iter().copied().any(|mapping| binds(&mapping)) {
                kept
            } else {
                imports.iter().collect()
            });
        }
        Ok(imports
            .iter()
            .filter(|mapping| {
                if !binds(mapping) {
                    return true;
                }
                match &binding {
                    scope::Binding::Scope(_) => false,
                    scope::Binding::ImportedModule { module } => {
                        mapping.is_namespace
                            && (if mapping.local_name == receiver {
                                mapping.source.as_str()
                            } else {
                                receiver
                            }) == module
                    }
                    scope::Binding::ImportedName {
                        source,
                        exported_name,
                    } => {
                        !mapping.is_namespace
                            && &mapping.source == source
                            && &mapping.exported_name == exported_name
                    }
                }
            })
            .collect())
    }
    pub(super) fn resolve(&self, reference: &UnresolvedReference) -> Result<Option<Answer<'a>>> {
        let path = reference.file_path.as_deref().unwrap_or_default();
        let name = &reference.reference_name;
        let imports = self.scoped(reference)?;
        if let Some(target) = self.module_member(reference, &imports)? {
            return Ok(Some(answer(target, 0.85, "import")));
        }
        if reference.reference_kind == "imports"
            && (name.contains('.') || self.is_from_module(reference)?)
            && let Some(file) = (self.module)(name, path).filter(|file| file != path)
            && let Some(&node) = self.nodes(&file).iter().find(|node| node.kind == "file")
        {
            return Ok(Some(answer(node, 0.9, "import")));
        }
        if reference.reference_kind == "imports" && !name.contains('.') {
            for mapping in &imports {
                if &mapping.local_name != name {
                    continue;
                }
                let module = if mapping.is_namespace {
                    mapping.source.clone()
                } else {
                    if self.package_binds(mapping, path)? {
                        continue;
                    }
                    submodule(mapping)
                };
                if let Some(file) = (self.module)(&module, path).filter(|file| file != path)
                    && let Some(&node) = self.nodes(&file).iter().find(|node| node.kind == "file")
                {
                    return Ok(Some(answer(node, 0.9, "import")));
                }
            }
        }
        for mapping in imports {
            if mapping.local_name != *name
                && !name
                    .strip_prefix(&mapping.local_name)
                    .is_some_and(|tail| tail.starts_with('.'))
            {
                continue;
            }
            let Some(file) = (self.module)(&mapping.source, path) else {
                continue;
            };
            let exported = if mapping.is_namespace {
                name.strip_prefix(&format!("{}.", mapping.local_name))
                    .unwrap_or(name)
            } else {
                &mapping.exported_name
            };
            if let Some(node) = self.export(&file, exported)? {
                if !mapping.is_namespace
                    && let Some(member) = name.strip_prefix(&format!("{}.", mapping.local_name))
                {
                    if let Some(target) =
                        self.static_member(node, member, &reference.reference_kind)
                    {
                        return Ok(Some(answer(target, 0.9, "import")));
                    }
                    return self.instance_member(node, member, reference);
                }
                return Ok(Some(answer(node, 0.9, "import")));
            }
        }
        Ok(None)
    }
    fn instance_member(
        &self,
        value: &'a Node,
        member: &str,
        reference: &UnresolvedReference,
    ) -> Result<Option<Answer<'a>>> {
        if reference.reference_kind != "calls"
            || !matches!(value.kind.as_str(), "constant" | "variable")
            || member.is_empty()
            || member.contains('.')
        {
            return Ok(None);
        }
        let Some(source) = self.sources.get(value.file_path.as_str()) else {
            return Ok(None);
        };
        let declaration = source
            .split('\n')
            .skip(value.start_line.saturating_sub(1) as usize)
            .take((value.end_line - value.start_line + 1) as usize)
            .collect::<Vec<_>>()
            .join("\n");
        let receiver = regex::escape(&value.name);
        for pattern in [
            format!(
                r"(?:^|;)\s*{receiver}\s*=(?!=)\s*([A-Z]\w*)\.objects\.(?:create|get|first|last|latest|earliest|get_by_natural_key)\s*\("
            ),
            format!(r"(?:^|;)\s*{receiver}\s*=(?!=)\s*([A-Z][\w.]*)\s*\((?![^\n]*,\s*$)"),
            format!(r#"\b{receiver}\b\s*:\s*["']([A-Z][\w.]*)["']"#),
            format!(r"\b{receiver}\b\s*:\s*([A-Z][\w.]*)"),
        ] {
            if let Some(capture) = regex(&pattern)?.captures(&declaration)? {
                let name = capture[1].rsplit('.').next().unwrap_or_default();
                if matches!(name, "None" | "True" | "False") {
                    continue;
                }
                if let Some(node) = self.method_on_type(name, member, reference, 0)? {
                    return Ok(Some(answer(node, 0.85, "instance-method")));
                }
            }
        }
        Ok(None)
    }
    fn method_on_type(
        &self,
        name: &str,
        member: &str,
        reference: &UnresolvedReference,
        depth: usize,
    ) -> Result<Option<&'a Node>> {
        self.charge(self.nodes.len())?;
        let qualified = format!("{name}::{member}");
        let suffix = format!("::{qualified}");
        let matches = self
            .nodes
            .iter()
            .filter(|node| {
                node.language == "python"
                    && node.kind == "method"
                    && node.name == member
                    && (node.qualified_name == qualified || node.qualified_name.ends_with(&suffix))
            })
            .collect::<Vec<_>>();
        if !matches.is_empty() {
            return Ok(matches
                .iter()
                .copied()
                .find(|node| reference.file_path.as_deref() == Some(node.file_path.as_str()))
                .or_else(|| matches.first().copied()));
        }
        if depth < 4 {
            for declaration in self
                .nodes
                .iter()
                .filter(|node| node.language == "python" && node.name == name)
            {
                for parent in (self.supertypes)(&declaration.id) {
                    if let Some(node) =
                        self.method_on_type(&parent.name, member, reference, depth + 1)?
                    {
                        return Ok(Some(node));
                    }
                }
            }
        }
        Ok(None)
    }
    fn static_member(&self, container: &'a Node, member: &str, kind: &str) -> Option<&'a Node> {
        if !matches!(
            container.kind.as_str(),
            "class" | "struct" | "union" | "interface" | "enum" | "trait" | "protocol"
        ) {
            return None;
        }
        let (head, rest) = member
            .split_once('.')
            .map_or((member, None), |(head, rest)| (head, Some(rest)));
        let qualified = format!("{}::{head}", container.qualified_name);
        let candidates = self
            .nodes(&container.file_path)
            .iter()
            .copied()
            .filter(|node| node.qualified_name == qualified)
            .collect::<Vec<_>>();
        if let Some(rest) = rest {
            return candidates
                .into_iter()
                .find_map(|node| self.static_member(node, rest, kind));
        }
        if kind == "calls"
            && let Some(node) = candidates
                .iter()
                .copied()
                .find(|node| matches!(node.kind.as_str(), "method" | "function"))
        {
            return Some(node);
        }
        candidates.first().copied()
    }
    fn module_member(
        &self,
        reference: &UnresolvedReference,
        imports: &[&Mapping],
    ) -> Result<Option<&'a Node>> {
        let name = &reference.reference_name;
        let path = reference.file_path.as_deref().unwrap_or_default();
        let Some((receiver, suffix)) = name
            .split_once('.')
            .filter(|(head, tail)| !head.is_empty() && !tail.is_empty())
        else {
            return Ok(None);
        };
        let member = suffix.split('.').next().unwrap();
        if let Some(local) = self.scopes.function_module_alias(receiver, reference)? {
            return if let Some(file) = (self.module)(&local, path).filter(|file| file != path) {
                self.export(&file, member)
            } else {
                Ok(None)
            };
        }
        for mapping in imports {
            let spelled = mapping.is_namespace
                && mapping.source.rsplit('.').next() == Some(mapping.local_name.as_str())
                && mapping.source.contains('.')
                && name.starts_with(&format!("{}.", mapping.source));
            if mapping.local_name != receiver && !spelled {
                continue;
            }
            let mut module = if mapping.is_namespace {
                mapping.source.clone()
            } else {
                submodule(mapping)
            };
            let suffix = if spelled {
                &name[mapping.source.len() + 1..]
            } else {
                suffix
            };
            let mut rest = suffix.split('.').collect::<Vec<_>>();
            if !mapping.is_namespace && self.package_binds(mapping, path)? {
                continue;
            }
            let Some(mut file) = (self.module)(&module, path).filter(|file| file != path) else {
                continue;
            };
            if reference.reference_kind == "calls" && rest.len() > 1 {
                while rest.len() > 1 {
                    let next = format!("{module}.{}", rest[0]);
                    let Some(next_file) = (self.module)(&next, path) else {
                        break;
                    };
                    module = next;
                    file = next_file;
                    rest.remove(0);
                }
                let Some(head) = self.export(&file, rest[0])? else {
                    continue;
                };
                if rest.len() == 1 && matches!(head.kind.as_str(), "function" | "class") {
                    return Ok(Some(head));
                }
                if rest.len() == 2
                    && head.kind == "class"
                    && let Some(&node) = self.nodes(&head.file_path).iter().find(|node| {
                        node.kind == "method"
                            && node.name == rest[1]
                            && node.qualified_name
                                == format!("{}::{}", head.qualified_name, rest[1])
                    })
                {
                    return Ok(Some(node));
                }
            } else if let Some(node) = self.export(&file, rest[0])? {
                return Ok(Some(node));
            }
        }
        Ok(None)
    }
    fn is_from_module(&self, reference: &UnresolvedReference) -> Result<bool> {
        let lines = self
            .sources
            .get(reference.file_path.as_deref().unwrap_or_default())
            .copied()
            .unwrap_or_default()
            .split('\n')
            .map(|line| line.strip_suffix('\r').unwrap_or(line))
            .collect::<Vec<_>>();
        let Some(line) = reference
            .line
            .checked_sub(1)
            .and_then(|at| lines.get(at as usize))
        else {
            return Ok(false);
        };
        let start = utf16_byte(line, reference.column as usize);
        if !line[start..].starts_with("from") {
            return Ok(false);
        }
        let mut text = line[start + 4..].to_owned();
        for line in lines.iter().skip(reference.line as usize).take(3) {
            if !trim(&text).ends_with('\\') {
                break;
            }
            text = regex(r"\\\s*$")?.try_replacen(&text, 1, " ")?.into_owned();
            text.push_str(line);
        }
        let name = text.trim_start_matches(crate::search::query_whitespace);
        Ok(name.len() < text.len()
            && name.starts_with(&reference.reference_name)
            && regex(r"^\s+import\b")?.is_match(&name[reference.reference_name.len()..])?)
    }
}
fn submodule(mapping: &Mapping) -> String {
    let name = if mapping.exported_name == "*" {
        &mapping.local_name
    } else {
        &mapping.exported_name
    };
    format!(
        "{}{}{name}",
        mapping.source,
        if mapping.source.ends_with('.') {
            ""
        } else {
            "."
        }
    )
}
fn answer<'a>(node: &'a Node, confidence: f64, resolved_by: &'static str) -> Answer<'a> {
    Answer {
        node,
        confidence,
        resolved_by,
        framework: None,
    }
}
fn utf16_byte(text: &str, column: usize) -> usize {
    let mut units = 0;
    for (at, ch) in text.char_indices() {
        if units >= column {
            return at;
        }
        units += ch.len_utf16();
    }
    text.len()
}
