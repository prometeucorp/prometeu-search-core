//! Indexed receivers reuse lexical Go declarations and package ownership.
use super::*;
use crate::frameworks::frontend::object::matching;
use crate::synthesis::{GO_PREDECLARED, blank_go};

pub(super) fn normalize(text: &str) -> Result<String> {
    let spaced = regex(r"\s+")?.replace_all(text.trim(), " ");
    let pointer = regex(r"([*\]])\s+")?.replace_all(&spaced, "$1");
    Ok(regex(r"\s*\.\s*")?.replace_all(&pointer, ".").into_owned())
}
pub(super) fn column(text: &str, units: u32) -> usize {
    let mut used = 0;
    for (at, ch) in text.char_indices() {
        if used >= units {
            return at;
        }
        used += ch.len_utf16() as u32;
    }
    text.len()
}
impl Bindings<'_> {
    // Predeclared element types cannot acquire an unrelated project's method.
    fn primitive_element(
        &self,
        text: &str,
        file: &str,
        r: &UnresolvedReference,
        depth: usize,
    ) -> Result<bool> {
        if depth > 12 {
            return Ok(false);
        }
        self.step(1)?;
        let text = normalize(text)?;
        let mut s = text.as_str();
        if let Some(pointer) = s.strip_prefix('*') {
            if !pointer.starts_with('[') || pointer.starts_with("[]") {
                return Ok(false);
            }
            s = pointer;
        }
        if s.starts_with('[') || s.starts_with("map[") {
            let open = s.find('[').unwrap();
            let Some(close) = matching(s, open)? else {
                return Ok(false);
            };
            let element = s[close + 1..]
                .trim()
                .strip_prefix('*')
                .unwrap_or(s[close + 1..].trim());
            // The reference treats predeclared element types as external even
            // when an unrelated project declaration shares their name.
            let mut probe = r.clone();
            probe.reference_name = element.to_owned();
            return Ok(GO_PREDECLARED.contains(&element) && !self.parameter_bound(&probe)?);
        }
        if s == "string" {
            return Ok(true);
        }
        if !regex(r"^[A-Za-z_]\w*$")?.is_match(s)? {
            return Ok(false);
        }
        let mut probe = r.clone();
        probe.reference_name = s.to_owned();
        if self.parameter_bound(&probe)? {
            return Ok(false);
        }
        for &node in self.by_name.get(s).into_iter().flatten() {
            self.step(1)?;
            if node.language != "go"
                || node.kind != "type_alias"
                || dir(&node.file_path) != dir(file)
            {
                continue;
            }
            let mut declaration = String::new();
            for line in node.start_line..=node.end_line.max(node.start_line) {
                if let Some(line) = self.line(&node.file_path, line.saturating_sub(1) as usize) {
                    self.step(line.len() / 32 + 1)?;
                    declaration.push_str(line);
                    declaration.push('\n');
                }
            }
            let declaration = blank_go(&declaration);
            let offset = column(&declaration, node.start_column);
            let Some(name) = regex(r"^[A-Za-z_]\w*")?.find(&declaration[offset..])? else {
                continue;
            };
            let underlying = declaration[offset + name.end()..].trim_start_matches([' ', '\t']);
            let underlying = underlying.strip_prefix('=').unwrap_or(underlying).trim();
            // Only a complete simple type text is followed here. Generic lists,
            // structs, and multiple statements need the full type-spec reader.
            if regex(r"^[A-Za-z_\w*\[\] .0-9]+$")?.is_match(underlying)? {
                return self.primitive_element(underlying, &node.file_path, r, depth + 1);
            }
        }
        Ok(false)
    }
    fn indexed_receiver(&self, r: &UnresolvedReference) -> Result<Option<String>> {
        if r.language.as_deref() != Some("go")
            || r.reference_kind != "calls"
            || !regex(r"^[A-Za-z_]\w*$")?.is_match(&r.reference_name)?
        {
            return Ok(None);
        }
        let file = r.file_path.as_deref().unwrap_or("");
        let Some(line) = self.line(file, r.line.saturating_sub(1) as usize) else {
            return Ok(None);
        };
        let at = column(line, r.column);
        if !line[at..].contains("].")
            || (at > 0
                && (line.as_bytes()[at - 1].is_ascii_alphanumeric()
                    || line.as_bytes()[at - 1] == b'_'))
        {
            return Ok(None);
        }
        self.step(line.len() / 32 + 1)?;
        let code = blank_go(line);
        let Some(head) = regex(r"^[A-Za-z_]\w*")?.find(&code[at..])? else {
            return Ok(None);
        };
        let name = head.as_str();
        let open = at + head.end() + code[at + head.end()..].len()
            - code[at + head.end()..]
                .trim_start_matches([' ', '\t'])
                .len();
        if code.as_bytes().get(open) != Some(&b'[') {
            return Ok(None);
        }
        let Some(close) = matching(&code, open)? else {
            return Ok(None);
        };
        if code[open + 1..close].contains(':') {
            return Ok(None);
        }
        let tail = code[close + 1..].trim_start_matches([' ', '\t']);
        if !regex(&format!(r"^\.\s*{}\s*\(", regex::escape(&r.reference_name)))?.is_match(tail)? {
            return Ok(None);
        }
        let mut receiver = r.clone();
        receiver.reference_name = name.to_owned();
        let Some(declaration) = self.local_declaration(&receiver)? else {
            return Ok(None);
        };
        let ty = if let Some(ty) = declaration.ty {
            ty
        } else {
            let source = blank_go(self.sources.get(file).copied().unwrap_or_default());
            let Some(start) = regex(r"^[A-Za-z_]\w*\s*(?::=|=)\s*(?:make\s*\(\s*)?")?
                .find(&source[declaration.at..])?
            else {
                return Ok(None);
            };
            let tail = &source[declaration.at + start.end()..];
            let Some(ty) = regex(
                r"^(?:\[[^\]]*\]|map\[[^\]]*\])\s*\*?\s*[A-Za-z_]\w*(?:\s*\.\s*[A-Za-z_]\w*)?",
            )?
            .find(tail)?
            else {
                return Ok(None);
            };
            ty.as_str().to_owned()
        };
        Ok(Some(ty.replacen("...", "[]", 1)))
    }
    pub(super) fn indexed_no_project_method(&self, r: &UnresolvedReference) -> Result<bool> {
        match self.indexed_receiver(r)? {
            Some(ty) => {
                self.primitive_element(&ty, r.file_path.as_deref().unwrap_or_default(), r, 0)
            }
            None => Ok(false),
        }
    }
}

