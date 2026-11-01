//! Callable values and qualified type references from committed source.
use super::*;

impl<'a> Context<'a> {
    pub(super) fn function_ref(
        &self,
        at: &UnresolvedReference,
        scopes: &crate::frameworks::python_resolution::Bindings<'a>,
    ) -> Result<(Option<&'a Node>, f64, &'static str)> {
        let file = at.file_path.as_deref().unwrap_or("");
        let language = at.language.as_deref().unwrap_or("");
        let root = at.reference_name.split('.').next().unwrap_or("");
        let import_binding = self.imported(file, root);
        let mut bound_target = None;
        if let Some(binding) = import_binding {
            bound_target =
                (self.resolve_import)(at)?.and_then(|id| self.ids.get(id.as_str()).copied());
            if let Some(target) = bound_target
                && matches!(target.kind.as_str(), "function" | "method")
                && family(&target.language, language)
            {
                return Ok((Some(target), 0.9, "import"));
            }
            let wanted = if binding.is_namespace {
                at.reference_name.strip_prefix(&format!("{root}."))
            } else {
                Some(binding.imported_name.as_str())
            };
            if let Some(target) =
                self.ambient_export(&binding.source, wanted, file, "function_ref")?
                && matches!(target.kind.as_str(), "function" | "method")
            {
                return Ok((Some(target), 0.9, "import"));
            }
        }
        let named: Vec<_> = self
            .named(&at.reference_name)
            .filter(|node| {
                node.kind == "function"
                    && node.id != at.from_node_id
                    && family(&node.language, language)
            })
            .collect();
        self.steps(named.len())?;
        let mut candidates = Vec::new();
        for node in &named {
            if scopes.lexically_reachable(node, at)? {
                candidates.push(*node);
            }
        }
        if let Some(owner) = self.ids.get(at.from_node_id.as_str()).copied()
            && matches!(owner.kind.as_str(), "function" | "method")
            && self.function_binds(&at.reference_name, owner, at.line)?
        {
            candidates.retain(|node| {
                node.file_path == file
                    && node.start_line >= owner.start_line
                    && node.start_line <= owner.end_line
            });
        }
        let own: Vec<_> = candidates
            .iter()
            .copied()
            .filter(|node| node.file_path == file)
            .collect();
        if !own.is_empty() {
            return Ok((
                own.iter().copied().min_by_key(|node| node.start_line),
                if own.len() == 1 { 0.95 } else { 0.9 },
                "function-ref",
            ));
        }
        if let Some(binding) = import_binding {
            let source = binding.source.split(['?', '#']).next().unwrap_or("");
            let asset = test(
                r"(?i)\.(?:css|scss|sass|less|styl|stylus|pcss|graphql|gql|png|jpe?g|gif|svg|webp|avif|ico|bmp|tiff?|json5?|ya?ml|toml|md|mdx|txt|html?|xml|csv|woff2?|ttf|otf|eot|mp3|mp4|webm|ogg|wav|flac|pdf|wasm)$",
                source,
            )?;
            let other_value = if let Some(bound) = bound_target {
                let beside = candidates
                    .iter()
                    .any(|candidate| candidate.file_path == bound.file_path);
                let head = self
                    .lines(&bound.file_path)?
                    .skip(bound.start_line.saturating_sub(1) as usize)
                    .take(
                        (bound.end_line.min(bound.start_line + 15) - bound.start_line + 1) as usize,
                    )
                    .collect::<Vec<_>>()
                    .join("\n");
                !beside
                    && !matches!(bound.kind.as_str(), "function" | "method")
                    && !test(r"\bimport\s*\(", &head)?
            } else {
                false
            };
            if binding.external || binding.is_namespace || asset || other_value {
                return Ok((None, 0.0, "function-ref"));
            }
        }
        Ok((
            if candidates.len() == 1 && named.len() == 1 {
                Some(candidates[0])
            } else {
                None
            },
            0.8,
            "function-ref",
        ))
    }

