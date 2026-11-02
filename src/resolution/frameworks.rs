//! Framework references whose spelling is a route binding rather than a symbol name.
use super::super::Graph;
use crate::parser::Reference;
use std::collections::HashSet;

fn swift_type(kind: &str) -> bool {
    matches!(kind, "class" | "struct" | "enum" | "protocol")
}

impl Graph {
    fn framework_nodes_named(&self, name: &str) -> Vec<&prometeu_extract::Node> {
        let mut nodes: Vec<_> = self
            .names
            .get(name)
            .into_iter()
            .flatten()
            .filter_map(|&index| self.metadata(index))
            .filter(|node| node.name == name)
            .collect();
        nodes.sort_by(|a, b| {
            (&a.file_path, a.start_line, &a.id).cmp(&(&b.file_path, b.start_line, &b.id))
        });
        nodes
    }
    fn framework_nodes_in_file(&self, file: &str) -> Vec<&prometeu_extract::Node> {
        let mut nodes: Vec<_> = self
            .files
            .get(file)
            .into_iter()
            .flat_map(|file| &file.nodes)
            .filter_map(|&index| self.metadata(index))
            .collect();
        nodes.sort_by(|a, b| (a.start_line, &a.id).cmp(&(b.start_line, &b.id)));
        nodes
    }
    pub(super) fn laravel_reference(
        &self,
        from: usize,
        name: &str,
        kind: &str,
    ) -> Option<(usize, f64)> {
        let path = &self.nodes[from].path;
        if self.files[path].analysis.language != "php"
            || !self.frameworks.iter().any(|name| name == "laravel")
        {
            return None;
        }
        let origin = self.metadata(from)?;
        let resolved = match crate::frameworks::laravel::resolve(
            &origin.id,
            name,
            kind,
            |name| self.framework_nodes_named(name),
            |path| self.framework_nodes_in_file(path),
        ) {
            Ok(resolved) => resolved,
            Err(_) => {
                self.resolution_truncated.set(true);
                return None;
            }
        }?;
        self.files[&resolved.0.file_path]
            .nodes
            .iter()
            .copied()
            .find(|&index| {
                self.metadata(index)
                    .is_some_and(|node| node.id == resolved.0.id)
            })
            .map(|index| (index, resolved.1))
    }

    pub(super) fn rails_reference(&self, path: &str, name: &str) -> Option<(usize, f64)> {
        if self.files[path].analysis.language != "ruby"
            || !self.frameworks.iter().any(|name| name == "rails")
        {
            return None;
        }
        let resolved = match crate::frameworks::rails::resolve(
            name,
            |path| self.files.contains_key(path),
            |name| self.framework_nodes_named(name),
            |path| self.framework_nodes_in_file(path),
        ) {
            Ok(resolved) => resolved,
            Err(_) => {
                self.resolution_truncated.set(true);
                return None;
            }
        }?;
        self.files[&resolved.0.file_path]
            .nodes
            .iter()
            .copied()
            .find(|&index| {
                self.metadata(index)
                    .is_some_and(|node| node.id == resolved.0.id)
            })
            .map(|index| (index, resolved.1))
    }
    pub(super) fn nest_reference(&self, path: &str, name: &str) -> Option<(usize, f64)> {
        if !matches!(
            self.files[path].analysis.language.as_str(),
            "typescript" | "javascript"
        ) || !self.frameworks.iter().any(|name| name == "nestjs")
        {
            return None;
        }
        let resolved = match crate::frameworks::nestjs::resolve(path, name, |name| {
            self.framework_nodes_named(name)
        }) {
            Ok(resolved) => resolved,
            Err(_) => {
                self.resolution_truncated.set(true);
                return None;
            }
        }?;
        self.files[&resolved.0.file_path]
            .nodes
            .iter()
            .copied()
            .find(|&index| {
                self.metadata(index)
                    .is_some_and(|node| node.id == resolved.0.id)
            })
            .map(|index| (index, resolved.1))
    }
    pub(super) fn express_reference(&self, path: &str, name: &str) -> Option<(usize, f64)> {
        if !matches!(
            self.files[path].analysis.language.as_str(),
            "javascript" | "typescript"
        ) || !self.frameworks.iter().any(|name| name == "express")
        {
            return None;
        }
        let resolved = match crate::frameworks::express::resolve(
            path,
            name,
            |name| self.framework_nodes_named(name),
            |path| self.framework_nodes_in_file(path),
        ) {
            Ok(resolved) => resolved,
            Err(_) => {
                self.resolution_truncated.set(true);
                return None;
            }
        }?;
        self.files[path]
            .nodes
            .iter()
            .copied()
            .find(|&index| {
                self.metadata(index)
                    .is_some_and(|node| node.id == resolved.0.id)
            })
            .map(|index| (index, resolved.1))
    }

