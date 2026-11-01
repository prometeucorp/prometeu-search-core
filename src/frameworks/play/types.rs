//! Source-backed JVM type visibility and field lookup for overload arguments.
use super::{Context, java, regex};
use anyhow::Result;
use prometeu_extract::{Node, UnresolvedReference};
use std::collections::HashSet;
#[derive(Clone, Default)]
pub(super) struct Scope {
    pkg: String,
    single: HashSet<String>,
    demand: HashSet<String>,
}
pub(super) struct Type<'a> {
    name: String,
    decl: Option<&'a Node>,
}
pub(super) fn visible_kind(kind: &str) -> bool {
    matches!(
        kind,
        "class" | "interface" | "enum" | "record" | "annotation"
    )
}
fn declared_kind(kind: &str) -> bool {
    matches!(kind, "class" | "interface" | "enum" | "struct" | "trait")
}
fn owner_kind(node: &Node) -> bool {
    matches!(
        node.kind.as_str(),
        "class" | "struct" | "union" | "interface"
    ) || node.language == "scala" && node.kind == "module"
}
impl<'a> Context<'a> {
    pub(super) fn jvm_types(&self, name: &str) -> Vec<&'a Node> {
        self.named(name)
            .into_iter()
            .filter(|node| declared_kind(&node.kind) && node.language == "java")
            .collect()
    }
    fn family_types(&self, name: &str) -> Vec<&'a Node> {
        self.named(name)
            .into_iter()
            .filter(|node| {
                declared_kind(&node.kind)
                    && matches!(node.language.as_str(), "java" | "kotlin" | "scala")
            })
            .collect()
    }
    fn scope(&self, file: &str) -> Result<Scope> {
        if let Some(scope) = self.scopes.borrow().get(file) {
            return Ok(scope.clone());
        }
        let text = crate::synthesis::comments::strip_for_regex(self.source(file), "java");
        self.step(text.len())?;
        let pkg = regex(r"(?m)^\s*package\s+([\w.]+)\s*;")?
            .captures(&text)?
            .map(|m| m[1].into())
            .unwrap_or_default();
        let mut scope = Scope {
            pkg,
            ..Default::default()
        };
        for m in regex(r"(?m)^\s*import\s+(?:static\s+)?([\w.]+?)(\.\*)?\s*;")?.captures_iter(&text)
        {
            let m = m?;
            if m.get(2).is_some() {
                scope.demand.insert(m[1].into());
            } else {
                scope.single.insert(m[1].into());
            }
        }
        self.scopes.borrow_mut().insert(file.into(), scope.clone());
        Ok(scope)
    }
    fn header_supers(&self, node: &Node) -> Result<Vec<String>> {
        let mut header = String::new();
        for line in self
            .source(&node.file_path)
            .split('\n')
            .skip(node.start_line.saturating_sub(1) as usize)
            .take(7)
        {
            header.push_str(line.strip_suffix('\r').unwrap_or(line));
            header.push(' ');
            if header.contains('{') {
                break;
            }
        }
        self.step(header.len())?;
        let head = header.split('{').next().unwrap();
        let clause = regex(r"\b(?:extends|implements)\b([^{]*)")?
            .captures(head)?
            .map(|m| m[1].to_owned())
            .unwrap_or_default();
        let clause = regex(r"<[^<>]*(?:<[^<>]*>[^<>]*)*>")?.replace_all(&clause, "");
        let mut out = Vec::new();
        for m in regex(r"([A-Za-z_]\w*)\s*(?=,|$|\bimplements\b|\s*$)")?.captures_iter(&clause) {
            let m = m?;
            if !["implements", "extends"].contains(&&m[1]) {
                out.push(m[1].into())
            }
        }
        Ok(out)
    }
    fn reachable_supers(&self, name: &str, from: &Node) -> Result<Vec<&'a Node>> {
        let all = self
            .named(name)
            .into_iter()
            .filter(|node| node.language == "java" && visible_kind(&node.kind))
            .collect::<Vec<_>>();
        if all.len() < 2 {
            return Ok(all);
        }
        let here = self.scope(&from.file_path)?;
        let mut reachable = Vec::new();
        for node in &all {
            if node.file_path == from.file_path {
                reachable.push(*node);
                continue;
            }
            let pkg = self.scope(&node.file_path)?.pkg;
            if pkg == here.pkg {
                reachable.push(*node);
                continue;
            }
            let segments = node.qualified_name.split("::").collect::<Vec<_>>();
            let chain = if !pkg.is_empty() && segments[0] == pkg {
                &segments[1..]
            } else {
                &segments[..]
            };
            let fqn = std::iter::once(pkg.as_str())
                .chain(chain.iter().copied())
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(".");
            let owner = fqn.rsplit_once('.').map_or("", |(owner, _)| owner);
            let outer = [pkg.as_str(), chain.first().copied().unwrap_or("")]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(".");
            if here.single.contains(&fqn)
                || here.demand.contains(owner)
                || chain.len() > 1 && here.single.contains(&outer)
            {
                reachable.push(*node)
            }
        }
        Ok(if reachable.is_empty() { all } else { reachable })
    }
    pub(super) fn ancestors(&self, qualified: &str) -> Result<HashSet<String>> {
        if let Some(names) = self.ancestor_cache.borrow().get(qualified) {
            return Ok(names.clone());
        }
        let mut names = HashSet::new();
        let mut seen = HashSet::from([qualified.to_owned()]);
        let mut level = vec![qualified.to_owned()];
        while !level.is_empty() {
            let mut next = Vec::new();
            for qn in level {
                self.step(1)?;
                for decl in self.qualified(&qn) {
                    if decl.language != "java" || !visible_kind(&decl.kind) {
                        continue;
                    }
                    for base in self.header_supers(decl)? {
                        names.insert(base.clone());
                        for parent in self.reachable_supers(&base, decl)? {
                            if seen.insert(parent.qualified_name.clone()) {
                                if let Some(finished) =
                                    self.ancestor_cache.borrow().get(&parent.qualified_name)
                                {
                                    names.extend(finished.iter().cloned());
                                } else {
                                    next.push(parent.qualified_name.clone())
                                }
                            }
                        }
                    }
                }
            }
            level = next;
        }
        self.ancestor_cache
            .borrow_mut()
            .insert(qualified.into(), names.clone());
        Ok(names)
    }
    fn declared_supers(&self, node: &Node) -> Result<Vec<String>> {
        if node.language == "scala" {
            let mut names = Vec::new();
            for decl in self.named(&node.name) {
                if decl.language != "scala"
                    || !matches!(decl.kind.as_str(), "class" | "trait" | "module" | "enum")
                {
                    continue;
                }
                let text = self
                    .source(&decl.file_path)
                    .split('\n')
                    .skip(decl.start_line.saturating_sub(1) as usize)
                    .take(13)
                    .collect::<Vec<_>>()
                    .join(" ");
                let mut depth = 0i32;
                let mut flat = String::new();
                for ch in text.chars() {
                    match ch {
                        '[' | '(' => depth += 1,
                        ']' | ')' => depth = (depth - 1).max(0),
                        _ if depth == 0 => {
                            if ch == '{' || ch == '=' {
                                break;
                            }
                            flat.push(ch)
                        }
                        _ => {}
                    }
                }
                let clause = regex(r"\bextends\b(.*)$")?
                    .captures(&flat)?
                    .map(|m| m[1].to_owned())
                    .unwrap_or_default();
                for m in regex(r"([A-Za-z_][\w.]*)")?.captures_iter(&clause) {
                    let m = m?;
                    let name = m[1].rsplit('.').next().unwrap();
                    if !["with", "derives", node.name.as_str()].contains(&name) {
                        names.push(name.into())
                    }
                }
            }
            return Ok(names);
        }
        if !matches!(node.language.as_str(), "java" | "kotlin") {
            return Ok(Vec::new());
        }
        let text = self
            .source(&node.file_path)
            .split('\n')
            .skip(node.start_line.saturating_sub(1) as usize)
            .take(9)
            .map(|line| line.strip_suffix('\r').unwrap_or(line))
            .collect::<Vec<_>>()
            .join(" ");
        let text = regex(r"//[^\n]*|/\*.*?\*/")?.replace_all(&text, " ");
        let mut depth = 0i32;
        let mut head = String::new();
        for ch in text.chars() {
            match ch {
                '{' if depth == 0 => break,
                '<' | '(' => depth += 1,
                '>' | ')' => depth = (depth - 1).max(0),
                _ if depth == 0 => head.push(ch),
                _ => {}
            }
        }
        let clause = regex(r"\b(?:extends|implements)\b([\s\S]*)$")?
            .captures(&head)?
            .or(
                regex(r"\b(?:class|interface|struct|record|object)\s+\w+[^:]*:([\s\S]*)$")?
                    .captures(&head)?,
            )
            .map(|m| m[1].to_owned())
            .unwrap_or_default();
        let clause = regex(r"\bwhere\b[\s\S]*$")?.replace(&clause, "");
        regex(r"([A-Z]\w*)\s*(?=,|$|\bimplements\b)")?
            .captures_iter(&clause)
            .map(|m| Ok(m?[1].to_owned()))
            .collect()
    }
    fn file_ancestors(&self, file: &str) -> Result<HashSet<&'a str>> {
        let mut frontier = self
            .in_file(file)
            .into_iter()
            .filter(|node| node.language == "java" && owner_kind(node))
            .collect::<Vec<_>>();
        let mut seen = frontier
            .iter()
            .map(|n| n.id.as_str())
            .collect::<HashSet<_>>();
        let mut out = HashSet::new();
        for _ in 0..5 {
            let mut next = Vec::new();
            for owner in frontier {
                for name in self.declared_supers(owner)? {
                    for node in self.named(&name) {
                        if node.language == "java" && owner_kind(node) && seen.insert(&node.id) {
                            out.insert(node.id.as_str());
                            next.push(node)
                        }
                    }
                }
            }
            frontier = next;
        }
        Ok(out)
    }
    fn nameable(&self, node: &Node, file: &str) -> Result<bool> {
        if node.language != "java" || node.file_path == file {
            return Ok(true);
        }
        let segments = node.qualified_name.split("::").collect::<Vec<_>>();
        let has_package =
            segments.len() > 1 && segments[0].starts_with(|c: char| c.is_ascii_lowercase());
        let chain = if has_package {
            &segments[1..]
        } else {
            &segments[..]
        };
        if chain.len() <= 1 {
            return Ok(true);
        }
        let pkg = if has_package { segments[0] } else { "" };
        let scope = self.scope(file)?;
        if scope.pkg == pkg {
            return Ok(true);
        }
        let outer = if pkg.is_empty() {
            chain[0].into()
        } else {
            format!("{pkg}.{}", chain[0])
        };
        if self
            .imports
            .get(file)
            .into_iter()
            .flatten()
            .any(|(_, source)| source == &outer || source.starts_with(&format!("{outer}.")))
        {
            return Ok(true);
        }
        if scope
            .demand
            .contains(&segments[..segments.len() - 1].join("."))
        {
            return Ok(true);
        }
        let ancestors = self.file_ancestors(file)?;
        Ok(self
            .qualified(&segments[..segments.len() - 1].join("::"))
            .iter()
            .any(|n| ancestors.contains(n.id.as_str())))
    }
    pub(super) fn type_ref(&self, raw: &str, file: &str) -> Result<Option<Type<'a>>> {
        let raw = java::without_generics(raw)?;
        let raw = regex(r"\[\s*\]")?.replace_all(&raw, "");
        let raw = raw.strip_suffix("...").unwrap_or(&raw);
        let raw = regex(r"\s+")?.replace_all(raw, "");
        let segments = raw.split('.').filter(|s| !s.is_empty()).collect::<Vec<_>>();
        let mut first = segments.len();
        let upper = regex(r"^[A-Z][\w$]*$")?;
        while first > 0 && upper.is_match(segments[first - 1])? {
            first -= 1
        }
        let chain = segments[first..].join("::");
        if chain.is_empty()
            || [
                "this",
                "self",
                "super",
                "new",
                "return",
                "await",
                "yield",
                "typeof",
                "null",
                "nil",
                "None",
                "true",
                "false",
                "True",
                "False",
                "undefined",
            ]
            .contains(&chain.as_str())
        {
            return Ok(None);
        }
        let pkg = segments[..first].join(".");
        let chain = chain.split("::").collect::<Vec<_>>();
        let outer = chain[0];
        if pkg.is_empty() && chain.len() == 1 && regex(r"^[A-Z][A-Z0-9]{0,2}$")?.is_match(outer)? {
            return Ok(Some(Type {
                name: outer.into(),
                decl: None,
            }));
        }
        let decls = self.family_types(outer);
        if decls.is_empty() {
            return Ok(None);
        }
        let pins = if !pkg.is_empty() {
            vec![format!("{pkg}.{outer}")]
        } else {
            self.imports
                .get(file)
                .into_iter()
                .flatten()
                .filter(|(local, _)| local == outer)
                .map(|(_, source)| source.clone())
                .collect()
        };
        if pins.is_empty() {
            let mut nameable = false;
            for decl in &decls {
                nameable |= self.nameable(decl, file)?;
            }
            if !nameable {
                return Ok(None);
            }
            if chain.len() == 1 {
                return Ok(Some(Type {
                    name: outer.into(),
                    decl: None,
                }));
            }
            let name = chain.join("::");
            return Ok(self
                .family_types(chain.last().unwrap())
                .iter()
                .any(|n| {
                    n.qualified_name == name || n.qualified_name.ends_with(&format!("::{name}"))
                })
                .then_some(Type { name, decl: None }));
        }
        let mut decl = decls.into_iter().find(|node| {
            pins.iter().any(|pin| {
                let dotted = node.qualified_name.replace("::", ".");
                let path = node.file_path.replace('\\', "/");
                let file = path
                    .rsplit_once('.')
                    .map_or(path.as_str(), |(file, _)| file);
                let directory = path.rsplit_once('/').map_or("", |(dir, _)| dir);
                let pin_path = pin.replace('.', "/");
                let pin_pkg = pin_path.rsplit_once('/').map_or("", |(dir, _)| dir);
                dotted == *pin
                    || file == pin_path
                    || file.ends_with(&format!("/{pin_path}"))
                    || node.language != "java"
                        && !pin_pkg.is_empty()
                        && (directory == pin_pkg || directory.ends_with(&format!("/{pin_pkg}")))
            })
        });
        for segment in &chain[1..] {
            let Some(owner) = decl else { break };
            decl = self
                .family_types(segment)
                .into_iter()
                .find(|n| n.qualified_name == format!("{}::{segment}", owner.qualified_name));
        }
        Ok(decl.map(|decl| {
            let segments = decl.qualified_name.split("::").collect::<Vec<_>>();
            let has_package = decl.language == "java"
                && segments.len() > 1
                && segments[0].starts_with(|c: char| c.is_ascii_lowercase());
            Type {
                name: segments[usize::from(has_package)..].join("::"),
                decl: Some(decl),
            }
        }))
    }
    pub(super) fn type_owners(&self, kind: &Type<'a>, file: &str) -> Result<Vec<&'a Node>> {
        if let Some(decl) = kind.decl {
            return Ok(vec![decl]);
        }
        let mut out = Vec::new();
        for node in self.family_types(kind.name.rsplit("::").next().unwrap()) {
            if node.language == "java"
                && (node.qualified_name == kind.name
                    || node.qualified_name.ends_with(&format!("::{}", kind.name)))
                && self.nameable(node, file)?
            {
                out.push(node)
            }
        }
        Ok(out)
    }
    pub(super) fn static_field(&self, owner: &'a Node, name: &str) -> Result<Option<&'a Node>> {
        let mut seen = HashSet::from([owner.id.as_str()]);
        let mut frontier = vec![owner];
        for _ in 0..5 {
            let mut next = Vec::new();
            for owner in frontier {
                self.step(1)?;
                if let Some(field) = self
                    .qualified(&format!("{}::{name}", owner.qualified_name))
                    .into_iter()
                    .find(|n| {
                        n.file_path == owner.file_path
                            && matches!(n.kind.as_str(), "field" | "constant" | "enum_member")
                    })
                {
                    return Ok(Some(field));
                }
                for parent in self.declared_supers(owner)? {
                    let Some(kind) = self.type_ref(&parent, &owner.file_path)? else {
                        continue;
                    };
                    let parents = if let Some(decl) = kind.decl {
                        vec![decl]
                    } else {
                        self.family_types(kind.name.rsplit("::").next().unwrap())
                            .into_iter()
                            .filter(|node| {
                                node.qualified_name == kind.name
                                    || node.qualified_name.ends_with(&format!("::{}", kind.name))
                            })
                            .collect()
                    };
                    for parent in parents {
                        if seen.insert(parent.id.as_str()) {
                            next.push(parent)
                        }
                    }
                }
            }
            frontier = next;
        }
        Ok(None)
    }
    pub(super) fn type_method(
        &self,
        kind: &Type<'a>,
        name: &str,
        reference: &UnresolvedReference,
        depth: usize,
    ) -> Result<Option<&'a Node>> {
        if let Some(decl) = kind.decl {
            if let Some(method) = self.named(name).into_iter().find(|n| {
                n.kind == "method" && n.qualified_name == format!("{}::{name}", decl.qualified_name)
            }) {
                return Ok(Some(method));
            }
            if decl.language != "java" {
                return Ok(None);
            }
        }
        let direct = self.method_on_type(
            &kind.name,
            name,
            reference,
            kind.decl.map(|decl| decl.qualified_name.replace("::", ".")),
            depth,
        )?;
        if direct.is_some() || !kind.name.contains("::") {
            return Ok(direct);
        }
        let simple = kind.name.rsplit("::").next().unwrap();
        let mut seen = HashSet::new();
        for owner in self.jvm_types(simple) {
            for parent in self.supers.get(owner.id.as_str()).into_iter().flatten() {
                if seen.insert(parent.name.as_str())
                    && let Some(method) =
                        self.method_on_type(&parent.name, name, reference, None, 1)?
                {
                    return Ok(Some(method));
                }
            }
        }
        Ok(None)
    }
    fn method_on_type(
        &self,
        kind: &str,
        name: &str,
        reference: &UnresolvedReference,
        preferred: Option<String>,
        depth: usize,
    ) -> Result<Option<&'a Node>> {
        self.step(1)?;
        let wanted = format!("{kind}::{name}");
        let matches = self
            .named(name)
            .into_iter()
            .filter(|n| {
                n.kind == "method"
                    && matches!(n.language.as_str(), "java" | "kotlin" | "scala")
                    && (n.qualified_name == wanted
                        || n.qualified_name.ends_with(&format!("::{wanted}")))
            })
            .collect::<Vec<_>>();
        if matches.is_empty() {
            if depth < 4 {
                let simple = kind.rsplit("::").next().unwrap();
                let mut tried = HashSet::new();
                for owner in self.jvm_types(simple) {
                    if kind != simple
                        && owner.qualified_name != kind
                        && !owner.qualified_name.ends_with(&format!("::{kind}"))
                    {
                        continue;
                    }
                    for parent in self.supers.get(owner.id.as_str()).into_iter().flatten() {
                        if parent.name.is_empty() || parent.name == simple {
                            continue;
                        }
                        let segments = parent.qualified_name.split("::").collect::<Vec<_>>();
                        let first = segments
                            .iter()
                            .position(|segment| {
                                segment.starts_with(|c: char| c.is_ascii_uppercase())
                            })
                            .unwrap_or(segments.len() - 1);
                        let chain = segments[first..].join("::");
                        for parent in [&chain, &parent.name] {
                            if tried.insert(parent.clone())
                                && let Some(found) = self.method_on_type(
                                    parent,
                                    name,
                                    reference,
                                    preferred.clone(),
                                    depth + 1,
                                )?
                            {
                                return Ok(Some(found));
                            }
                        }
                    }
                }
            }
            return Ok(None);
        }
        if matches.len() > 1
            && let Some(preferred) = preferred.as_deref()
        {
            if let Some((pkg, class)) = preferred.rsplit_once('.')
                && let Some(node) = matches
                    .iter()
                    .find(|node| node.qualified_name == format!("{pkg}::{class}::{name}"))
            {
                return Ok(Some(node));
            }
            let path = format!("{}.java", preferred.replace('.', "/"));
            if let Some(node) = matches
                .iter()
                .find(|node| node.file_path.replace('\\', "/").ends_with(&path))
            {
                return Ok(Some(node));
            }
        }
        if reference.reference_kind == "function_ref" && matches.len() != 1 {
            return Ok(None);
        }
        let own = matches
            .iter()
            .copied()
            .filter(|node| node.language == "java")
            .collect::<Vec<_>>();
        let matches = if own.is_empty() { matches } else { own };
        Ok(matches
            .iter()
            .find(|node| Some(&node.file_path) == reference.file_path.as_ref())
            .copied()
            .or_else(|| matches.first().copied()))
    }
}
