//! Source-only constants, named component bindings, and lazy route mounts.
use super::{
    Node, component_classes, in_file_path, is_route, join, object, parse, regex, segments,
    static_string, trim,
};
use anyhow::{Result, ensure};
use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap, HashSet},
};
pub(crate) type ModuleResolver<'a> = dyn Fn(&str, &str, &str) -> Result<Option<String>> + 'a;
pub(crate) struct AngularContext<'a> {
    pub nodes: &'a [Node],
    pub sources: &'a BTreeMap<&'a str, &'a str>,
    pub manifests: &'a BTreeMap<&'a str, &'a str>,
    pub auxiliary_sources: Option<&'a BTreeMap<&'a str, &'a str>>,
    pub resolve_module: &'a ModuleResolver<'a>,
    by_file: BTreeMap<&'a str, Vec<&'a Node>>,
    constants: RefCell<HashMap<(String, String), Option<String>>>,
}
impl<'a> AngularContext<'a> {
    pub(crate) fn new(
        nodes: &'a [Node],
        sources: &'a BTreeMap<&'a str, &'a str>,
        resolve_module: &'a ModuleResolver<'a>,
        manifests: &'a BTreeMap<&'a str, &'a str>,
    ) -> Self {
        let mut by_file: BTreeMap<&str, Vec<&Node>> = BTreeMap::new();
        for node in nodes {
            by_file.entry(&node.file_path).or_default().push(node);
        }
        for nodes in by_file.values_mut() {
            nodes.sort_by(|a, b| (a.start_line, &a.id).cmp(&(b.start_line, &b.id)));
        }
        Self {
            nodes,
            sources,
            manifests,
            auxiliary_sources: None,
            resolve_module,
            by_file,
            constants: RefCell::new(HashMap::new()),
        }
    }
    pub(crate) fn nodes_in(&self, file: &str) -> &[&'a Node] {
        self.by_file.get(file).map_or(&[], Vec::as_slice)
    }
    pub(crate) fn read(&self, file: &str) -> Option<&'a str> {
        self.sources.get(file).copied().or_else(|| {
            self.auxiliary_sources
                .and_then(|sources| sources.get(file).copied())
        })
    }
    pub(crate) fn module(&self, spec: &str, from: &str) -> Result<Option<String>> {
        (self.resolve_module)(spec, from, "typescript")
    }
    pub(crate) fn declaring_file(&self, name: &str, from: &str) -> Result<Option<String>> {
        let Some(source) = self.read(from).filter(|s| !s.is_empty()) else {
            return Ok(None);
        };
        let aliases = regex(r"\s+as\s+")?;
        for capture in
            regex(r#"\bimport\s*\{([^}]*)\}\s*from\s*(['"])([^'"]+)\2"#)?.captures_iter(source)
        {
            let capture = capture?;
            for item in capture[1].split(',') {
                let local = aliases.split(trim(item)).last().transpose()?.unwrap_or("");
                if trim(local) == name {
                    return self.module(&capture[3], from);
                }
            }
        }
        Ok(Some(from.into()))
    }
    pub(crate) fn lazy_component(
        &self,
        spec: &str,
        member: Option<&str>,
        from: &str,
    ) -> Result<Option<&'a Node>> {
        let Some(file) = self.module(spec, from)? else {
            return Ok(None);
        };
        let mut name = member.map(str::to_owned);
        if name.as_deref().is_none_or(str::is_empty) {
            let Some(source) = self.read(&file).filter(|s| !s.is_empty()) else {
                return Ok(None);
            };
            let classes = component_classes(source)?;
            name = classes
                .iter()
                .find(|class| class.is_default)
                .or_else(|| (classes.len() == 1).then(|| &classes[0]))
                .map(|class| class.name.clone());
            if name.is_none() {
                name = regex(r"\bexport\s+default\s+class\s+([A-Za-z_$][\w$]*)")?
                    .captures(source)?
                    .map(|capture| capture[1].into());
            }
        }
        Ok(name.and_then(|name| {
            self.nodes_in(&file)
                .iter()
                .copied()
                .find(|node| node.kind == "class" && node.name == name)
        }))
    }
    pub(crate) fn component(&self, encoded: &str, from: &str) -> Result<Option<&'a Node>> {
        if let Some(lazy) = regex(r"^import:([^#]+)#([\w$]+)$")?.captures(encoded)? {
            return self.lazy_component(
                &lazy[1],
                (&lazy[2] != "default").then_some(&lazy[2]),
                from,
            );
        }
        let Some(file) = self.declaring_file(encoded, from)? else {
            return Ok(None);
        };
        Ok(self
            .nodes_in(&file)
            .iter()
            .copied()
            .find(|node| node.kind == "class" && node.name == encoded))
    }
    fn declaration_source(&self, root: &str, file: &str, hops: usize) -> Result<Option<&'a str>> {
        let Some(content) = self.read(file).filter(|s| !s.is_empty()) else {
            return Ok(None);
        };
        if regex(&format!(
            r"\b(?:const|let|class|enum)\s+{}\b",
            ::regex::escape(root)
        ))?
        .is_match(content)?
        {
            return Ok(Some(content));
        }
        if hops >= 3 {
            return Ok(None);
        }
        for capture in
            regex(r#"\bexport\s+(\*|\{[^}]*\})\s*from\s+(['"])([^'"]+)\2"#)?.captures_iter(content)
        {
            let capture = capture?;
            if &capture[1] != "*"
                && !regex(&format!(r"\b{}\b", ::regex::escape(root)))?.is_match(&capture[1])?
            {
                continue;
            }
            if let Some(target) = self.module(&capture[3], file)?
                && let Some(found) = self.declaration_source(root, &target, hops + 1)?
            {
                return Ok(Some(found));
            }
        }
        Ok(None)
    }
    fn local_alias(&self, expr: &str, from: &str) -> Result<Option<String>> {
        let Some(content) = self.read(from).filter(|s| !s.is_empty()) else {
            return Ok(None);
        };
        let mut parts = expr.split('.');
        let root = parts.next().unwrap_or("");
        if root.is_empty() {
            return Ok(None);
        }
        let tail = parts.collect::<Vec<_>>();
        let compact = |text: &str| {
            text.chars()
                .filter(|&ch| !crate::search::query_whitespace(ch))
                .collect::<String>()
        };
        for capture in regex(r"\b(?:const|let)\s*\{([^}]*)\}\s*=\s*([A-Za-z_$][\w$]*(?:\s*\.\s*[A-Za-z_$][\w$]*)*)\s*;?")?.captures_iter(content){let capture=capture?;
            for binding in capture[1].split(','){let mut parts=binding.split(':').map(trim);let prop=parts.next().unwrap_or("");let local=parts.next().unwrap_or(prop);if local==root&&!prop.is_empty(){let mut out=vec![format!("{}.{prop}",compact(&capture[2]))];out.extend(tail.iter().map(|s|(*s).into()));return Ok(Some(out.join(".")))}}
        }
        Ok(regex(&format!(
            r"\b(?:const|let)\s+{}\s*=\s*([A-Za-z_$][\w$]*(?:\s*\.\s*[A-Za-z_$][\w$]*)+)\s*;",
            ::regex::escape(root)
        ))?
        .captures(content)?
        .map(|capture| {
            let mut out = vec![compact(&capture[1])];
            out.extend(tail.iter().map(|s| (*s).into()));
            out.join(".")
        }))
    }
    pub(crate) fn constant_text(&self, expr: &str, from: &str) -> Result<Option<String>> {
        self.constant_inner(expr, from, 0)
    }
    fn constant_inner(&self, expr: &str, from: &str, depth: usize) -> Result<Option<String>> {
        ensure!(
            depth < 128,
            "Angular constant alias nesting exceeds 128 levels"
        );
        let key = (from.to_owned(), expr.to_owned());
        if let Some(found) = self.constants.borrow().get(&key) {
            return Ok(found.clone());
        }
        ensure!(
            self.constants.borrow().len() < 100_000,
            "Angular constant lookup budget exceeded"
        );
        if let Some(aliased) = self
            .local_alias(expr, from)?
            .filter(|aliased| aliased != expr)
        {
            let value = self.constant_inner(&aliased, from, depth + 1)?;
            self.constants.borrow_mut().insert(key, value.clone());
            return Ok(value);
        }
        let parts = expr.split('.').map(trim).collect::<Vec<_>>();
        let root = parts[0];
        let chain = &parts[1..];
        let mut value = None;
        if !root.is_empty()
            && !chain.is_empty()
            && let Some(file) = self.declaring_file(root, from)?
            && let Some(content) = self.declaration_source(root, &file, 0)?
        {
            let safe = crate::synthesis::comments::strip_for_regex(content, "typescript");
            let decl = regex(&format!(
                r"\b(?:const|let)\s+{}\s*(?::[^=]+)?=\s*\{{",
                ::regex::escape(root)
            ))?
            .find(&safe)?;
            let enum_decl = if decl.is_none() {
                regex(&format!(r"\benum\s+{}\s*\{{", ::regex::escape(root)))?.find(&safe)?
            } else {
                None
            };
            let class_decl = if decl.is_none() && enum_decl.is_none() {
                regex(&format!(r"\bclass\s+{}\b[^{{]*\{{", ::regex::escape(root)))?.find(&safe)?
            } else {
                None
            };
            if let Some(decl) = decl {
                value = read_chain(&safe, decl.end() - 1, chain)?;
            } else if let Some(decl) = enum_decl.filter(|_| chain.len() == 1) {
                let open = decl.end() - 1;
                let close = object::matching(&safe, open)?.unwrap_or(open);
                let body = &safe[open..close.max(open)];
                value = regex(&format!(
                    r#"\b{}\s*=\s*(['"`][^'"`]*['"`])"#,
                    ::regex::escape(chain[0])
                ))?
                .captures(body)?
                .map(|capture| capture[1].into());
            } else if let Some(decl) = class_decl {
                let open = decl.end() - 1;
                let close = object::matching(&safe, open)?.unwrap_or(open);
                let body = &safe[open..close.max(open)];
                if let Some(field) = regex(&format!(
                    r"\bstatic\s+(?:readonly\s+)?{}\s*(?::[^=;]+)?=\s*",
                    ::regex::escape(chain[0])
                ))?
                .find(body)?
                {
                    let at = open + field.end();
                    if safe.as_bytes().get(at) == Some(&b'{') {
                        if chain.len() > 1 {
                            value = read_chain(&safe, at, &chain[1..])?;
                        }
                    } else if chain.len() == 1 {
                        value = regex(r#"^(['"`])[^'"`]*\1"#)?
                            .find(&safe[at..])?
                            .map(|found| found.as_str().into());
                    }
                }
            }
        }
        self.constants.borrow_mut().insert(key, value.clone());
        Ok(value)
    }
    pub(crate) fn constant_value(&self, expr: &str, from: &str) -> Result<Option<String>> {
        self.constant_text(expr, from)?
            .map(|value| static_string(&value))
            .transpose()
            .map(Option::flatten)
    }
    pub(crate) fn resolved_segments(&self, path: &str, file: &str) -> Result<Vec<String>> {
        let mut out = Vec::new();
        for seg in path.split('/').filter(|part| !part.is_empty()) {
            if seg.starts_with('{') && seg.ends_with('}') && !seg[1..].contains('{') {
                if let Some(value) = self.constant_value(&seg[1..seg.len() - 1], file)? {
                    out.extend(segments(&value));
                } else {
                    out.push(seg.into());
                }
            } else {
                let mut replaced = String::new();
                let mut at = 0;
                for capture in regex(r"\{([^{}]+)\}")?.captures_iter(seg) {
                    let capture = capture?;
                    let whole = capture.get(0).unwrap();
                    replaced.push_str(&seg[at..whole.start()]);
                    replaced.push_str(
                        self.constant_value(&capture[1], file)?
                            .as_deref()
                            .unwrap_or(whole.as_str()),
                    );
                    at = whole.end();
                }
                replaced.push_str(&seg[at..]);
                out.push(replaced);
            }
        }
        Ok(out)
    }
    fn loaded_route_files(
        &self,
        spec: &str,
        from: &str,
        route_files: &HashSet<&str>,
        mount_files: &HashSet<&str>,
    ) -> Result<Vec<String>> {
        let mut barrels = self.module(spec, from)?.into_iter().collect::<Vec<_>>();
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for _ in 0..4 {
            let mut next = Vec::new();
            for file in barrels {
                if !seen.insert(file.clone()) {
                    continue;
                }
                if route_files.contains(file.as_str()) || mount_files.contains(file.as_str()) {
                    out.push(file);
                    continue;
                }
                let Some(content) = self.read(&file).filter(|s| !s.is_empty()) else {
                    continue;
                };
                if regex(r"@NgModule\s*\(")?.is_match(content)? {
                    for capture in regex(r#"\bimport\s+[^'"]*?from\s+(['"])([^'"]+)\1"#)?
                        .captures_iter(content)
                    {
                        let capture = capture?;
                        if let Some(imported) = self.module(&capture[2], &file)?
                            && (route_files.contains(imported.as_str())
                                || mount_files.contains(imported.as_str()))
                            && !out.contains(&imported)
                        {
                            out.push(imported);
                        }
                    }
                    continue;
                }
                for capture in regex(
                    r#"\bexport\s+(?:\*|\{[^}]*\})\s*(?:as\s+[\w$]+\s*)?from\s+(['"])([^'"]+)\1"#,
                )?
                .captures_iter(content)
                {
                    let capture = capture?;
                    if let Some(target) = self.module(&capture[2], &file)?
                        && !seen.contains(&target)
                    {
                        next.push(target);
                    }
                }
            }
            barrels = next;
            if barrels.is_empty() {
                break;
            }
        }
        Ok(out)
    }
    pub(crate) fn mounts(&self) -> Result<Mounts> {
        let route_files = self
            .nodes
            .iter()
            .filter(|node| is_route(node))
            .map(|node| node.file_path.as_str())
            .collect::<HashSet<_>>();
        let mut mounts = BTreeMap::new();
        for (&file, &source) in self.sources {
            if !regex(r"\.[cm]?ts$")?.is_match(file)? || !source.contains("loadChildren") {
                continue;
            }
            let parsed = parse(source)?;
            if !parsed.mounts.is_empty() {
                mounts.insert(file, parsed.mounts);
            }
        }
        let mount_files = mounts.keys().copied().collect::<HashSet<_>>();
        let mut loaded = BTreeMap::new();
        for (file, mounts) in &mounts {
            for mount in mounts {
                for target in
                    self.loaded_route_files(&mount.spec, file, &route_files, &mount_files)?
                {
                    if target != *file {
                        loaded
                            .entry(target)
                            .or_insert_with(|| (file.to_string(), mount.prefix.clone()));
                    }
                }
            }
        }
        let mut output = Mounts {
            files: mounts.keys().map(|s| s.to_string()).collect(),
            prefixes: HashMap::new(),
        };
        for file in self.sources.keys() {
            mount_prefix(
                file,
                &loaded,
                &mut output.prefixes,
                &mut HashSet::new(),
                self,
                0,
            )?;
        }
        Ok(output)
    }
    pub(crate) fn post_extract(&self) -> Result<Vec<Node>> {
        if !self.nodes.iter().any(is_route) {
            return Ok(Vec::new());
        }
        let mounts = self.mounts()?;
        let mut updates = Vec::new();
        for node in self.nodes.iter().filter(|node| is_route(node)) {
            let mut parts = mounts.prefix(&node.file_path).to_vec();
            parts.extend(self.resolved_segments(in_file_path(node), &node.file_path)?);
            let name = join(&parts);
            if name != node.name {
                let mut node = node.clone();
                node.name = name;
                updates.push(node);
            }
        }
        Ok(updates)
    }
}
fn read_chain(safe: &str, open: usize, links: &[&str]) -> Result<Option<String>> {
    let mut start = open;
    let mut end = object::matching(safe, start)?;
    for (i, link) in links.iter().enumerate() {
        let Some(close) = end.filter(|&end| end > start) else {
            break;
        };
        let fields = object::fields(safe, start, close)?;
        let Some(field) = fields.get(*link) else {
            return Ok(None);
        };
        if i == links.len() - 1 {
            return Ok(Some(trim(field.text).into()));
        }
        let Some(open) = safe[field.at..].find('{').map(|at| at + field.at) else {
            break;
        };
        start = open;
        end = object::matching(safe, start)?;
    }
    Ok(None)
}
pub(crate) struct Mounts {
    pub files: Vec<String>,
    prefixes: HashMap<String, Vec<String>>,
}
impl Mounts {
    pub fn prefix(&self, file: &str) -> &[String] {
        self.prefixes.get(file).map_or(&[], Vec::as_slice)
    }
}
fn mount_prefix(
    file: &str,
    loaded: &BTreeMap<String, (String, String)>,
    memo: &mut HashMap<String, Vec<String>>,
    seen: &mut HashSet<String>,
    ctx: &AngularContext<'_>,
    depth: usize,
) -> Result<Vec<String>> {
    if let Some(found) = memo.get(file) {
        return Ok(found.clone());
    }
    ensure!(
        depth < 512,
        "Angular route mount nesting exceeds 512 levels"
    );
    let mut prefix = Vec::new();
    if let Some((parent, path)) = loaded.get(file)
        && seen.insert(file.into())
    {
        prefix = mount_prefix(parent, loaded, memo, seen, ctx, depth + 1)?;
        prefix.extend(ctx.resolved_segments(path, parent)?);
    }
    memo.insert(file.into(), prefix.clone());
    Ok(prefix)
}