    pub(super) fn framework_reference(
        &self,
        path: &str,
        caller: usize,
        reference: &Reference,
    ) -> Option<(Vec<usize>, &'static str)> {
        let separator = if self.files[path].analysis.language == "php" {
            "::"
        } else {
            "."
        };
        let name = reference.qualifier.as_ref().map_or_else(
            || reference.target.clone(),
            |qualifier| format!("{qualifier}{separator}{}", reference.target),
        );
        let name = self
            .canonical_reference(caller, reference)
            .map_or(name, |raw| raw.reference_name.clone());
        if let Some((target, _)) = self.laravel_reference(caller, &name, &reference.kind) {
            return Some((vec![target], "framework_match"));
        }
        if let Some(target) = self.cached_framework_reference(
            caller,
            &name,
            &reference.kind,
            reference.line,
            reference.column,
        ) {
            let method = match target
                .first()
                .and_then(|&to| {
                    self.cached_framework_metadata(
                        caller,
                        to,
                        &name,
                        reference.line,
                        reference.column.map(|column| column as u32),
                    )
                })
                .map(|metadata| {
                    if metadata.by_name {
                        "exact-match"
                    } else {
                        metadata.resolved_by
                    }
                }) {
                Some("exact-match") => "name_match",
                Some("import") => "import_match",
                Some("instance-method") => "typed_receiver_match",
                Some("function-ref") => "function_ref_match",
                _ => "framework_match",
            };
            return Some((target.to_vec(), method));
        }
        if self.frameworks.iter().any(|name| name == "play")
            && crate::frameworks::play::is_routes(path)
        {
            match crate::frameworks::play::claims(&name) {
                Ok(true) => return Some((Vec::new(), "framework_match")),
                Err(_) => {
                    self.resolution_truncated.set(true);
                    return Some((Vec::new(), "framework_match"));
                }
                Ok(false) => {}
            }
        }
        if matches!(self.files[path].analysis.language.as_str(), "php" | "yaml")
            && self.frameworks.iter().any(|name| name == "drupal")
            && (name.starts_with("drupal:")
                || name.contains("container->get(")
                || name
                    .trim_start_matches('\\')
                    .starts_with("Drupal::service()"))
        {
            match crate::frameworks::drupal::claims(&name) {
                Ok(true) => return Some((Vec::new(), "framework_match")),
                Err(_) => {
                    self.resolution_truncated.set(true);
                    return Some((Vec::new(), "framework_match"));
                }
                Ok(false) => {}
            }
        }
        if self.files[path].analysis.language == "php"
            && self.frameworks.iter().any(|name| name == "codeigniter")
        {
            match crate::frameworks::codeigniter::claims(&name) {
                Ok(true) => return Some((Vec::new(), "framework_match")),
                Err(_) => {
                    self.resolution_truncated.set(true);
                    return Some((Vec::new(), "framework_match"));
                }
                Ok(false) => {}
            }
        }
        if let Some((target, _)) = self.express_reference(path, &name) {
            return Some((vec![target], "framework_match"));
        }
        if let Some((target, _)) = self.nest_reference(path, &name) {
            return Some((vec![target], "framework_match"));
        }
        if let Some((target, _)) = self.rails_reference(path, &name) {
            return Some((vec![target], "framework_match"));
        }
        if self.files[path].analysis.language == "ruby"
            && self.frameworks.iter().any(|name| name == "rails")
            && crate::frameworks::rails::claims(&name).unwrap_or(false)
        {
            return Some((Vec::new(), "framework_match"));
        }
        if reference.kind == "calls"
            && matches!(self.files[path].analysis.language.as_str(), "vue" | "astro")
            && self.nodes[caller].symbol.kind == "route"
            && let Some(component) = self.files[path].nodes.iter().copied().find(|&index| {
                let node = &self.nodes[index].symbol;
                node.kind == "component" && node.name == reference.target
            })
        {
            return Some((vec![component], "framework_match"));
        }
        if let Some(target) = self.react_reference(
            caller,
            &name,
            &reference.kind,
            reference.line,
            reference.column,
        ) {
            return Some((vec![target], "framework_match"));
        }
        if self.react_claims(caller, &name) {
            return Some((Vec::new(), "framework_match"));
        }
        if let Some((targets, _)) = self.component_reference(path, caller, reference) {
            return Some((targets, "framework_match"));
        }
        if (self.files[path].analysis.language == "csharp"
            || (matches!(
                self.files[path].analysis.language.as_str(),
                "typescript" | "javascript"
            ) && self.frameworks.iter().any(|name| name == "nestjs")))
            && reference.kind == "references"
            && reference.qualifier.is_none()
            && self.nodes[caller].symbol.kind == "route"
        {
            let owner = self.files[path]
                .nodes
                .iter()
                .copied()
                .filter(|&index| {
                    let node = &self.nodes[index].symbol;
                    matches!(node.kind.as_str(), "class" | "struct")
                        && node.start_line <= reference.line
                        && node.end_line >= reference.line
                })
                .max_by_key(|&index| self.nodes[index].symbol.start_line);
            let candidates: Vec<_> = self
                .named(path, &reference.target)
                .into_iter()
                .filter(|&index| {
                    let node = &self.nodes[index];
                    node.path == path
                        && matches!(node.symbol.kind.as_str(), "method" | "function")
                        && owner.is_none_or(|owner| {
                            let owner = &self.nodes[owner].symbol;
                            node.symbol.start_line >= owner.start_line
                                && node.symbol.end_line <= owner.end_line
                        })
                })
                .collect();
            if !candidates.is_empty() {
                return Some((candidates, "scope_match"));
            }
        }
        if matches!(
            self.files[path].analysis.language.as_str(),
            "java" | "kotlin"
        ) {
            return self.spring_reference(path, caller, reference);
        }
        if self.files[path].analysis.language != "swift" {
            return None;
        }
        let name = reference.qualifier.as_ref().map_or_else(
            || reference.target.clone(),
            |qualifier| format!("{qualifier}.{}", reference.target),
        );
        let pattern = regex::Regex::new(
            r"^((?:[A-Za-z_][A-Za-z0-9_]*\.)*[A-Za-z_][A-Za-z0-9_]*)?@([A-Za-z_][A-Za-z0-9_]*)$",
        )
        .unwrap();
        let captures = pattern.captures(&name)?;
        let found = self.vapor_handler(
            captures.get(1).map(|m| m.as_str()),
            &captures[2],
            path,
            reference.line,
        );
        Some((found.into_iter().collect(), "framework_match"))
    }