impl<'a> Bindings<'a> {
    pub(super) fn declaration(&self, node: &Node) -> String {
        (node.start_line..=node.end_line.max(node.start_line))
            .filter_map(|line| self.line(&node.file_path, line.saturating_sub(1) as usize))
            .collect::<Vec<_>>()
            .join("\n")
    }
    pub(super) fn named_type(&self, text: &str, file: &str) -> Result<Option<&'a Node>> {
        let text = normalize(text)?;
        let text = text.strip_prefix('*').unwrap_or(&text);
        let Some(name) =
            regex(r"^([A-Za-z_]\w*)(?:\.([A-Za-z_]\w*))?(?:\[[\s\S]*\])?$")?.captures(text)?
        else {
            return Ok(None);
        };
        let (name, directory) = if let Some(member) = name.get(2) {
            let Some(directory) = self
                .imports
                .get(file)
                .into_iter()
                .flatten()
                .find(|(binding, _)| binding.local_name == name[1])
                .and_then(|(_, directory)| directory.as_deref())
            else {
                return Ok(None);
            };
            (member.as_str(), directory)
        } else {
            (&name[1], dir(file))
        };
        let mut candidates = self
            .by_name
            .get(name)
            .into_iter()
            .flatten()
            .copied()
            .filter(|node| {
                node.language == "go"
                    && matches!(node.kind.as_str(), "struct" | "interface" | "type_alias")
                    && dir(&node.file_path) == directory
            });
        Ok(candidates.next().filter(|_| candidates.next().is_none()))
    }
    pub(super) fn indexed_element(
        &self,
        text: &str,
        file: &str,
        depth: usize,
    ) -> Result<Option<(String, String)>> {
        if depth >= 12 {
            return Ok(None);
        }
        self.step(1)?;
        let normalized = normalize(text)?;
        let text = normalized
            .strip_prefix('*')
            .filter(|text| text.starts_with('[') && !text.starts_with("[]"))
            .unwrap_or(&normalized);
        if text.starts_with('[') || text.starts_with("map[") {
            let Some(close) = matching(text, text.find('[').unwrap())? else {
                return Ok(None);
            };
            return Ok(Some((text[close + 1..].trim().to_owned(), file.to_owned())));
        }
        let Some(node) = self.named_type(text, file)? else {
            return Ok(None);
        };
        if node.kind != "type_alias" {
            return Ok(None);
        }
        let declaration = blank_go(&self.declaration(node));
        let start = column(&declaration, node.start_column);
        let tail = &declaration[start + node.name.len()..];
        let tail = tail.trim().strip_prefix('=').unwrap_or(tail.trim()).trim();
        self.indexed_element(tail, &node.file_path, depth + 1)
    }
    pub(super) fn indexed_type_method(
        &self,
        node: &'a Node,
        name: &str,
        seen: &mut std::collections::HashSet<String>,
    ) -> Result<Option<&'a Node>> {
        if seen.len() >= 16 || !seen.insert(node.id.clone()) {
            return Ok(None);
        }
        self.step(1)?;
        let qualified = format!("{}::{name}", node.name);
        let methods = self
            .by_name
            .get(name)
            .into_iter()
            .flatten()
            .copied()
            .filter(|method| {
                method.language == "go"
                    && method.kind == "method"
                    && method.qualified_name == qualified
                    && dir(&method.file_path) == dir(&node.file_path)
            })
            .collect::<Vec<_>>();
        if !methods.is_empty() {
            return Ok((methods.len() == 1).then(|| methods[0]));
        }
        let declaration_pattern = regex(&format!(
            r"(?m)^func\s*\(\s*(?:[A-Za-z_]\w*\s+)?\*?\s*{}\s*(?:\[[^\]\n]*\])?\s*\)\s*{}\s*[[(]",
            regex::escape(&node.name),
            regex::escape(name)
        ))?;
        for (&file, &source) in self.sources {
            if !file.ends_with(".go")
                || file.ends_with("_test.go")
                || dir(file) != dir(&node.file_path)
                || self.by_file.contains_key(file)
            {
                continue;
            }
            self.step(source.len() / 32 + 1)?;
            if source == crate::auxiliary_sources::GO_UNREAD_UNKNOWN
                || source.len() > 32 * 1024 * 1024
                || declaration_pattern.is_match(source)?
            {
                return Ok(None);
            }
        }
        let declaration = blank_go(&self.declaration(node));
        if node.kind == "type_alias" {
            let start = column(&declaration, node.start_column);
            let tail = declaration[start + node.name.len()..].trim();
            if let Some(alias) = tail.strip_prefix('=')
                && let Some(target) = self.named_type(alias.trim(), &node.file_path)?
            {
                return self.indexed_type_method(target, name, seen);
            }
            return Ok(None);
        }
        let Some((_, body)) = declaration.split_once('{') else {
            return Ok(None);
        };
        let Some((body, _)) = body.rsplit_once('}') else {
            return Ok(None);
        };
        let mut body = body.to_owned();
        let nested = regex(r"\{[^{}]*\}")?;
        while nested.is_match(&body)? {
            body = nested.replace_all(&body, "").into_owned();
        }
        for member in body.split(['\n', ';']) {
            if regex(&format!(r"^\s*{}\s+", regex::escape(name)))?.is_match(member)? {
                return Ok(None);
            }
            if let Some(embedded) = self.named_type(member.trim(), &node.file_path)?
                && let Some(method) = self.indexed_type_method(embedded, name, &mut seen.clone())?
            {
                return Ok(Some(method));
            }
        }
        Ok(None)
    }
    pub(super) fn indexed_project_method(
        &self,
        r: &UnresolvedReference,
    ) -> Result<Option<super::Answer<'a>>> {
        let Some(ty) = self.indexed_receiver(r)? else {
            return Ok(None);
        };
        let Some((mut element, file)) =
            self.indexed_element(&ty, r.file_path.as_deref().unwrap_or_default(), 0)?
        else {
            return Ok(None);
        };
        if file == r.file_path.as_deref().unwrap_or_default()
            && let Some(constraint) = self.constraint_text(element.trim_start_matches('*'), r)?
        {
            let Some(constraint) = constraint else {
                return Ok(Some(super::Answer {
                    node: None,
                    confidence: 0.,
                    resolved_by: "instance-method",
                }));
            };
            element = constraint;
        }
        let Some(node) = self.named_type(&element, &file)? else {
            let root = element
                .trim_start_matches('*')
                .split('.')
                .next()
                .unwrap_or_default();
            if element.starts_with("**")
                || self
                    .imports
                    .get(file.as_str())
                    .into_iter()
                    .flatten()
                    .any(|(binding, directory)| binding.local_name == root && directory.is_none())
            {
                return Ok(Some(super::Answer {
                    node: None,
                    confidence: 0.,
                    resolved_by: "instance-method",
                }));
            }
            return Ok(None);
        };
        let target = self.indexed_type_method(
            node,
            &r.reference_name,
            &mut std::collections::HashSet::new(),
        )?;
        Ok(Some(super::Answer {
            node: target,
            confidence: 0.9,
            resolved_by: "instance-method",
        }))
    }
}