    fn function_binds(&self, name: &str, owner: &Node, line: u32) -> Result<bool> {
        if !test(r"^[A-Za-z_$][\w$]*$", name)? {
            return Ok(false);
        }
        let text = self
            .lines(&owner.file_path)?
            .skip(owner.start_line.saturating_sub(1) as usize)
            .take(line.min(owner.end_line).saturating_sub(owner.start_line) as usize + 1)
            .collect::<Vec<_>>()
            .join("\n");
        self.steps(text.len() / 64 + 1)?;
        let text = crate::synthesis::comments::strip_for_regex(&text, "javascript");
        let name = ::regex::escape(name);
        if test(
            &format!(r"\b(?:const|let|var)\s+{name}(?![\w$])(?!\s*[,\]}}])"),
            &text,
        )? {
            return Ok(true);
        }
        let paren = r"\((?:[^()]|\([^()]*\))*\)";
        let brace = r"\{(?:[^{}]|\{[^{}]*\})*\}";
        let bracket = r"\[(?:[^\[\]]|\[[^\[\]]*\])*\]";
        let angle = r"<(?!\s)(?:=>|=(?!>)|[^<>=]|<(?:=>|=(?!>)|[^<>=])*>)*>";
        let part = format!(
            r"(?:=>|=(?!>)|[^,(){{}}\[\]<>=]|<(?=\s)|>(?=[\s=])|{paren}|{brace}|{bracket}|{angle})"
        );
        let modifiers = r"(?:(?:public|private|protected|readonly|override)\s+)*";
        let pattern = format!(
            r"\(\s*(?:(?:{modifiers}(?:\.\.\.)?[\w$]+|{brace}|{bracket})(?:\s*(?:\?\s*)?:{part}+|\s*={part}+|\s*),\s*)*{modifiers}{name}(?![\w$])(?:\s*\??\s*:{part}*)?(?:\s*={part}*)?(?:\s*,(?:[^()]|{paren})*)?\)\s*(?::(?:[^=;{{}}()\n]|{brace})*)?(?:=>|\{{)"
        );
        for found in regex(&pattern)?.find_iter(&text) {
            let found = found?;
            if !test(
                r"\b(?:if|while|for|switch|with)\s*$",
                &text[..found.start()],
            )? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub(super) fn import_or_type(&self, at: &UnresolvedReference) -> Result<Option<Binding>> {
        if !matches!(
            at.reference_kind.as_str(),
            "references" | "imports" | "calls" | "instantiates"
        ) {
            return Ok(None);
        }
        let file = at.file_path.as_deref().unwrap_or("");
        let mut name = at.reference_name.clone();
        if at.reference_kind == "references" {
            // A generic parameter shadows a project declaration of the same name.
            for owner in self.in_file(file).filter(|node| {
                matches!(node.kind.as_str(), "class" | "interface" | "type_alias")
                    && (node.start_line, node.start_column) <= (at.line, at.column)
                    && (node.end_line, node.end_column) >= (at.line, at.column)
            }) {
                if self.type_parameter(owner, &name)? {
                    return Ok(Some(Binding {
                        reference: at.clone(),
                        target: None,
                        confidence: 0.0,
                        by_name: false,
                        resolved_by: "exact-match",
                    }));
                }
            }
            let line = self
                .lines(file)?
                .nth(at.line.saturating_sub(1) as usize)
                .unwrap_or("");
            let start = byte(line, at.column);
            if line[start..].starts_with(&name)
                && let Some(qualifier) =
                    regex(r"([A-Za-z_$][\w$]*(?:[ \t]*\.[ \t]*[A-Za-z_$][\w$]*)*)[ \t]*\.[ \t]*$")?
                        .captures(&line[..start])?
            {
                name = format!("{}.{}", qualifier[1].replace([' ', '\t'], ""), name);
            }
        }
        let root = name.split('.').next().unwrap_or("");
        let binding = self.imported(file, root);
        let mut target = None;
        let mut claimed = false;
        if let Some(binding) = binding {
            if binding.is_namespace && name == root && at.reference_kind == "imports" {
                target = binding
                    .resolved_path
                    .as_deref()
                    .and_then(|path| self.in_file(path).find(|node| node.kind == "file"));
                claimed = target.is_some();
            } else if name.contains('.') && at.reference_kind == "references" {
                let mut qualified = at.clone();
                qualified.reference_name = name.clone();
                target = (self.resolve_import)(&qualified)?
                    .and_then(|id| self.ids.get(id.as_str()).copied());
                claimed = true;
            }
            if target.is_none() {
                let wanted = if binding.is_default && at.reference_kind == "imports" {
                    None
                } else if binding.is_namespace {
                    name.strip_prefix(&format!("{root}."))
                } else {
                    Some(binding.imported_name.as_str())
                };
                target = self.ambient_export(&binding.source, wanted, file, &at.reference_kind)?;
                claimed |= target.is_some();
            }
        } else if at.reference_kind == "imports" {
            target = self.ambient_export(&name, None, file, &at.reference_kind)?;
            claimed = target.is_some();
        }
        Ok(claimed.then(|| Binding {
            reference: at.clone(),
            target: target.map(|node| node.id.clone()),
            confidence: 0.9,
            by_name: false,
            resolved_by: "import",
        }))
    }

    fn ambient_export(
        &self,
        specifier: &str,
        wanted: Option<&str>,
        from: &str,
        kind: &str,
    ) -> Result<Option<&'a Node>> {
        let mut choices = Vec::new();
        let from_parts: Vec<_> = from.split('/').collect();
        for (&path, &source) in self.sources {
            if !(path.ends_with(".d.ts") || path.ends_with(".d.mts") || path.ends_with(".d.cts"))
                || (!crate::classify::is_test_path(from) && crate::classify::is_test_path(path))
            {
                continue;
            }
            if !self.ambient.borrow().contains_key(path) {
                self.steps(source.len() / 64 + 1)?;
                let blocks = if ambient::script(source) {
                    ambient::scan(source)
                } else {
                    Vec::new()
                };
                self.ambient.borrow_mut().insert(path.to_string(), blocks);
            }
            let blocks = self.ambient.borrow().get(path).cloned().unwrap_or_default();
            for block in blocks.iter().filter(|block| block.specifier == specifier) {
                let exports = ambient::exports(source, block);
                let nodes: Vec<_> = self
                    .in_file(path)
                    .filter(|node| {
                        wanted.map_or(node.kind == "file", |name| {
                            node.name == exports.get(name).map_or(name, String::as_str)
                                && (exports.contains_key(name) || node.is_exported.unwrap_or(false))
                                && (node.start_line, node.start_column)
                                    >= (block.start_line, block.start_column)
                                && (node.end_line, node.end_column)
                                    <= (block.end_line, block.end_column)
                        })
                    })
                    .collect();
                let near = from_parts
                    .iter()
                    .zip(path.split('/'))
                    .take_while(|(left, right)| **left == *right)
                    .count();
                for node in nodes {
                    let rank = if kind == "references" {
                        usize::from(!matches!(
                            node.kind.as_str(),
                            "interface" | "class" | "type_alias" | "enum"
                        ))
                    } else {
                        0
                    };
                    choices.push((
                        crate::classify::is_test_path(path),
                        std::cmp::Reverse(near),
                        path,
                        rank,
                        node.start_line,
                        node.start_column,
                        node,
                    ));
                }
            }
        }
        choices.sort_by(|a, b| (a.0, a.1, a.2, a.3, a.4, a.5).cmp(&(b.0, b.1, b.2, b.3, b.4, b.5)));
        Ok(choices.first().map(|choice| choice.6))
    }
}
