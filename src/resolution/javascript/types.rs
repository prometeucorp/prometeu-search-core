//! Type declaration, alias and inheritance lookup used by declared field receivers.
use super::*;
#[derive(Clone, Copy)]
enum Named<'a> {
    Node(&'a Node),
    Outside,
    Unknown,
}
const OWNERS: &[&str] = &["class", "interface"];
impl<'a> Context<'a> {
    pub(super) fn class_head(&self, class: &Node) -> Result<String> {
        let text = self
            .lines(&class.file_path)?
            .skip(class.start_line.saturating_sub(1) as usize)
            .take(8)
            .collect::<Vec<_>>()
            .join(" ");
        let start = regex(&format!(
            r"\b(?:class|interface)\s+{}\b",
            ::regex::escape(&class.name)
        ))?
        .find(&text)?
        .or(regex(r"\bclass\b")?.find(&text)?)
        .map_or(0, |m| m.start());
        let mut head = String::new();
        let mut angles = 0;
        let mut previous = '\0';
        for ch in text[start..].chars() {
            if ch == '<' {
                angles += 1;
            } else if ch == '>' && angles > 0 && previous != '=' {
                angles -= 1;
            } else if angles == 0 {
                if ch == '{' {
                    break;
                }
                head.push(ch);
            }
            previous = ch;
        }
        Ok(head)
    }
    pub(super) fn head_supers(&self, class: &Node) -> Result<Option<Vec<String>>> {
        let head = self.class_head(class)?;
        let mut out = Vec::new();
        for keyword in ["extends", "implements"] {
            let pattern = regex(&format!(
                r"\b{keyword}\b([\s\S]*?)(?:\b(?:extends|implements)\b|$)"
            ))?;
            let Some(clause) = pattern.captures(&head)? else {
                continue;
            };
            for part in clause[1].split(',') {
                let name = trim(part);
                if !test(r"^[A-Za-z_$][\w$]*(?:\.[A-Za-z_$][\w$]*)*$", name)? {
                    if keyword == "extends" {
                        return Ok(None);
                    }
                    continue;
                }
                out.push(name.to_string());
            }
        }
        Ok(Some(out))
    }
    pub(super) fn declared_supers(&self, class: &Node) -> Result<Vec<String>> {
        let head = self.class_head(class)?;
        let pattern = regex(r"\bextends\s+([\w.\\]+)")?;
        let Some(found) = pattern.captures(&head)? else {
            return Ok(vec![]);
        };
        Ok(vec![
            found[1]
                .rsplit([':', '.', '\\'])
                .next()
                .unwrap_or("")
                .to_string(),
        ])
    }
    pub(super) fn alias_value(&self, alias: &Node) -> Result<Option<String>> {
        let lines = self.lines(&alias.file_path)?;
        let mut lines: Vec<_> = lines
            .into_iter()
            .skip(alias.start_line.saturating_sub(1) as usize)
            .take((alias.end_line - alias.start_line + 1) as usize)
            .map(str::to_string)
            .collect();
        let Some(last) = lines.last_mut() else {
            return Ok(None);
        };
        last.truncate(byte(last, alias.end_column));
        let text = lines.join("\n");
        let text = crate::synthesis::comments::strip_for_regex(
            &text[byte(&text, alias.start_column)..],
            "typescript",
        );
        let mut depth = 0i32;
        let mut value = None::<String>;
        let chars: Vec<_> = text.chars().collect();
        for (i, &ch) in chars.iter().enumerate() {
            if ch == '<' {
                depth += 1;
            } else if ch == '>' && i.checked_sub(1).and_then(|j| chars.get(j)) != Some(&'=') {
                depth -= 1;
            } else if depth > 0 {
                continue;
            } else if let Some(value) = &mut value {
                value.push(ch);
            } else if ch == '=' && chars.get(i + 1) != Some(&'>') {
                value = Some(String::new());
            }
            if depth < 0 {
                return Ok(None);
            }
        }
        let Some(value) = value else {
            return Ok(None);
        };
        let value = regex(r";\s*$")?.replace(&value, "");
        let parts: Vec<_> = value
            .split('|')
            .map(trim)
            .filter(|part| !matches!(*part, "" | "null" | "undefined"))
            .collect();
        if parts.len() != 1 {
            return Ok(None);
        }
        let name = regex(r"^\?\s*")?.replace(parts[0], "");
        Ok(test(r"^[A-Za-z_$][\w$]*(?:\.[A-Za-z_$][\w$]*)*$", &name)?.then(|| name.to_string()))
    }
    pub(super) fn type_parameter(&self, class: &Node, name: &str) -> Result<bool> {
        let head = self
            .lines(&class.file_path)?
            .skip(class.start_line.saturating_sub(1) as usize)
            .take(24)
            .collect::<Vec<_>>()
            .join("\n");
        let pattern = regex(&format!(
            r"\b(?:class|interface|type)\s+{}\s*<",
            ::regex::escape(&class.name)
        ))?;
        let Some(open) = pattern.find(&head)? else {
            return Ok(false);
        };
        let named = |part: &str| -> Result<bool> {
            let part = regex(r"^(?:in|out|const)\s+")?.replace(trim(part), "");
            Ok(part
                .split(|ch: char| crate::search::query_whitespace(ch) || ch == '=')
                .next()
                == Some(name))
        };
        let mut depth = 0i32;
        let mut from = open.end();
        for (i, ch) in head.char_indices().skip_while(|(i, _)| *i < open.end()) {
            match ch {
                '<' | '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' => depth -= 1,
                '>' if head.as_bytes().get(i.wrapping_sub(1)) != Some(&b'=') => {
                    if depth == 0 {
                        return named(&head[from..i]);
                    }
                    depth -= 1;
                }
                ',' if depth == 0 => {
                    if named(&head[from..i])? {
                        return Ok(true);
                    }
                    from = i + 1;
                }
                _ => {}
            }
        }
        Ok(true)
    }
    fn type_in(&self, path: &str, name: &str) -> Option<&'a Node> {
        self.in_file(path)
            .find(|node| node.name == name && OWNERS.contains(&node.kind.as_str()))
    }
    pub(super) fn named_class(
        &self,
        name: &str,
        from: &Node,
        at: &UnresolvedReference,
    ) -> Result<Option<&'a Node>> {
        if self.imported(&from.file_path, name).is_some() {
            let node = self.resolve(name, from, at)?;
            return Ok(match node {
                Some(node)
                    if matches!(node.kind.as_str(), "class" | "component")
                        && !self.merged(node)? =>
                {
                    Some(node)
                }
                _ => None,
            });
        }
        Ok(self
            .in_file(&from.file_path)
            .find(|node| node.kind == "class" && node.name == name))
    }
    pub(super) fn imported_file(
        &self,
        written: &str,
        path: &str,
        language: &str,
        at: &UnresolvedReference,
    ) -> Result<Option<String>> {
        let pattern = regex(r"^[A-Za-z_$][\w$]*(?:\.[A-Za-z_$][\w$]*)*")?;
        let Some(name) = pattern.find(trim(written))? else {
            return Ok(None);
        };
        let name = name.as_str();
        if self
            .imported(path, name.split('.').next().unwrap())
            .is_none()
        {
            return Ok(None);
        }
        let mut reference = at.clone();
        reference.file_path = Some(path.to_string());
        reference.language = Some(language.to_string());
        reference.reference_name = name.to_string();
        reference.reference_kind = "references".to_string();
        let node =
            (self.resolve_import)(&reference)?.and_then(|id| self.ids.get(id.as_str()).copied());
        Ok(match node {
            Some(node)
                if node.kind != "file" && node.file_path != path && !self.merged(node)? =>
            {
                Some(node.file_path.clone())
            }
            _ => None,
        })
    }
    pub(super) fn outside(
        &self,
        written: &str,
        name: &str,
        path: &str,
        language: &str,
    ) -> Result<Option<Option<String>>> {
        let pattern = regex(r"^[A-Za-z_$][\w$]*")?;
        let Some(head) = pattern.find(trim(written))? else {
            return Ok(None);
        };
        let Some(binding) = self
            .imported(path, head.as_str())
            .filter(|binding| binding.external)
        else {
            return Ok(None);
        };
        let pattern = regex(&format!(
            r#"\bdeclare\s+module\s+(['"])(?:node:)?{}\1"#,
            ::regex::escape(
                binding
                    .source
                    .strip_prefix("node:")
                    .unwrap_or(&binding.source)
            )
        ))?;
        for node in self.named(name) {
            if family(&node.language, language) && pattern.is_match(self.source(node))? {
                return Ok(Some(Some(node.file_path.clone())));
            }
        }
        Ok(Some(None))
    }
    fn named_type(
        &self,
        written: &str,
        path: &str,
        language: &str,
        id: &str,
        at: &UnresolvedReference,
    ) -> Result<Named<'a>> {
        let head = written.split('.').next().unwrap_or(written);
        let last = written.rsplit('.').next().unwrap_or(written);
        if let Some(mapping) = self.imported(path, head) {
            if mapping.external {
                return Ok(match self.outside(written, last, path, language)? {
                    Some(None) => Named::Outside,
                    Some(Some(file)) => self
                        .type_in(&file, last)
                        .map_or(Named::Unknown, Named::Node),
                    None => Named::Unknown,
                });
            }
            let mut reference = at.clone();
            reference.file_path = Some(path.to_string());
            reference.language = Some(language.to_string());
            reference.reference_name = written.to_string();
            reference.reference_kind = "references".to_string();
            let node = (self.resolve_import)(&reference)?
                .and_then(|id| self.ids.get(id.as_str()).copied());
            return Ok(match node {
                Some(node) if !self.merged(node)? => {
                    if OWNERS.contains(&node.kind.as_str()) {
                        Named::Node(node)
                    } else {
                        self.type_in(&node.file_path, &node.name)
                            .map_or(Named::Unknown, Named::Node)
                    }
                }
                _ => Named::Unknown,
            });
        }
        Ok(if !written.contains('.') {
            self.in_file(path)
                .find(|node| {
                    node.id != id && node.name == written && OWNERS.contains(&node.kind.as_str())
                })
                .map_or(Named::Unknown, Named::Node)
        } else {
            Named::Unknown
        })
    }
    fn method_up(
        &self,
        class: &'a Node,
        name: &str,
        at: &UnresolvedReference,
        depth: usize,
    ) -> Result<(Option<&'a Node>, bool)> {
        self.step()?;
        let members: Vec<_> = self
            .in_file(&class.file_path)
            .filter(|node| {
                node.name == name
                    && node.start_line >= class.start_line
                    && node.end_line <= class.end_line
                    && node
                        .qualified_name
                        .rsplit_once("::")
                        .map(|(owner, _)| owner.rsplit([':', '.']).next().unwrap_or(owner))
                        == Some(class.name.as_str())
            })
            .collect();
        if let Some(method) = members.iter().find(|node| node.kind == "method") {
            return Ok((Some(method), true));
        }
        if members
            .iter()
            .any(|node| matches!(node.kind.as_str(), "property" | "field"))
            || depth >= 10
        {
            return Ok((None, false));
        }
        let Some(supers) = self.head_supers(class)? else {
            return Ok((None, false));
        };
        let mut complete = true;
        for written in supers {
            match self.named_type(&written, &class.file_path, &class.language, &class.id, at)? {
                Named::Outside => {}
                Named::Node(base) if base.id != class.id => {
                    let (method, known) = self.method_up(base, name, at, depth + 1)?;
                    if method.is_some() {
                        return Ok((method, known));
                    }
                    complete &= known;
                }
                _ => complete = false,
            }
        }
        Ok((None, complete))
    }
    fn aliased_type(
        &self,
        name: &str,
        file: &str,
        at: &UnresolvedReference,
        depth: usize,
    ) -> Result<Option<&'a Node>> {
        self.step()?;
        if depth >= 4 {
            return Ok(None);
        }
        let Some(alias) = self
            .in_file(file)
            .find(|node| node.name == name && node.kind == "type_alias")
        else {
            return Ok(None);
        };
        let Some(written) = self.alias_value(alias)? else {
            return Ok(None);
        };
        if self.type_parameter(alias, &written)? {
            return Ok(None);
        }
        match self.named_type(&written, &alias.file_path, &alias.language, &alias.id, at)? {
            Named::Node(node) => Ok(Some(node)),
            Named::Outside => Ok(None),
            Named::Unknown => {
                let file = self
                    .imported_file(&written, &alias.file_path, &alias.language, at)?
                    .unwrap_or_else(|| alias.file_path.clone());
                self.aliased_type(written.rsplit('.').next().unwrap(), &file, at, depth + 1)
            }
        }
    }
    pub(super) fn type_visible(&self, name: &str, at: &UnresolvedReference) -> Result<bool> {
        let file = at.file_path.as_deref().unwrap_or("");
        if self
            .imported_file(name, file, at.language.as_deref().unwrap_or(""), at)?
            .is_some()
        {
            return Ok(true);
        }
        let head = name.split('.').next().unwrap_or(name);
        Ok(self.in_file(file).any(|node| {
            node.name == head
                && matches!(
                    node.kind.as_str(),
                    "class" | "interface" | "type_alias" | "constant" | "variable"
                )
        }))
    }
    pub(super) fn known_method(
        &self,
        name: &str,
        file: &str,
        method: &str,
        site: &str,
        language: &str,
        at: &UnresolvedReference,
    ) -> Result<Option<Option<&'a Node>>> {
        let mut known = self.type_in(file, name);
        if known.is_none() {
            match self.named_type(name, site, language, "", at)? {
                Named::Node(node) => known = Some(node),
                Named::Unknown => known = self.aliased_type(name, file, at, 0)?,
                Named::Outside => {}
            }
        }
        let Some(known) = known else {
            return Ok(None);
        };
        let (method, complete) = self.method_up(known, method, at, 0)?;
        Ok(if method.is_some() || complete {
            Some(method)
        } else {
            None
        })
    }
    pub(super) fn method_of_field(
        &self,
        field: &Field,
        declaring: Option<&Node>,
        method: &str,
        at: &UnresolvedReference,
    ) -> Result<Option<&'a Node>> {
        let path = at.file_path.as_deref().unwrap_or("");
        let language = at.language.as_deref().unwrap_or("");
        let name = field
            .type_name
            .rsplit('.')
            .next()
            .unwrap_or(&field.type_name);
        if field.value_type {
            let mut holders: Vec<_> = self
                .named(name)
                .filter(|node| {
                    matches!(node.kind.as_str(), "constant" | "variable")
                        && family(&node.language, language)
                })
                .collect();
            holders.sort_by_key(|node| node.file_path != path);
            for holder in holders {
                if let Some(found) = self.objects.member(holder, method, at)? {
                    return Ok(Some(found));
                }
            }
            return Ok(None);
        }
        if !name.as_bytes().first().is_some_and(u8::is_ascii_uppercase) {
            return Ok(None);
        }
        let declaring = declaring.filter(|node| node.file_path != path);
        let site = declaring.map_or(path, |node| node.file_path.as_str());
        let lang = declaring.map_or(language, |node| node.language.as_str());
        let mut imported = self.imported_file(&field.type_name, site, lang, at)?;
        if imported.is_none() {
            match self.outside(&field.type_name, name, site, lang)? {
                Some(None) => return Ok(None),
                Some(Some(file)) => imported = Some(file),
                None => {
                    if declaring.is_some() {
                        imported = Some(site.to_string());
                    }
                }
            }
        }
        let file = imported.as_deref().unwrap_or(site);
        let mut known = self.type_in(file, name);
        if known.is_none() {
            match self.named_type(name, site, lang, "", at)? {
                Named::Node(node) => known = Some(node),
                Named::Unknown => known = self.aliased_type(name, file, at, 0)?,
                Named::Outside => {}
            }
        }
        if let Some(known) = known {
            let (method, complete) = self.method_up(known, method, at, 0)?;
            if method.is_some() || complete {
                return Ok(method);
            }
        }
        let suffix = format!("{name}::{method}");
        let mut declared: Vec<_> = self
            .named(method)
            .filter(|node| {
                node.kind == "method"
                    && family(&node.language, language)
                    && (node.qualified_name == suffix
                        || node.qualified_name.ends_with(&format!("::{suffix}")))
            })
            .collect();
        if declared.len() > 1 {
            if let Some(file) = imported.as_deref()
                && let Some(own) = declared.iter().find(|node| node.file_path == file)
            {
                return Ok(Some(own));
            }
            let dirs: Vec<_> = path.split('/').collect();
            let shared = |file: &str| {
                file.split('/')
                    .take(file.split('/').count().saturating_sub(1))
                    .zip(dirs.iter().take(dirs.len().saturating_sub(1)))
                    .take_while(|(a, b)| a == *b)
                    .count()
            };
            declared.sort_by(|a, b| {
                shared(&b.file_path)
                    .cmp(&shared(&a.file_path))
                    .then_with(|| a.file_path.encode_utf16().cmp(b.file_path.encode_utf16()))
            });
        }
        if let Some(found) = declared.first() {
            return Ok(Some(found));
        }
        // Unknown chains may still name a source-declared superclass. The known
        // chain above deliberately stops complete, absent methods before here.
        self.fallback_method(name, method, at, file, 0)
    }
    pub(super) fn fallback_method(
        &self,
        name: &str,
        method: &str,
        at: &UnresolvedReference,
        file: &str,
        depth: usize,
    ) -> Result<Option<&'a Node>> {
        self.step()?;
        let language = at.language.as_deref().unwrap_or("");
        let suffix = format!("{name}::{method}");
        let mut matches: Vec<_> = self
            .named(method)
            .filter(|node| {
                node.kind == "method"
                    && family(&node.language, language)
                    && (node.qualified_name == suffix
                        || node.qualified_name.ends_with(&format!("::{suffix}")))
            })
            .collect();
        if !matches.is_empty() {
            if let Some(node) = matches.iter().find(|node| node.file_path == file) {
                return Ok(Some(node));
            }
            matches.sort_by_key(|node| Some(node.file_path.as_str()) != at.file_path.as_deref());
            return Ok(matches.first().copied());
        }
        if depth >= 4 {
            return Ok(None);
        }
        if depth == 0 {
            let classes: Vec<_> = self
                .named(name)
                .filter(|node| node.file_path == file && OWNERS.contains(&node.kind.as_str()))
                .collect();
            let mut seen = std::collections::HashSet::new();
            for node in &classes {
                seen.insert(node.id.as_str());
            }
            for node in classes {
                if let Some(found) = self.inherited(node, method, language, &mut seen, 0)? {
                    return Ok(Some(found));
                }
            }
        }
        let mut names = Vec::new();
        for class in self
            .named(name)
            .filter(|node| node.language == language && node.kind == "class")
        {
            for base in self.supers.get(class.id.as_str()).into_iter().flatten() {
                if base.name != name && !names.contains(&base.name.as_str()) {
                    names.push(base.name.as_str());
                }
            }
        }
        for parent in names {
            if let Some(found) = self.fallback_method(parent, method, at, file, depth + 1)? {
                return Ok(Some(found));
            }
        }
        Ok(None)
    }
    fn inherited(
        &self,
        node: &Node,
        method: &str,
        language: &str,
        seen: &mut std::collections::HashSet<&'a str>,
        depth: usize,
    ) -> Result<Option<&'a Node>> {
        self.step()?;
        for base in self.supers.get(node.id.as_str()).into_iter().flatten() {
            if !seen.insert(&base.id) {
                continue;
            }
            let suffix = format!("{}::{method}", base.name);
            let own = self.named(method).find(|node| {
                node.kind == "method"
                    && family(&node.language, language)
                    && (node.qualified_name == suffix
                        || node.qualified_name.ends_with(&format!("::{suffix}")))
                    && node.file_path == base.file_path
                    && node.start_line >= base.start_line
                    && node.end_line <= base.end_line
            });
            if own.is_some() {
                return Ok(own);
            }
            if depth < 3
                && let Some(found) = self.inherited(base, method, language, seen, depth + 1)?
            {
                return Ok(Some(found));
            }
        }
        Ok(None)
    }
}
