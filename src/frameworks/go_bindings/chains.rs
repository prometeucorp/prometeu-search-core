//! Method receivers retain their declaring package through expressions and calls.
use super::*;
use crate::frameworks::frontend::object::matching;
use crate::synthesis::{GO_PREDECLARED, blank_go, go_result_types};
use indexed::{column, normalize};
use std::{collections::HashSet, rc::Rc};

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Operand,
    Select,
    Assert,
    Call,
    Index,
    Composite,
}
struct Link<'a> {
    kind: Kind,
    text: &'a str,
}
#[derive(Clone, PartialEq)]
struct Type {
    text: String,
    file: String,
}
struct Fields {
    file: String,
    named: Vec<(Vec<String>, String)>,
    embedded: Vec<String>,
}
fn typed(text: &str, file: &str) -> Result<Type> {
    Ok(Type {
        text: normalize(text)?,
        file: file.to_owned(),
    })
}

fn links(source: &str, start: usize) -> Result<(Vec<Link<'_>>, usize)> {
    let mut at = start;
    let mut out = Vec::new();
    let name = |at: &mut usize| -> Option<&str> {
        let start = *at;
        if !source
            .as_bytes()
            .get(start)
            .is_some_and(|byte| byte.is_ascii_alphabetic() || *byte == b'_')
        {
            return None;
        }
        *at += 1;
        while source
            .as_bytes()
            .get(*at)
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
        {
            *at += 1;
        }
        Some(&source[start..*at])
    };
    let group = |at: &mut usize| -> Result<Option<&str>> {
        let Some(end) = matching(source, *at)? else {
            return Ok(None);
        };
        let inner = &source[*at + 1..end];
        *at = end + 1;
        Ok(Some(inner))
    };
    let first = if source.as_bytes().get(at) == Some(&b'(') {
        group(&mut at)?
    } else {
        name(&mut at)
    };
    let Some(first) = first else {
        return Ok((out, at));
    };
    out.push(Link {
        kind: Kind::Operand,
        text: first,
    });
    loop {
        while matches!(source.as_bytes().get(at), Some(b' ' | b'\t')) {
            at += 1;
        }
        let kind = match source.as_bytes().get(at) {
            Some(b'.') => {
                at += 1;
                while source
                    .as_bytes()
                    .get(at)
                    .is_some_and(u8::is_ascii_whitespace)
                {
                    at += 1;
                }
                if source.as_bytes().get(at) == Some(&b'(') {
                    Kind::Assert
                } else {
                    Kind::Select
                }
            }
            Some(b'(') => Kind::Call,
            Some(b'[') => Kind::Index,
            Some(b'{')
                if source.as_bytes().get(start) != Some(&b'(')
                    && out.len() <= 3
                    && out.iter().all(|link| {
                        matches!(link.kind, Kind::Operand | Kind::Select | Kind::Index)
                    }) =>
            {
                Kind::Composite
            }
            _ => break,
        };
        let text = if kind == Kind::Select {
            name(&mut at)
        } else {
            group(&mut at)?
        };
        let Some(text) = text else {
            if kind == Kind::Call {
                out.push(Link { kind, text: "" });
            }
            break;
        };
        out.push(Link {
            kind,
            text: text.trim(),
        });
    }
    Ok((out, at))
}

fn parts<'a>(text: &'a str, separators: &[u8]) -> Result<Vec<&'a str>> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut at = 0;
    while at < text.len() {
        let byte = text.as_bytes()[at];
        if matches!(byte, b'(' | b'[' | b'{') {
            if let Some(end) = matching(text, at)? {
                at = end;
            }
        } else if separators.contains(&byte) {
            out.push(text[start..at].trim());
            start = at + 1;
        }
        at += 1;
    }
    out.push(text[start..].trim());
    Ok(out)
}
fn statement_end(text: &str, start: usize) -> Result<usize> {
    let mut at = start;
    while at < text.len() {
        match text.as_bytes()[at] {
            b'(' | b'[' | b'{' => {
                if let Some(end) = matching(text, at)? {
                    at = end;
                } else {
                    return Ok(text.len());
                }
            }
            b'\n' | b';' | b'}' => return Ok(at),
            _ => {}
        }
        at += 1;
    }
    Ok(at)
}

