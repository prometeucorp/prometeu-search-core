use super::*;

impl<'a> Typed<'a, '_> {
    fn module_class(&self, module: &str, name: &str, from: &str, kind: &str) -> Option<&'a Node> {
        let dots = module.bytes().take_while(|byte| *byte == b'.').count();
        let mut relative = module[dots..].replace('.', "/");
        if dots > 0 {
            let normalized = from.replace('\\', "/");
            let mut dirs = normalized.split('/').collect::<Vec<_>>();
            dirs.pop();
            for _ in 1..dots {
                dirs.pop()?;
            }
            if !relative.is_empty() {
                dirs.push(&relative)
            }
            relative = dirs.join("/");
        }
        if relative.is_empty() {
            return None;
        }
        let wanted = [
            format!("{relative}.py"),
            format!("{relative}/__init__.py"),
            format!("{relative}.pyi"),
        ];
        let mut hits = self.named(name).iter().copied().filter(|node| {
            node.kind == kind
                && (kind != "function" || node.qualified_name == node.name)
                && wanted.iter().any(|wanted| {
                    let path = node.file_path.replace('\\', "/");
                    path == *wanted || dots == 0 && path.ends_with(&format!("/{wanted}"))
                })
        });
        let first = hits.next()?;
        hits.next().is_none().then_some(first)
    }
    pub(in super::super) fn class_named(
        &self,
        name: &str,
        path: &str,
        kind: &str,
    ) -> Result<Option<&'a Node>> {
        let parts = name.split('.').collect::<Vec<_>>();
        let last = parts.last().copied().unwrap_or_default();
        let mut candidates = Vec::new();
        for mapping in self.mappings.get(path).into_iter().flatten() {
            self.charge(1)?;
            if mapping.is_namespace
                && parts.len() > 1
                && name.starts_with(&format!("{}.", mapping.source))
            {
                let suffix = &name[mapping.source.len() + 1..];
                let mut segments = suffix.split('.').collect::<Vec<_>>();
                segments.pop();
                let mut module = mapping.source.clone();
                for segment in segments {
                    module.push('.');
                    module.push_str(segment)
                }
                candidates.push(self.module_class(&module, last, path, kind));
            } else if mapping.local_name == parts[0] {
                if parts.len() == 1 {
                    candidates.push(if mapping.is_namespace {
                        None
                    } else {
                        self.module_class(&mapping.source, &mapping.exported_name, path, kind)
                    });
                } else {
                    let mut base = if mapping.is_namespace {
                        mapping.source.clone()
                    } else {
                        format!(
                            "{}{}{}",
                            mapping.source,
                            if mapping.source.ends_with('.') {
                                ""
                            } else {
                                "."
                            },
                            mapping.exported_name
                        )
                    };
                    for segment in &parts[1..parts.len() - 1] {
                        base.push('.');
                        base.push_str(segment)
                    }
                    candidates.push(self.module_class(&base, last, path, kind));
                }
            }
        }
        if let Some(first) = candidates.first() {
            return Ok(first.filter(|first| {
                candidates
                    .iter()
                    .all(|candidate| candidate.is_some_and(|node| node.id == first.id))
            }));
        }
        if parts.len() > 1 {
            return Ok(None);
        }
        let mut hits = self.named(name).iter().copied().filter(|node| {
            node.file_path == path
                && node.kind == kind
                && (kind != "function" || node.qualified_name == node.name)
        });
        Ok(hits.next().filter(|_| hits.next().is_none()))
    }
    pub(super) fn base_names(&self, class: &Node) -> Result<Vec<String>> {
        let lines = self.line_text(&class.file_path);
        let header = lines
            .iter()
            .skip(class.start_line as usize - 1)
            .take(5)
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(" ");
        let Some(capture) = regex(&format!(
            r"\bclass\s+{}\s*\(([^)]*)\)",
            regex::escape(&class.name)
        ))?
        .captures(&header)?
        else {
            return Ok(Vec::new());
        };
        let mut names = Vec::new();
        for name in capture[1].split(',').map(trim) {
            if name != "object" && regex(r"^[A-Za-z_][\w.]*$")?.is_match(name)? {
                names.push(name.to_owned())
            }
        }
        Ok(names)
    }
    pub(in super::super) fn class_bases(&self, class: &'a Node) -> Result<Vec<Option<&'a Node>>> {
        if let Some(cached) = self.bases.borrow().get(class.id.as_str()) {
            return Ok(cached.clone());
        }
        let bases = self
            .base_names(class)?
            .iter()
            .map(|name| self.class_named(name, &class.file_path, "class"))
            .collect::<Result<Vec<_>>>()?;
        self.bases.borrow_mut().insert(&class.id, bases.clone());
        Ok(bases)
    }
    pub(super) fn hierarchy_known(
        &self,
        class: &'a Node,
        seen: &mut HashSet<&'a str>,
    ) -> Result<bool> {
        if seen.len() >= 5 || !seen.insert(&class.id) {
            return Ok(true);
        }
        let bases = self.class_bases(class)?;
        for (name, base) in self.base_names(class)?.iter().zip(bases) {
            if match base {
                Some(base) => !self.hierarchy_known(base, seen)?,
                None => !self
                    .outside_name(name.split('.').next().unwrap_or_default(), &class.file_path)?,
            } {
                return Ok(false);
            }
        }
        Ok(true)
    }
    pub(super) fn find_method(
        &self,
        class: &'a Node,
        name: &str,
        read: &mut Read,
        seen: &mut HashSet<&'a str>,
    ) -> Result<Option<(&'a Node, bool)>> {
        self.charge(1)?;
        if seen.len() >= 5 || !seen.insert(&class.id) {
            return Ok(None);
        }
        let qualified = format!("{}::{name}", class.name);
        if let Some(method) = self.named(name).iter().copied().find(|node| {
            node.kind == "method"
                && node.file_path == class.file_path
                && (node.qualified_name == qualified
                    || node.qualified_name.ends_with(&format!("::{qualified}")))
        }) {
            return Ok(Some((method, false)));
        }
        read.insert(class.file_path.clone());
        for base in self.class_bases(class)?.into_iter().flatten() {
            if let Some((method, _)) = self.find_method(base, name, read, seen)? {
                return Ok(Some((method, true)));
            }
        }
        Ok(None)
    }
    pub(super) fn method(
        &self,
        class: &'a Node,
        name: &str,
        read: &mut Read,
    ) -> Result<Option<Answer<'a>>> {
        Ok(self
            .find_method(class, name, read, &mut HashSet::new())?
            .map(|(node, inherited)| Answer {
                node,
                confidence: if inherited { 0.8 } else { 0.85 },
                resolved_by: "instance-method",
                metadata: None,
            }))
    }
    pub(super) fn subclass_method(
        &self,
        class: &'a Node,
        name: &str,
        reference: &UnresolvedReference,
    ) -> Result<Option<Answer<'a>>> {
        let mut descendants = Vec::new();
        for method in self
            .named(name)
            .iter()
            .copied()
            .filter(|node| node.kind == "method")
        {
            self.charge(1)?;
            let parent = self
                .in_file(&method.file_path)
                .iter()
                .copied()
                .find(|node| {
                    node.kind == "class"
                        && method.qualified_name == format!("{}::{name}", node.qualified_name)
                });
            if let Some(parent) = parent
                && self.derives(parent, class, reference, &mut HashSet::new())?
            {
                descendants.push(method)
            }
        }
        Ok((descendants.len() == 1).then(|| Answer {
            node: descendants[0],
            confidence: 0.8,
            resolved_by: "instance-method",
            metadata: None,
        }))
    }
    fn derives(
        &self,
        class: &'a Node,
        base: &Node,
        reference: &UnresolvedReference,
        seen: &mut HashSet<&'a str>,
    ) -> Result<bool> {
        self.charge(1)?;
        if seen.len() >= 16 || !seen.insert(&class.id) {
            return Ok(false);
        }
        let Some(mut list) = self.base_list(class)? else {
            return Ok(false);
        };
        let brackets = regex(r"\[[^[\]]*\]")?;
        while brackets.is_match(&list)? {
            list = brackets.try_replacen(&list, 0, "")?.into_owned()
        }
        for name in list.split(',').map(trim) {
            if let Some(parent) = self.ref_class(name, &class.file_path, reference)?
                && (parent.id == base.id || self.derives(parent, base, reference, seen)?)
            {
                return Ok(true);
            }
        }
        Ok(false)
    }
    fn ref_class(
        &self,
        name: &str,
        path: &str,
        reference: &UnresolvedReference,
    ) -> Result<Option<&'a Node>> {
        let mappings = self
            .mappings
            .get(path)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let mut name = name.to_owned();
        for mapping in mappings.iter().filter(|mapping| mapping.is_namespace) {
            if let Some(tail) = name.strip_prefix(&format!("{}.", mapping.source))
                && regex(r"^\w+$")?.is_match(tail)?
            {
                if mappings
                    .iter()
                    .filter(|other| other.local_name == mapping.local_name)
                    .count()
                    != 1
                {
                    return Ok(None);
                }
                name = format!("{}.{tail}", mapping.local_name);
                break;
            }
        }
        if mappings
            .iter()
            .any(|mapping| Some(mapping.local_name.as_str()) == name.split('.').next())
        {
            let mut raw = reference.clone();
            raw.file_path = Some(path.to_owned());
            raw.reference_name = name;
            raw.reference_kind = "references".into();
            let hit = self.imports.resolve(&raw)?;
            return Ok(hit.map(|hit| hit.node).filter(|node| {
                node.kind == "class"
                    && self
                        .in_file(&node.file_path)
                        .iter()
                        .filter(|other| {
                            other.kind == "class" && other.qualified_name == node.qualified_name
                        })
                        .count()
                        == 1
            }));
        }
        let mut hits = self
            .named(&name)
            .iter()
            .copied()
            .filter(|node| node.kind == "class" && node.file_path == path);
        Ok(hits.next().filter(|_| hits.next().is_none()))
    }
    fn base_list(&self, class: &Node) -> Result<Option<String>> {
        let Some(lines) = self.original.get(class.file_path.as_str()) else {
            return Ok(None);
        };
        let Some(head) = regex(r"^\s*class\s+\w+")?.find(
            lines
                .get(class.start_line as usize - 1)
                .copied()
                .unwrap_or_default(),
        )?
        else {
            return Ok(None);
        };
        let mut list = String::new();
        let mut depth = 0i32;
        let mut params = 0i32;
        for at in class.start_line as usize - 1..class.end_line.max(class.start_line) as usize {
            self.charge(1)?;
            let text = lines.get(at).copied().unwrap_or_default();
            let text = if at == class.start_line as usize - 1 {
                &text[head.end()..]
            } else {
                text
            };
            for ch in text.split('#').next().unwrap_or_default().chars() {
                if depth == 0 {
                    match ch {
                        '[' => params += 1,
                        ']' => params -= 1,
                        '(' if params == 0 => depth = 1,
                        _ if params == 0 && !crate::search::query_whitespace(ch) => {
                            return Ok(None);
                        }
                        _ => {}
                    }
                    continue;
                }
                if ch == '(' {
                    depth += 1
                } else if ch == ')' {
                    depth -= 1;
                    if depth == 0 {
                        return Ok(Some(list));
                    }
                }
                list.push(ch);
            }
            if depth > 0 {
                list.push(' ')
            }
        }
        Ok(None)
    }
    pub(in super::super) fn infer_local(
        &self,
        name: &str,
        reference: &UnresolvedReference,
    ) -> Result<Option<String>> {
        let path = reference.file_path.as_deref().unwrap_or_default();
        let Some(lines) = self.original.get(path) else {
            return Ok(None);
        };
        if lines.is_empty() {
            return Ok(None);
        }
        let start = self
            .in_file(path)
            .iter()
            .filter(|node| {
                matches!(node.kind.as_str(), "function" | "method")
                    && node.start_line <= reference.line
                    && node.end_line >= reference.line
            })
            .map(|node| node.start_line.saturating_sub(1) as usize)
            .max()
            .unwrap_or(0);
        let name = regex::escape(name);
        let patterns=[format!(r"(?:^|;)\s*{name}\s*=(?!=)\s*([A-Z]\w*)\.objects\.(?:create|get|first|last|latest|earliest|get_by_natural_key)\s*\("),format!(r"(?:^|;)\s*{name}\s*=(?!=)\s*([A-Z][\w.]*)\s*\((?![^\n]*,\s*$)"),format!(r#"\b{name}\b\s*:\s*["']([A-Z][\w.]*)["']"#),format!(r"\b{name}\b\s*:\s*([A-Z][\w.]*)")].into_iter().map(|pattern|regex(&pattern)).collect::<Result<Vec<_>>>()?;
        let end = reference.line.saturating_sub(1) as usize;
        for at in (start..=end.min(lines.len() - 1)).rev() {
            self.charge(1)?;
            let line = lines[at];
            if line.encode_utf16().count() > 10_000 {
                continue;
            }
            for pattern in &patterns {
                if let Some(capture) = pattern.captures(line)? {
                    return Ok(Some(
                        capture[1].rsplit('.').next().unwrap_or_default().to_owned(),
                    ));
                }
            }
        }
        Ok(None)
    }
}