    fn spring_reference(
        &self,
        path: &str,
        caller: usize,
        reference: &Reference,
    ) -> Option<(Vec<usize>, &'static str)> {
        if reference.kind != "references" {
            return None;
        }
        let origin = self.metadata(caller)?;
        if origin
            .id
            .starts_with(&format!("route:{path}:{}:", reference.line))
        {
            let handler = self.files[path]
                .nodes
                .iter()
                .copied()
                .filter(|&index| {
                    let node = &self.nodes[index].symbol;
                    matches!(node.kind.as_str(), "method" | "function")
                        && node.name == reference.target
                        && node.start_line <= reference.line
                        && node.end_line >= reference.line
                })
                .max_by_key(|&index| self.nodes[index].symbol.start_line);
            if let Some(handler) = handler {
                return Some((vec![handler], "framework_match"));
            }
        }
        if !origin.id.starts_with("spring-value:") && !origin.id.starts_with("spring-cp:") {
            return None;
        }
        let name = reference.qualifier.as_ref().map_or_else(
            || reference.target.clone(),
            |qualifier| format!("{qualifier}.{}", reference.target),
        );
        let prefix = name.strip_suffix(":prefix");
        let canonical = |key: &str| key.to_lowercase().replace(['-', '_'], "");
        let wanted = canonical(prefix.unwrap_or(&name));
        let mut candidates: Vec<_> = self
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| {
                if node.symbol.kind != "constant"
                    || !matches!(
                        self.files[&node.path].analysis.language.as_str(),
                        "yaml" | "properties"
                    )
                {
                    return false;
                }
                let key = canonical(&node.symbol.qualified_name);
                if prefix.is_some() {
                    key.starts_with(&wanted)
                } else {
                    key == wanted
                }
            })
            .map(|(index, _)| index)
            .collect();
        if candidates.is_empty() {
            return Some((Vec::new(), "framework_match"));
        }
        // A profile is selected at runtime. Keep equally plausible configuration
        // bindings explicit instead of choosing the first declaration in the cache.
        if prefix.is_some() {
            let shortest = candidates
                .iter()
                .map(|&index| {
                    canonical(&self.nodes[index].symbol.qualified_name)
                        .encode_utf16()
                        .count()
                })
                .min()
                .unwrap();
            candidates.retain(|&index| {
                canonical(&self.nodes[index].symbol.qualified_name)
                    .encode_utf16()
                    .count()
                    == shortest
            });
        }
        Some((candidates, "framework_match"))
    }

    fn swift_method_owner(&self, index: usize, method: &str) -> &str {
        let name = &self.nodes[index].symbol.qualified_name;
        name.get(..name.len().saturating_sub(method.len() + 2))
            .unwrap_or("")
    }

    fn swift_one_owner(&self, candidates: Vec<usize>, method: &str) -> Option<usize> {
        if candidates.is_empty()
            || candidates
                .iter()
                .map(|&index| self.swift_method_owner(index, method))
                .collect::<HashSet<_>>()
                .len()
                > 1
        {
            return None;
        }
        if candidates.len() == 1 {
            return candidates.first().copied();
        }
        let request =
            regex::Regex::new(r"\(\s*(?:[A-Za-z0-9_]+\s+)?[A-Za-z0-9_]+\s*:\s*Request\b").unwrap();
        let preferred: Vec<_> = candidates
            .iter()
            .copied()
            .filter(|&index| {
                let node = &self.nodes[index];
                let head = self.files[&node.path]
                    .source
                    .lines()
                    .skip(node.symbol.start_line.saturating_sub(1))
                    .take(
                        node.symbol
                            .end_line
                            .min(node.symbol.start_line + 2)
                            .saturating_sub(node.symbol.start_line)
                            + 1,
                    )
                    .collect::<Vec<_>>()
                    .join(" ");
                request.is_match(&head)
            })
            .collect();
        let candidates = if preferred.is_empty() {
            &candidates
        } else {
            &preferred
        };
        candidates
            .iter()
            .copied()
            .min_by_key(|&index| self.nodes[index].symbol.start_line)
    }

    fn vapor_handler(
        &self,
        ty: Option<&str>,
        method: &str,
        path: &str,
        line: usize,
    ) -> Option<usize> {
        let callables: Vec<_> = self
            .names
            .get(method)?
            .iter()
            .copied()
            .filter(|&index| {
                let node = &self.nodes[index];
                self.files[&node.path].analysis.language == "swift"
                    && matches!(node.symbol.kind.as_str(), "method" | "function")
            })
            .collect();
        let mut ty = ty.map(str::to_owned);
        if ty.is_none() {
            let owner = self.files[path]
                .nodes
                .iter()
                .copied()
                .filter(|&index| {
                    let node = &self.nodes[index].symbol;
                    swift_type(&node.kind) && node.start_line <= line && node.end_line >= line
                })
                .max_by_key(|&index| self.nodes[index].symbol.start_line);
            let Some(owner) = owner else {
                let functions: Vec<_> = callables
                    .into_iter()
                    .filter(|&index| self.nodes[index].symbol.kind == "function")
                    .collect();
                let local: Vec<_> = functions
                    .iter()
                    .copied()
                    .filter(|&index| self.nodes[index].path == path)
                    .collect();
                return if !local.is_empty() {
                    self.swift_one_owner(local, method)
                } else if functions.len() == 1 {
                    functions.first().copied()
                } else {
                    None
                };
            };
            let owner = &self.nodes[owner].symbol;
            let local: Vec<_> = callables
                .iter()
                .copied()
                .filter(|&index| {
                    let node = &self.nodes[index];
                    node.path == path
                        && node.symbol.start_line >= owner.start_line
                        && node.symbol.end_line <= owner.end_line
                })
                .collect();
            if !local.is_empty() {
                return self.swift_one_owner(local, method);
            }
            ty = Some(owner.qualified_name.replace("::", "."));
        }
        let ty = ty.unwrap();
        let parts: Vec<_> = ty.split('.').collect();
        let last = *parts.last().unwrap();
        let full = parts.join("::");
        let exact: Vec<_> = callables
            .iter()
            .copied()
            .filter(|&index| self.swift_method_owner(index, method) == full)
            .collect();
        if !exact.is_empty() {
            return self.swift_one_owner(exact, method);
        }
        let suffix = format!("::{full}");
        let nested: Vec<_> = callables
            .iter()
            .copied()
            .filter(|&index| self.swift_method_owner(index, method).ends_with(&suffix))
            .collect();
        if !nested.is_empty() {
            return self.swift_one_owner(nested, method);
        }
        if parts.len() > 1 {
            let declaration = regex::Regex::new(&format!(
                r"\bextension\s+{}\b",
                parts
                    .iter()
                    .map(|part| regex::escape(part))
                    .collect::<Vec<_>>()
                    .join(r"\s*\.\s*")
            ))
            .unwrap();
            let extended: Vec<_> = callables
                .into_iter()
                .filter(|&index| {
                    if self.swift_method_owner(index, method) != last {
                        return false;
                    }
                    let node = &self.nodes[index];
                    let file = &self.files[&node.path];
                    let owner = file
                        .nodes
                        .iter()
                        .copied()
                        .filter(|&index| {
                            let candidate = &self.nodes[index].symbol;
                            swift_type(&candidate.kind)
                                && candidate.name == last
                                && candidate.start_line <= node.symbol.start_line
                                && candidate.end_line >= node.symbol.end_line
                        })
                        .max_by_key(|&index| self.nodes[index].symbol.start_line);
                    owner.is_some_and(|index| {
                        declaration.is_match(
                            &file
                                .source
                                .lines()
                                .skip(self.nodes[index].symbol.start_line.saturating_sub(1))
                                .take(3)
                                .collect::<Vec<_>>()
                                .join(" "),
                        )
                    })
                })
                .collect();
            return self.swift_one_owner(extended, method);
        }
        let suffix = format!("::{last}");
        self.swift_one_owner(
            callables
                .into_iter()
                .filter(|&index| self.swift_method_owner(index, method).ends_with(&suffix))
                .collect(),
            method,
        )
    }
}