impl<'a> Bindings<'a> {
    pub(super) fn constraint_text(
        &self,
        name: &str,
        reference: &UnresolvedReference,
    ) -> Result<Option<Option<String>>> {
        let mut probe = reference.clone();
        probe.reference_kind = "references".into();
        probe.reference_name = name.to_owned();
        let bound = self.parameter_bound(&probe)?;
        let file = reference.file_path.as_deref().unwrap_or_default();
        for &node in self.by_file.get(file).into_iter().flatten() {
            if !matches!(
                node.kind.as_str(),
                "function" | "method" | "struct" | "interface" | "type_alias"
            ) || node.start_line > reference.line
                || node.end_line < reference.line
            {
                continue;
            }
            let source = blank_go(&self.declaration(node));
            let Some(head) =
                regex(&format!(r"\b{}\s*\[", regex::escape(&node.name)))?.find(&source)?
            else {
                continue;
            };
            let open = head.end() - 1;
            let Some(end) = matching(&source, open)? else {
                continue;
            };
            let params = parts(&source[open + 1..end], b",")?;
            let mut waiting = false;
            for param in params {
                let Some(declaration) =
                    regex(r"^([A-Za-z_]\w*)(?:\s+([\s\S]+))?$")?.captures(param)?
                else {
                    continue;
                };
                waiting |= &declaration[1] == name;
                if let Some(constraint) = declaration.get(2)
                    && waiting
                {
                    let constraint = constraint.as_str().trim();
                    return Ok(Some(
                        (regex(r"^[A-Za-z_]\w*(?:\.[A-Za-z_]\w*)?$")?.is_match(constraint)?
                            && !matches!(constraint, "any" | "comparable"))
                        .then(|| constraint.to_owned()),
                    ));
                }
            }
        }
        Ok(bound.then_some(None))
    }
    fn chain_source(&self, file: &str) -> Result<Rc<String>> {
        if let Some(source) = self.chain_sources.borrow().get(file) {
            return Ok(source.clone());
        }
        let Some((&file, &source)) = self.sources.get_key_value(file) else {
            return Ok(Rc::new(String::new()));
        };
        self.step(source.len() / 32 + 1)?;
        let source = Rc::new(blank_go(source));
        let mut cache = self.chain_sources.borrow_mut();
        if cache.len() >= 64 {
            cache.clear();
        }
        cache.insert(file, source.clone());
        Ok(source)
    }
    fn chain_local(
        &self,
        name: &str,
        file: &str,
        at: usize,
    ) -> Result<Option<crate::synthesis::GoDeclaration>> {
        let Some((&file, &source)) = self.sources.get_key_value(file) else {
            return Ok(None);
        };
        let mut scopes = self.scopes.borrow_mut();
        if !scopes.contains_key(file) {
            self.step(source.len() / 32 + 1)?;
            if scopes.len() >= 64 {
                scopes.clear();
            }
            scopes.insert(file, GoScopes::new(source)?);
        }
        scopes[file].lookup(name, at)
    }
    fn chain_result(&self, node: &Node, result: usize) -> Result<Option<Type>> {
        let Some(text) = go_result_types(node.signature.as_deref())?
            .and_then(|types| types.get(result).cloned())
        else {
            return Ok(None);
        };
        let probe = UnresolvedReference {
            candidates: None,
            from_node_id: node.id.clone(),
            reference_name: text.clone(),
            reference_kind: "references".into(),
            line: node.start_line,
            column: node.start_column,
            file_path: Some(node.file_path.clone()),
            language: Some("go".into()),
        };
        if let Some(constraint) = self.constraint_text(&text, &probe)? {
            return constraint
                .map(|text| typed(&text, &node.file_path))
                .transpose();
        }
        Ok(Some(typed(&text, &node.file_path)?))
    }
    fn chain_called(
        &self,
        name: &str,
        directory: &str,
        written: &Type,
        imported: bool,
        result: usize,
        depth: usize,
    ) -> Result<Option<Type>> {
        let file = written.file.as_str();
        if depth > 12 {
            return Ok(None);
        }
        self.step(1)?;
        let functions = self
            .by_name
            .get(name)
            .into_iter()
            .flatten()
            .copied()
            .filter(|node| {
                node.language == "go"
                    && node.kind == "function"
                    && dir(&node.file_path) == directory
                    && (if imported {
                        !node.file_path.ends_with("_test.go")
                    } else {
                        self.packages.get(file) == self.packages.get(node.file_path.as_str())
                            && (file.ends_with("_test.go") || !node.file_path.ends_with("_test.go"))
                    })
            })
            .collect::<Vec<_>>();
        if functions.is_empty() {
            if result == 0 && self.named_type(&written.text, file)?.is_some() {
                return Ok(Some(written.clone()));
            }
            if !imported {
                for directory in self.dot_packages.get(file).into_iter().flatten() {
                    if let Some(ty) =
                        self.chain_called(name, directory, written, true, result, depth + 1)?
                    {
                        return Ok(Some(ty));
                    }
                }
            }
            return Ok(None);
        }
        let mut found = None;
        for node in functions {
            let Some(ty) = self.chain_result(node, result)? else {
                return Ok(None);
            };
            if found
                .as_ref()
                .is_some_and(|prior: &Type| prior.text != ty.text)
            {
                return Ok(None);
            }
            found = Some(ty);
        }
        Ok(found)
    }
    fn chain_package_value(
        &self,
        name: &str,
        directory: &str,
        file: &str,
        imported: bool,
        depth: usize,
    ) -> Result<Option<Type>> {
        if depth > 12 {
            return Ok(None);
        }
        let mut found = None;
        for &node in self.by_name.get(name).into_iter().flatten() {
            if node.language != "go"
                || !matches!(node.kind.as_str(), "variable" | "constant")
                || dir(&node.file_path) != directory
                || imported && node.file_path.ends_with("_test.go")
                || !imported
                    && (self.packages.get(file) != self.packages.get(node.file_path.as_str())
                        || !file.ends_with("_test.go") && node.file_path.ends_with("_test.go"))
            {
                continue;
            }
            let declaration = blank_go(&self.declaration(node));
            let Some(spec) = regex(
                r"^\s*(?:(?:var|const)\s+)?([A-Za-z_]\w*(?:\s*,\s*[A-Za-z_]\w*)*)\s*([\s\S]*)$",
            )?
            .captures(&declaration)?
            else {
                return Ok(None);
            };
            let mut inherited = String::new();
            if spec[2].trim().is_empty() && node.kind == "constant" {
                let source = self.chain_source(&node.file_path)?;
                let lines = source
                    .lines()
                    .take(node.start_line.saturating_sub(1) as usize)
                    .collect::<Vec<_>>();
                for line in lines.into_iter().rev() {
                    if regex(r"^\s*const\b")?.is_match(line)? {
                        break;
                    }
                    if let Some(typed) =
                        regex(r"^\s*[A-Za-z_]\w*(?:\s*,\s*[A-Za-z_]\w*)*\s+(\S+)\s*=")?
                            .captures(line)?
                    {
                        inherited = typed[1].to_owned();
                        break;
                    }
                }
            }
            let tail = if inherited.is_empty() {
                spec[2].trim()
            } else {
                &inherited
            };
            let ty = if let Some(value) = tail.strip_prefix('=') {
                let names = spec[1].split(',').map(str::trim).collect::<Vec<_>>();
                let values = parts(value, b",")?;
                let Some(k) = names.iter().position(|item| *item == name) else {
                    return Ok(None);
                };
                if names.len() != values.len() {
                    return Ok(None);
                }
                self.chain_expr(values[k], usize::MAX, &node.file_path, depth + 1, 0)?
            } else {
                let text = tail.split('=').next().unwrap_or_default().trim();
                (!text.is_empty())
                    .then(|| typed(text, &node.file_path))
                    .transpose()?
            };
            let Some(ty) = ty else {
                return Ok(None);
            };
            if found
                .as_ref()
                .is_some_and(|prior: &Type| prior.text != ty.text)
            {
                return Ok(None);
            }
            found = Some(ty);
        }
        Ok(found)
    }
    fn chain_declared(
        &self,
        name: &str,
        decl: &crate::synthesis::GoDeclaration,
        file: &str,
        depth: usize,
    ) -> Result<Option<Type>> {
        let code = self.chain_source(file)?;
        let p = decl.at;
        if let Some(from) = regex(r"^[A-Za-z_]\w*((?:\s*,\s*[A-Za-z_]\w*)*)\s*(?::=|=(?!=))\s*")?
            .captures(code.get(p..).unwrap_or_default())?
        {
            let line = code[..p].rfind('\n').map_or(0, |at| at + 1);
            let before = regex(r"(?:[A-Za-z_]\w*\s*,\s*)*$")?
                .find(&code[line..p])?
                .unwrap();
            let k = if before.as_str().is_empty() {
                0
            } else {
                before.as_str().split(',').count() - 1
            };
            let n = k + from[1].split(',').count();
            let start = p + from.get(0).unwrap().end();
            let rhs = &code[start..statement_end(&code, start)?];
            if let Some(range) = rhs.trim().strip_prefix("range ") {
                if k != 1 || n != 2 {
                    return Ok(None);
                }
                return match self.chain_expr(range, decl.at, file, depth + 1, 0)? {
                    Some(ty) => self.chain_element(&ty, false, depth + 1),
                    None => Ok(None),
                };
            }
            let values = parts(rhs, b",")?;
            if values.len() == n {
                return self.chain_expr(values[k], decl.at, file, depth + 1, 0);
            }
            if values.len() == 1 {
                return self.chain_expr(values[0], decl.at, file, depth + 1, k);
            }
            return Ok(None);
        }
        let _ = name;
        decl.ty
            .as_ref()
            .map(|ty| typed(&ty.replacen("...", "[]", 1), file))
            .transpose()
    }
    fn chain_element(&self, ty: &Type, slice: bool, depth: usize) -> Result<Option<Type>> {
        if depth > 12 {
            return Ok(None);
        }
        if let Some((element, file)) = self.indexed_element(&ty.text, &ty.file, 0)? {
            return Ok(Some(typed(
                &if slice {
                    format!("[]{element}")
                } else {
                    element
                },
                &file,
            )?));
        }
        if slice && ty.text == "string" {
            return Ok(Some(ty.clone()));
        }
        Ok(None)
    }
    fn chain_fields(&self, node: &Node, depth: usize) -> Result<Option<Fields>> {
        if depth > 12 {
            return Ok(None);
        }
        self.step(1)?;
        let text = blank_go(&self.declaration(node));
        if node.kind == "type_alias" {
            let start = column(&text, node.start_column) + node.name.len();
            let mut tail = text[start..].trim();
            if tail.starts_with('[')
                && let Some(end) = matching(tail, 0)?
            {
                tail = tail[end + 1..].trim();
            }
            tail = tail.strip_prefix('=').unwrap_or(tail).trim();
            if let Some(target) = self.named_type(tail, &node.file_path)? {
                return self.chain_fields(target, depth + 1);
            }
        }
        let Some(start) = text.find('{') else {
            return Ok(None);
        };
        let Some(end) = matching(&text, start)? else {
            return Ok(None);
        };
        let mut fields = Vec::new();
        let mut embedded = Vec::new();
        for line in parts(&text[start + 1..end], b"\n;")? {
            let line = line.split('`').next().unwrap_or_default().trim();
            if line.is_empty() {
                continue;
            }
            if let Some(field) =
                regex(r"^([A-Za-z_]\w*(?:\s*,\s*[A-Za-z_]\w*)*)\s+(\S[\s\S]*)$")?.captures(line)?
            {
                fields.push((
                    field[1]
                        .split(',')
                        .map(|name| name.trim().to_owned())
                        .collect(),
                    field[2].trim().to_owned(),
                ));
            } else {
                embedded.push(line.to_owned());
            }
        }
        Ok(Some(Fields {
            file: node.file_path.clone(),
            named: fields,
            embedded,
        }))
    }
    fn chain_field(&self, ty: &Type, name: &str, depth: usize) -> Result<Option<Type>> {
        if depth > 12 {
            return Ok(None);
        }
        let mut level = vec![ty.clone()];
        let mut seen = HashSet::new();
        for _ in 0..4 {
            let mut found = Vec::new();
            let mut next = Vec::new();
            for ty in level {
                let Some(node) = self.named_type(&ty.text, &ty.file)? else {
                    continue;
                };
                let Some(Fields {
                    file,
                    named: fields,
                    embedded,
                }) = self.chain_fields(node, depth + 1)?
                else {
                    continue;
                };
                for (names, text) in fields {
                    if names.iter().any(|field| field == name) {
                        found.push(typed(&text, &file)?);
                    }
                }
                for text in embedded {
                    let bare = text
                        .trim_start_matches('*')
                        .split('[')
                        .next()
                        .unwrap_or_default()
                        .rsplit('.')
                        .next()
                        .unwrap_or_default()
                        .trim();
                    if bare == name {
                        found.push(typed(&text, &file)?);
                    }
                    if seen.insert((file.clone(), text.clone())) {
                        next.push(typed(&text, &file)?);
                    }
                }
            }
            if !found.is_empty() {
                return Ok((found.len() == 1).then(|| found.remove(0)));
            }
            if next.is_empty() {
                break;
            }
            level = next;
        }
        Ok(None)
    }
    fn chain_method_result(
        &self,
        ty: &Type,
        name: &str,
        result: usize,
        depth: usize,
    ) -> Result<Option<Type>> {
        if depth > 12 {
            return Ok(None);
        }
        if let Some(node) = self.named_type(&ty.text, &ty.file)?
            && let Some(method) = self.indexed_type_method(node, name, &mut HashSet::new())?
        {
            return self.chain_result(method, result);
        }
        if let Some(field) = self.chain_field(ty, name, depth + 1)?
            && let Some(signature) = field.text.strip_prefix("func")
        {
            return go_result_types(Some(signature.trim()))?
                .and_then(|types| types.get(result).cloned())
                .map(|text| typed(&text, &field.file))
                .transpose();
        }
        Ok(None)
    }
    fn chain_expr(
        &self,
        expr: &str,
        at: usize,
        file: &str,
        depth: usize,
        result: usize,
    ) -> Result<Option<Type>> {
        if depth > 12 {
            return Ok(None);
        }
        self.step(1)?;
        let text = expr.trim();
        if result == 0 {
            if let Some(inner) = text.strip_prefix('&') {
                return self
                    .chain_expr(inner, at, file, depth + 1, 0)?
                    .map(|ty| typed(&format!("*{}", ty.text), &ty.file))
                    .transpose();
            }
            if let Some(inner) = text.strip_prefix('*') {
                return self
                    .chain_expr(inner, at, file, depth + 1, 0)?
                    .and_then(|ty| {
                        ty.text
                            .strip_prefix('*')
                            .map(|text| (text.to_owned(), ty.file.clone()))
                    })
                    .map(|(text, file)| typed(&text, &file))
                    .transpose();
            }
            if let Some(builtin) = regex(r"^(make|new|append)\s*\(")?.captures(text)? {
                let open = builtin.get(0).unwrap().end() - 1;
                if matching(text, open)? != Some(text.len() - 1) {
                    return Ok(None);
                }
                let args = parts(&text[open + 1..text.len() - 1], b",")?;
                let first = args[0];
                return match &builtin[1] {
                    "make" => Ok(Some(typed(first, file)?)),
                    "new" => Ok(Some(typed(&format!("*{first}"), file)?)),
                    _ => self.chain_expr(first, at, file, depth + 1, 0),
                };
            }
            if let Some(open) = text.find('{')
                && (text.starts_with('[') || text.starts_with("map["))
                && matching(text, open)? == Some(text.len() - 1)
            {
                return Ok(Some(typed(&text[..open], file)?));
            }
        }
        let (links, end) = links(text, 0)?;
        if end != text.len() {
            return Ok(None);
        }
        self.chain_type(&links, text.starts_with('('), at, file, depth + 1, result)
    }
    fn chain_type(
        &self,
        links: &[Link<'_>],
        paren: bool,
        at: usize,
        file: &str,
        depth: usize,
        result: usize,
    ) -> Result<Option<Type>> {
        if depth > 12 {
            return Ok(None);
        }
        self.step(links.len() + 1)?;
        let Some(head) = links.first() else {
            return Ok(None);
        };
        if result == 0
            && links
                .last()
                .is_some_and(|link| link.kind == Kind::Assert && link.text != "type")
        {
            return Ok(Some(typed(links.last().unwrap().text, file)?));
        }
        let mut index = 1;
        let mut ty = if paren
            && links.get(1).is_some_and(|link| link.kind == Kind::Call)
            && self.named_type(head.text, file)?.is_some()
        {
            index = 2;
            Some(typed(head.text, file)?)
        } else if paren {
            self.chain_expr(head.text, at, file, depth + 1, 0)?
        } else if at != usize::MAX
            && let Some(decl) = self.chain_local(head.text, file, at)?
        {
            if links.get(1).is_some_and(|link| link.kind == Kind::Call) {
                return Ok(None);
            }
            self.chain_declared(head.text, &decl, file, depth + 1)?
        } else if let Some((_, directory)) = self
            .imports
            .get(file)
            .into_iter()
            .flatten()
            .find(|(mapping, _)| mapping.local_name == head.text)
        {
            if links.get(1).is_none_or(|link| link.kind != Kind::Select) {
                return Ok(None);
            }
            let Some(directory) = directory else {
                return Ok(None);
            };
            index = 2;
            let mut written = format!("{}.{}", head.text, links[1].text);
            if links
                .get(index)
                .is_some_and(|link| link.kind == Kind::Index)
                && links
                    .get(index + 1)
                    .is_some_and(|link| matches!(link.kind, Kind::Call | Kind::Composite))
            {
                written.push_str(&format!("[{}]", links[index].text));
                index += 1;
            }
            match links.get(index).map(|link| link.kind) {
                Some(Kind::Composite) => {
                    index += 1;
                    Some(typed(&written, file)?)
                }
                Some(Kind::Call) => {
                    index += 1;
                    self.chain_called(
                        links[1].text,
                        directory,
                        &typed(&written, file)?,
                        true,
                        if index == links.len() { result } else { 0 },
                        depth + 1,
                    )?
                }
                _ => self.chain_package_value(links[1].text, directory, file, true, depth + 1)?,
            }
        } else {
            let mut written = head.text.to_owned();
            if links
                .get(index)
                .is_some_and(|link| link.kind == Kind::Index)
                && links
                    .get(index + 1)
                    .is_some_and(|link| matches!(link.kind, Kind::Call | Kind::Composite))
            {
                written.push_str(&format!("[{}]", links[index].text));
                index += 1;
            }
            match links.get(index).map(|link| link.kind) {
                Some(Kind::Composite) => {
                    index += 1;
                    Some(typed(&written, file)?)
                }
                Some(Kind::Call) => {
                    index += 1;
                    if matches!(head.text, "new" | "make" | "append") {
                        self.chain_expr(
                            &format!("{}({})", head.text, links[index - 1].text),
                            at,
                            file,
                            depth + 1,
                            0,
                        )?
                    } else {
                        self.chain_called(
                            head.text,
                            dir(file),
                            &typed(&written, file)?,
                            false,
                            if index == links.len() { result } else { 0 },
                            depth + 1,
                        )?
                    }
                }
                _ => self.chain_package_value(head.text, dir(file), file, false, depth + 1)?,
            }
        };
        while index < links.len() {
            let Some(current) = ty else {
                return Ok(None);
            };
            let link = &links[index];
            ty = match link.kind {
                Kind::Select
                    if links
                        .get(index + 1)
                        .is_some_and(|link| link.kind == Kind::Call) =>
                {
                    index += 1;
                    self.chain_method_result(
                        &current,
                        link.text,
                        if index + 1 == links.len() { result } else { 0 },
                        depth + 1,
                    )?
                }
                Kind::Select => self.chain_field(&current, link.text, depth + 1)?,
                Kind::Index => self.chain_element(&current, link.text.contains(':'), depth + 1)?,
                Kind::Assert if link.text != "type" => Some(typed(link.text, file)?),
                _ => return Ok(None),
            };
            index += 1;
        }
        Ok(ty)
    }
    pub(super) fn chained_method(&self, r: &UnresolvedReference) -> Result<Option<Answer<'a>>> {
        if r.language.as_deref() != Some("go")
            || !matches!(r.reference_kind.as_str(), "calls" | "function_ref")
        {
            return Ok(None);
        }
        let value = r.reference_kind == "function_ref";
        let file = r.file_path.as_deref().unwrap_or_default();
        let Some(line) = self.line(file, r.line.saturating_sub(1) as usize) else {
            return Ok(None);
        };
        let at =
            line.as_ptr() as usize - self.sources[file].as_ptr() as usize + column(line, r.column);
        let code = self.chain_source(file)?;
        if at > 0 && code.as_bytes()[at - 1].is_ascii_alphanumeric() {
            return Ok(None);
        }
        // Member values locate the final name; calls locate the receiver expression.
        let (chain, _) = if value {
            links(&r.reference_name, 0)?
        } else {
            links(&code, at)?
        };
        let name = r.reference_name.rsplit('.').next().unwrap_or_default();
        let mut positions = chain
            .iter()
            .enumerate()
            .filter(|(index, link)| {
                *index > 0
                    && link.kind == Kind::Select
                    && link.text == name
                    && (value
                        || chain
                            .get(index + 1)
                            .is_some_and(|link| link.kind == Kind::Call))
            })
            .map(|(index, _)| index);
        let Some(first) = positions.next() else {
            return if value { self.qualified(r) } else { Ok(None) };
        };
        let index = if r.reference_name == name && chain[first - 1].kind != Kind::Index {
            positions.next_back().unwrap_or(first)
        } else {
            first
        };
        if index == 1
            && self
                .imports
                .get(file)
                .into_iter()
                .flatten()
                .any(|(mapping, _)| mapping.local_name == chain[0].text)
        {
            return if value { self.qualified(r) } else { Ok(None) };
        }
        let complex = index > 1 || code.as_bytes().get(at) == Some(&b'(');
        let mut ty = self.chain_type(
            &chain[..index],
            code.as_bytes().get(at) == Some(&b'('),
            at,
            file,
            0,
            0,
        )?;
        if value && ty.is_none() && index == 1 && self.named_type(chain[0].text, file)?.is_some() {
            ty = Some(typed(chain[0].text, file)?);
        }
        if let Some(known) = ty.as_mut()
            && known.file == file
            && let Some(constraint) = self.constraint_text(known.text.trim_start_matches('*'), r)?
        {
            let Some(constraint) = constraint else {
                return Ok(Some(Answer {
                    node: None,
                    confidence: 0.,
                    resolved_by: "instance-method",
                }));
            };
            known.text = constraint;
        }
        let Some(ty) = ty else {
            if !value
                && !complex
                && let Some(decl) = self.chain_local(chain[0].text, file, at)?
            {
                let statement = &code[decl.at..statement_end(&code, decl.at)?];
                if let Some(call) =
                    regex(r"(?::=|=)\s*([A-Za-z_]\w*)\.\w+\s*\(")?.captures(statement)?
                    && let Some(receiver) = self.chain_local(&call[1], file, decl.at)?
                    && receiver
                        .ty
                        .is_some_and(|ty| ty.ends_with(']') && ty.contains('.'))
                {
                    return self.method_name_fallback(name, chain[0].text, r);
                }
            }
            if value && !complex {
                let pool = self
                    .by_name
                    .get(name)
                    .into_iter()
                    .flatten()
                    .copied()
                    .filter(|node| node.language == "go")
                    .collect::<Vec<_>>();
                if let Some(node) = pool.first().copied().filter(|_| pool.len() == 1)
                    && node.id != r.from_node_id
                    && matches!(node.kind.as_str(), "function" | "method")
                {
                    let words = |text: &str| -> Result<Vec<String>> {
                        Ok(regex(r"([a-z])([A-Z])")?
                            .replace_all(text, "$1 $2")
                            .split(|ch: char| !ch.is_alphanumeric())
                            .filter(|part| part.len() > 1)
                            .map(str::to_lowercase)
                            .collect())
                    };
                    let owner = node
                        .qualified_name
                        .rsplit_once("::")
                        .map_or("", |(owner, _)| owner);
                    let receiver_words = words(chain[0].text)?;
                    if node.kind == "function"
                        || words(owner)?
                            .iter()
                            .any(|word| receiver_words.contains(word))
                    {
                        return Ok(Some(Answer {
                            node: Some(node),
                            confidence: 0.8,
                            resolved_by: "function-ref",
                        }));
                    }
                }
            }
            return Ok((complex || value).then_some(Answer {
                node: None,
                confidence: 0.,
                resolved_by: "instance-method",
            }));
        };
        let target = if let Some(node) = self.named_type(&ty.text, &ty.file)? {
            self.indexed_type_method(node, name, &mut HashSet::new())?
        } else {
            None
        };
        if !complex
            && !value
            && target.is_none()
            && let Some(node) = self.named_type(&ty.text, &ty.file)?
            && node.kind == "type_alias"
        {
            let declaration = blank_go(&self.declaration(node));
            let tail =
                declaration[column(&declaration, node.start_column) + node.name.len()..].trim();
            if !tail.starts_with('=')
                && self
                    .named_type(tail, &node.file_path)?
                    .is_some_and(|base| base.kind == "interface")
            {
                return self.method_name_fallback(name, chain[0].text, r);
            }
        }
        let confidence =
            if index == 2 && chain[1].kind == Kind::Select || r.reference_name.contains("().") {
                0.85
            } else if !complex
                && !value
                && target.is_some()
                && self
                    .by_name
                    .get(name)
                    .into_iter()
                    .flatten()
                    .filter(|node| node.language == "go" && node.kind == "method")
                    .count()
                    == 1
            {
                let simple = match self.chain_local(chain[0].text, file, at)? {
                    Some(decl) => {
                        decl.ty.is_some()
                            || self.chain_source(file)?[decl.at..statement_end(&code, decl.at)?]
                                .trim()
                                .ends_with([')', '}'])
                    }
                    None => self
                        .by_name
                        .get(chain[0].text)
                        .into_iter()
                        .flatten()
                        .any(|node| node.kind == "constant" && dir(&node.file_path) == dir(file)),
                };
                if simple { 0.9 } else { 0.7 }
            } else {
                0.9
            };
        let known = value
            || complex
            || target.is_some()
            || GO_PREDECLARED.contains(&ty.text.as_str())
            || self.named_type(&ty.text, &ty.file)?.is_some()
            || ty.text.contains('.');
        Ok(known.then_some(Answer {
            node: target.filter(|node| !value || node.id != r.from_node_id),
            confidence,
            resolved_by: if value && index == 1 {
                "function-ref"
            } else {
                "instance-method"
            },
        }))
    }
    fn method_name_fallback(
        &self,
        name: &str,
        receiver: &str,
        r: &UnresolvedReference,
    ) -> Result<Option<Answer<'a>>> {
        let file = r.file_path.as_deref().unwrap_or_default();
        let methods = self
            .by_name
            .get(name)
            .into_iter()
            .flatten()
            .copied()
            .filter(|node| {
                node.language == "go"
                    && node.kind == "method"
                    && (crate::classify::is_test_path(file)
                        || !crate::classify::is_test_path(&node.file_path))
            })
            .collect::<Vec<_>>();
        if methods.len() == 1 {
            return Ok(Some(Answer {
                node: Some(methods[0]),
                confidence: 0.7,
                resolved_by: "instance-method",
            }));
        }
        let words = |text: &str| -> Result<Vec<String>> {
            let text = regex(r"([a-z])([A-Z])")?.replace_all(text, "$1 $2");
            let text = regex(r"([A-Z]+)([A-Z][a-z])")?.replace_all(&text, "$1 $2");
            Ok(text
                .split(|ch: char| !ch.is_alphanumeric())
                .filter(|part| part.len() > 1)
                .map(str::to_lowercase)
                .collect())
        };
        let receiver = words(receiver)?;
        let mut found = None;
        let mut best = 0;
        for node in methods {
            let owner = words(
                node.qualified_name
                    .rsplit_once("::")
                    .map_or("", |(owner, _)| owner),
            )?;
            let score = receiver.iter().filter(|word| owner.contains(word)).count();
            if score > best {
                best = score;
                found = Some(node);
            }
        }
        Ok(found.map(|node| Answer {
            node: Some(node),
            confidence: 0.65,
            resolved_by: "instance-method",
        }))
    }
}
