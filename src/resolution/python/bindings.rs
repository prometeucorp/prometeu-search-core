use super::*;
use std::collections::BTreeSet;

#[derive(Clone, PartialEq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub(in super::super) enum LineBinding {
    Global,
    Import {
        key: String,
    },
    Assign {
        #[serde(rename = "type")]
        ty: Option<String>,
        value: String,
        line: usize,
    },
    Other,
}

#[derive(serde::Serialize)]
pub(in super::super) struct LocalBindings {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub param: Option<Option<String>>,
    pub bindings: Vec<LineBinding>,
}

impl<'a> Scopes<'a> {
    pub(in super::super) fn module_statements(&self, path: &str) -> Result<Vec<(String, bool)>> {
        let Some(file) = self.files.get(path) else {
            return Ok(Vec::new());
        };
        let module = own_lines(None, file).into_iter().collect::<BTreeSet<_>>();
        let mut out = Vec::new();
        let mut script = false;
        for (at, line) in file.lines.iter().enumerate() {
            self.charge(1)?;
            if regex(r#"^if\s+(?:__name__\s*==\s*(['"])__main__\1|(['"])__main__\2\s*==\s*__name__)\s*:"#)?.is_match(line)? {script=true;continue;}
            if script && regex(r"^\S")?.is_match(line)? {
                script = false;
            }
            if script || !file.starts[at] {
                continue;
            }
            let mut statement = line.clone();
            for (next, line) in file.lines.iter().enumerate().skip(at + 1) {
                if file.starts[next] {
                    break;
                }
                statement.push('\n');
                statement.push_str(line);
            }
            out.push((statement, module.contains(&at)));
        }
        Ok(out)
    }
    pub(in super::super) fn scope_bindings(
        &self,
        scope: &'a Node,
        name: &str,
    ) -> Result<Rc<LocalBindings>> {
        self.local_bindings(scope, &self.files[scope.file_path.as_str()], name)
    }
    pub(in super::super) fn locally_bound(
        &self,
        name: &str,
        reference: &UnresolvedReference,
    ) -> Result<bool> {
        let Some(file) = reference
            .file_path
            .as_deref()
            .and_then(|path| self.files.get(path))
        else {
            return Ok(false);
        };
        self.binds_locally(name, reference, file, false)
    }
    pub(in super::super) fn all_import_keys(
        &self,
        name: &str,
        path: &str,
    ) -> Result<BTreeSet<String>> {
        let mut keys = BTreeSet::new();
        if let Some(file) = self.files.get(path) {
            for (at, line) in file.lines.iter().enumerate() {
                self.charge(1)?;
                if file.starts[at] && regex(r"\bimport\b")?.is_match(line)? {
                    for binding in line_bindings(file, at, name)? {
                        if let LineBinding::Import { key } = binding {
                            keys.insert(key);
                        }
                    }
                }
            }
        }
        Ok(keys)
    }
    pub(in super::super) fn global_rebound(&self, name: &str, path: &str) -> Result<bool> {
        Ok(self
            .global_bindings(name, path)?
            .iter()
            .any(|binding| !matches!(binding, LineBinding::Import { .. })))
    }
    pub(in super::super) fn binding_scope(
        &self,
        name: &str,
        reference: &UnresolvedReference,
    ) -> Result<Option<(&'a Node, Rc<LocalBindings>)>> {
        let Some(file) = reference
            .file_path
            .as_deref()
            .and_then(|path| self.files.get(path))
        else {
            return Ok(None);
        };
        let mut definitions = file.functions.to_vec();
        definitions.sort_by_key(|node| std::cmp::Reverse(node.start_line));
        for scope in definitions {
            if scope.start_line > reference.line || scope.end_line < reference.line {
                continue;
            }
            let bindings = self.local_bindings(scope, file, name)?;
            if bindings.param.is_some() {
                return Ok(Some((scope, bindings)));
            }
            if bindings.bindings.contains(&LineBinding::Global) {
                return Ok(None);
            }
            if !bindings.bindings.is_empty() {
                return Ok(Some((scope, bindings)));
            }
        }
        Ok(None)
    }
    fn local_bindings(
        &self,
        scope: &'a Node,
        file: &File<'a>,
        name: &str,
    ) -> Result<Rc<LocalBindings>> {
        let key = (scope.id.as_str(), name.to_owned());
        if let Some(cached) = self.local_bindings.borrow().get(&key) {
            return Ok(cached.clone());
        }
        let escaped = regex::escape(name);
        let word = regex(&format!(r"(?<![\w.]){escaped}\b(?!\s*[.\[])"))?;
        let mention = regex(&format!(r"(?<![\w.]){escaped}\b"))?;
        let mut bindings = Vec::new();
        for at in own_lines(Some(scope), file) {
            self.charge(1)?;
            let pattern = if regex(r"^\s*(?:from|import)\b")?.is_match(&file.lines[at])? {
                &mention
            } else {
                &word
            };
            let mut names = pattern.is_match(&file.lines[at])?;
            if !names && file.starts[at] {
                for next in at + 1..file.lines.len() {
                    if file.starts[next] {
                        break;
                    }
                    if pattern.is_match(&file.lines[next])? {
                        names = true;
                        break;
                    }
                }
            }
            if names {
                bindings.extend(line_bindings(file, at, name)?);
            }
        }
        let bindings = Rc::new(LocalBindings {
            param: param_annotation(scope.signature.as_deref().unwrap_or_default(), name)?,
            bindings,
        });
        self.local_bindings
            .borrow_mut()
            .insert(key, bindings.clone());
        Ok(bindings)
    }
}

fn param_annotation(signature: &str, name: &str) -> Result<Option<Option<String>>> {
    if !signature.contains(name) {
        return Ok(None);
    }
    let original = signature.encode_utf16().collect::<Vec<_>>();
    let Some(open) = original.iter().position(|&unit| unit == u16::from(b'(')) else {
        return Ok(None);
    };
    let blank = statements::blank_strings(signature)
        .encode_utf16()
        .collect::<Vec<_>>();
    let mut spans = Vec::new();
    let mut depth = 0i32;
    let mut from = open + 1;
    for (at, &unit) in blank.iter().enumerate().skip(open) {
        if matches!(unit, 40 | 91 | 123) {
            depth += 1;
        } else if matches!(unit, 41 | 93 | 125) {
            depth -= 1;
            if depth == 0 {
                spans.push((from, at));
                break;
            }
        } else if unit == 44 && depth == 1 {
            spans.push((from, at));
            from = at + 1;
        }
    }
    for (start, end) in spans {
        let text = String::from_utf16_lossy(&original[start..end]);
        let Some(head) = regex(r"^\s*(\*{0,2})([A-Za-z_]\w*)\s*(:?)")?.captures(&text)? else {
            continue;
        };
        if &head[2] != name {
            continue;
        }
        if !head[1].is_empty() || head[3].is_empty() {
            return Ok(Some(None));
        }
        let at = start + head[0].encode_utf16().count();
        let mut to = end;
        let mut depth = 0i32;
        for (index, &unit) in blank.iter().enumerate().take(end).skip(at) {
            if matches!(unit, 40 | 91 | 123) {
                depth += 1;
            } else if matches!(unit, 41 | 93 | 125) {
                depth -= 1;
            } else if unit == 61 && depth == 0 {
                to = index;
                break;
            }
        }
        let annotation = String::from_utf16_lossy(&original[at..to]);
        let annotation = trim(&annotation);
        return Ok(Some(
            (!annotation.is_empty()).then(|| annotation.to_owned()),
        ));
    }
    Ok(None)
}

impl Scopes<'_> {
    pub(in super::super) fn function_module_alias(
        &self,
        name: &str,
        reference: &UnresolvedReference,
    ) -> Result<Option<String>> {
        if reference.language.as_deref() != Some("python") || !regex(r"^\w+$")?.is_match(name)? {
            return Ok(None);
        }
        let Some(file) = reference
            .file_path
            .as_deref()
            .and_then(|path| self.files.get(path))
        else {
            return Ok(None);
        };
        let mut scopes = file
            .functions
            .iter()
            .copied()
            .filter(|scope| scope.start_line <= reference.line && scope.end_line >= reference.line)
            .collect::<Vec<_>>();
        scopes.sort_by_key(|scope| std::cmp::Reverse(scope.start_line));
        for scope in scopes {
            let bindings = self.own_bindings(Some(scope), file, name)?;
            if bindings.is_empty() {
                continue;
            }
            let modules = bindings
                .iter()
                .map(|binding| match binding {
                    LineBinding::Import { key } => key.strip_suffix(":*").unwrap_or_default(),
                    _ => "",
                })
                .collect::<BTreeSet<_>>();
            return Ok(
                if modules.len() == 1
                    && let Some(module) = modules.first()
                    && module.contains('.')
                    && !self.binds_locally(name, reference, file, false)?
                {
                    Some((*module).to_owned())
                } else {
                    None
                },
            );
        }
        Ok(None)
    }
    pub(in super::super) fn module_import_keys(
        &self,
        name: &str,
        path: &str,
    ) -> Result<BTreeSet<String>> {
        Ok(self
            .global_bindings(name, path)?
            .into_iter()
            .filter_map(|binding| match binding {
                LineBinding::Import { key } => Some(key),
                _ => None,
            })
            .collect())
    }
    pub(in super::super) fn global_bindings(
        &self,
        name: &str,
        path: &str,
    ) -> Result<Vec<LineBinding>> {
        let Some(file) = self.files.get(path) else {
            return Ok(Vec::new());
        };
        let mut regions = vec![own_lines(None, file)];
        for (at, line) in file.lines.iter().enumerate() {
            self.charge(1)?;
            if !regex(r"^\s*global\b")?.is_match(line)? || line_bindings(file, at, name)?.is_empty()
            {
                continue;
            }
            let scope = file
                .nodes
                .iter()
                .copied()
                .filter(|node| {
                    matches!(node.kind.as_str(), "class" | "function" | "method")
                        && node.start_line as usize <= at + 1
                        && node.end_line as usize > at
                })
                .max_by_key(|node| node.start_line);
            if let Some(scope) = scope
                && scope.kind != "class"
            {
                regions.push(own_lines(Some(scope), file));
            }
        }
        let mut script = BTreeSet::new();
        for (at, line) in file.lines.iter().enumerate() {
            if let Some(found) = regex(r#"^if\s+(?:__name__\s*==\s*(['"])__main__\1|(['"])__main__\2\s*==\s*__name__)\s*:"#)?.find(line)? {
                if !trim(&line[found.end()..]).is_empty() { script.insert(at); }
                for (next, line) in file.lines.iter().enumerate().skip(at + 1) {
                    if regex(r"^\S")?.is_match(line)? { break; }
                    script.insert(next);
                }
            }
        }
        let mut out = Vec::new();
        for at in regions
            .into_iter()
            .flatten()
            .filter(|at| !script.contains(at))
        {
            self.charge(1)?;
            for binding in line_bindings(file, at, name)? {
                if binding != LineBinding::Global {
                    out.push(binding);
                }
            }
        }
        Ok(out)
    }
    fn own_bindings(
        &self,
        scope: Option<&Node>,
        file: &File<'_>,
        name: &str,
    ) -> Result<Vec<LineBinding>> {
        let mut out = Vec::new();
        for at in own_lines(scope, file) {
            self.charge(1)?;
            out.extend(line_bindings(file, at, name)?);
        }
        Ok(out)
    }
    fn binds_locally(
        &self,
        name: &str,
        reference: &UnresolvedReference,
        file: &File<'_>,
        imports_bind: bool,
    ) -> Result<bool> {
        let param = regex(&format!(
            r"[(,]\s*\*{{0,2}}{}\s*[:=,)]",
            regex::escape(name)
        ))?;
        let mut scopes = file
            .functions
            .iter()
            .copied()
            .filter(|scope| scope.start_line <= reference.line && scope.end_line >= reference.line)
            .collect::<Vec<_>>();
        scopes.sort_by_key(|scope| std::cmp::Reverse(scope.start_line));
        for scope in scopes {
            let own = own_lines(Some(scope), file);
            let bindings = self.own_bindings(Some(scope), file, name)?;
            if bindings.contains(&LineBinding::Global) {
                return Ok(false);
            }
            let mut definition = None;
            for &at in &own {
                if regex(r"^\s*(?:async\s+)?def\b")?.is_match(&file.lines[at])? {
                    definition = Some(at);
                    break;
                }
            }
            let mut header = scope.signature.clone().unwrap_or_default();
            if let Some(at) = definition {
                for next in at..file.lines.len() {
                    if next != at && file.starts[next] {
                        break;
                    }
                    header.push_str(&file.lines[next]);
                }
            }
            if param.is_match(&header)?
                || bindings.iter().any(|binding| match binding {
                    LineBinding::Global => false,
                    LineBinding::Import { .. } => imports_bind,
                    LineBinding::Other | LineBinding::Assign { .. } => true,
                })
            {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

fn own_lines(scope: Option<&Node>, file: &File<'_>) -> Vec<usize> {
    let from = scope.map_or(1, |scope| scope.start_line as usize);
    let to = scope.map_or(file.lines.len(), |scope| {
        (scope.end_line as usize).min(file.lines.len())
    });
    if from > to {
        return Vec::new();
    }
    let mut nested = vec![false; to - from + 1];
    for node in &file.nodes {
        if !matches!(node.kind.as_str(), "class" | "function" | "method")
            || scope.is_some_and(|scope| scope.id == node.id)
        {
            continue;
        }
        let start = node.start_line as usize;
        let end = node.end_line as usize;
        if start < from
            || end > to
            || scope.is_some_and(|scope| node.start_line <= scope.start_line)
        {
            continue;
        }
        nested[start - from..=end - from].fill(true);
    }
    (from..=to)
        .filter(|&line| !nested[line - from])
        .map(|line| line - 1)
        .collect()
}

fn line_bindings(file: &File<'_>, at: usize, name: &str) -> Result<Vec<LineBinding>> {
    let mut line = file.lines[at].clone();
    if file.starts[at] {
        for next in at + 1..file.lines.len() {
            if file.starts[next] {
                break;
            }
            line.push('\n');
            line.push_str(&file.lines[next]);
        }
    }
    let mut out = Vec::new();
    if file.starts[at]
        && line.contains("import")
        && let Some(capture) =
            regex(r"^\s*(?:from\s+([\w.]+)\s+)?import\s+([\s\S]*)$")?.captures(&line)?
    {
        let source = capture.get(1).map(|part| part.as_str());
        let names = &capture[2];
        if source.is_some() && trim(names) == "*" {
            return Ok(vec![LineBinding::Other]);
        }
        if !names.contains(name) {
            return Ok(out);
        }
        for part in names.replace(['(', ')', '\\'], " ").split(',') {
            let Some(spec) = regex(r"^([\w.]+)(?:\s+as\s+(\w+))?$")?.captures(trim(part))? else {
                continue;
            };
            let original = &spec[1];
            let alias = spec.get(2).map(|part| part.as_str());
            let local = alias.unwrap_or_else(|| {
                if source.is_some() {
                    original
                } else {
                    original.split('.').next().unwrap_or_default()
                }
            });
            if local == name {
                out.push(LineBinding::Import {
                    key: if let Some(source) = source {
                        format!("{source}:{original}")
                    } else {
                        format!("{}:*", if alias.is_some() { original } else { local })
                    },
                });
            }
        }
        return Ok(out);
    }
    if !line.contains(name) {
        return Ok(out);
    }
    let escaped = regex::escape(name);
    let word = regex(&format!(r"(?<![\w.]){escaped}\b(?!\s*[.\[])"))?;
    if !word.is_match(&line)? {
        return Ok(out);
    }
    if let Some(capture) = regex(r"^\s*(global|nonlocal)\s+([\w\s,]+)$")?.captures(&line)? {
        if capture[2].split(',').any(|part| trim(part) == name) {
            out.push(if &capture[1] == "global" {
                LineBinding::Global
            } else {
                LineBinding::Other
            });
        }
        return Ok(out);
    }
    if file.starts[at] {
        if regex(r"^\s*case\b")?.is_match(&line)? {
            return Ok(vec![LineBinding::Other]);
        }
        if let Some(assignment) = assignments(&line)? {
            let target = if assignment.targets.len() == 1 && !assignment.augmented {
                trim(assignment.targets[0])
            } else {
                ""
            };
            let annotation =
                regex(&format!(r#"^{escaped}\s*:\s*["']?([\w.]+)["']?$"#))?.captures(target)?;
            if target == name || annotation.is_some() {
                out.push(LineBinding::Assign {
                    ty: annotation.map(|capture| capture[1].into()),
                    value: trim(assignment.value).into(),
                    line: at,
                });
            } else {
                for target in assignment.targets {
                    if word.is_match(target)? {
                        out.push(LineBinding::Other);
                        break;
                    }
                }
            }
        } else if let Some(annotation) =
            regex(&format!(r#"^\s*{escaped}\s*:\s*["']?([\w.]+)["']?\s*$"#))?.captures(&line)?
        {
            out.push(LineBinding::Assign {
                ty: Some(annotation[1].into()),
                value: String::new(),
                line: at,
            });
        } else if regex(r"^\s*del\b")?.is_match(&line)? {
            out.push(LineBinding::Other)
        }
    }
    if regex(&format!(r"\b{escaped}[ \t]*:=|\bas[ \t]+{escaped}\b"))?.is_match(&line)? {
        out.push(LineBinding::Other);
    }
    for capture in regex(r"\bfor\s+([^:]+?)\s+in\b")?.captures_iter(&line) {
        if word.is_match(&capture?[1])? {
            out.push(LineBinding::Other);
        }
    }
    for capture in regex(r"\blambda\b([^:]*):")?.captures_iter(&line) {
        if word.is_match(&capture?[1])? {
            out.push(LineBinding::Other);
        }
    }
    Ok(out)
}

pub(in super::super) struct Assignment<'a> {
    pub targets: Vec<&'a str>,
    pub value: &'a str,
    pub augmented: bool,
}
pub(in super::super) fn assignments(line: &str) -> Result<Option<Assignment<'_>>> {
    let bytes = line.as_bytes();
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut quote = None;
    let mut from = 0;
    let mut at = 0;
    let mut augmented = false;
    while at < bytes.len() {
        let byte = bytes[at];
        if let Some(active) = quote {
            if byte == b'\\' {
                at += 2;
                continue;
            }
            if byte == active {
                quote = None;
            }
            at += 1;
            continue;
        }
        match byte {
            b'\'' | b'"' => quote = Some(byte),
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            b'=' if depth == 0 => {
                if bytes.get(at + 1) == Some(&b'=') {
                    at += 2;
                    continue;
                }
                let previous = at.checked_sub(1).map(|at| bytes[at]);
                if matches!(previous, Some(b'!' | b':'))
                    || matches!(previous, Some(b'<' | b'>'))
                        && at.checked_sub(2).map(|at| bytes[at]) != previous
                {
                    at += 1;
                    continue;
                }
                let op = regex(r"(?://|\*\*|>>|<<|[-+*/%&|^@])$")?
                    .find(&line[from..at])?
                    .map_or(0, |found| found.as_str().len());
                augmented |= op > 0;
                out.push(&line[from..at - op]);
                from = at + 1;
            }
            _ => {}
        }
        at += 1;
    }
    Ok((!out.is_empty()).then(|| Assignment {
        targets: out,
        value: &line[from..],
        augmented,
    }))
}
