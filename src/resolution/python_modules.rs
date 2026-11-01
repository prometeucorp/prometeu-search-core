//! Python package roots, namespace portions, and evidence-backed script imports.
use crate::parser::Import;
use anyhow::Result;
use prometeu_extract::Node;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};

fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(parent, _)| parent)
}
fn join(directory: &str, path: &str) -> String {
    if directory.is_empty() {
        path.into()
    } else {
        format!("{directory}/{path}")
    }
}
fn within(path: &str, scope: &str) -> bool {
    scope.is_empty()
        || path == scope
        || path
            .strip_prefix(scope)
            .is_some_and(|tail| tail.starts_with('/'))
}
fn regex(pattern: &str) -> Result<fancy_regex::Regex> {
    Ok(fancy_regex::Regex::new(
        &crate::parser::javascript_regex::pattern(pattern),
    )?)
}
fn trim(value: &str) -> &str {
    value.trim_matches(crate::search::query_whitespace)
}
fn unquote(value: &str) -> &str {
    let value = value.strip_prefix(['\'', '"']).unwrap_or(value);
    value.strip_suffix(['\'', '"']).unwrap_or(value)
}

pub(crate) struct PythonModules {
    files: HashSet<String>,
    packages: HashSet<String>,
    directories: HashSet<String>,
    projects: HashSet<String>,
    local: BTreeMap<String, String>,
    defined: HashMap<String, HashSet<String>>,
    imports: BTreeMap<String, Vec<Import>>,
    heads: RefCell<HashMap<String, Vec<(String, bool)>>>,
}
impl PythonModules {
    pub(crate) fn new(
        paths: impl IntoIterator<Item = String>,
        manifests: &BTreeMap<String, String>,
        nodes: &[Node],
        imports: BTreeMap<String, Vec<Import>>,
    ) -> Result<Self> {
        let mut this = Self {
            files: HashSet::new(),
            packages: HashSet::new(),
            directories: HashSet::from([String::new()]),
            projects: HashSet::from([String::new()]),
            local: BTreeMap::new(),
            defined: HashMap::new(),
            imports,
            heads: RefCell::new(HashMap::new()),
        };
        for path in paths {
            let path = path.replace('\\', "/");
            if !path.ends_with(".py") {
                continue;
            }
            let directory = parent(&path);
            if path == "__init__.py" || path.ends_with("/__init__.py") {
                this.packages.insert(directory.into());
            }
            let mut ancestor = directory;
            while !ancestor.is_empty() && this.directories.insert(ancestor.into()) {
                ancestor = parent(ancestor);
            }
            this.files.insert(path);
        }
        for node in nodes {
            if matches!(
                node.kind.as_str(),
                "function" | "class" | "variable" | "constant"
            ) && !node.qualified_name.contains("::")
            {
                this.defined
                    .entry(node.file_path.clone())
                    .or_default()
                    .insert(node.name.clone());
            }
        }
        let packages: Vec<_> = this.packages.iter().cloned().collect();
        for directory in &packages {
            let mut top = directory.as_str();
            while !top.is_empty() && this.packages.contains(parent(top)) {
                top = parent(top);
            }
            if !top.is_empty() {
                this.add_local(parent(top), parent(top));
            }
        }
        let directories: Vec<_> = this.directories.iter().cloned().collect();
        for service in directories {
            let configured = this.files.contains(&join(&service, "setup.py"))
                || manifests.contains_key(&join(&service, "pyproject.toml"))
                || manifests.contains_key(&join(&service, "setup.cfg"));
            if !configured && !service.is_empty() {
                continue;
            }
            let reach = if service.is_empty() {
                None
            } else {
                this.service_reach(&service)
            };
            let mut roots = vec![service.clone()];
            let source = join(&service, "src");
            if this.directories.contains(&source) && !this.packages.contains(&source) {
                roots.push(source);
            }
            roots.extend(
                declared_roots(&service, manifests)?
                    .into_iter()
                    .filter(|root| this.directories.contains(root)),
            );
            for root in roots {
                if let Some(reach) = &reach {
                    this.add_local(&root, reach);
                } else {
                    this.projects.insert(root);
                }
            }
        }
        this.local.retain(|root, _| !this.projects.contains(root));
        Ok(this)
    }
    fn add_local(&mut self, root: &str, reach: &str) {
        if self
            .local
            .get(root)
            .is_none_or(|prior| within(prior, reach))
        {
            self.local.insert(root.into(), reach.into());
        }
    }
    fn service_reach(&self, service: &str) -> Option<String> {
        if self.packages.contains(service) {
            return Some(service.into());
        }
        let mut ancestor = parent(service);
        while !ancestor.is_empty() {
            if self.packages.contains(ancestor) {
                return Some(service.into());
            }
            if [
                "tests",
                "test",
                "testing",
                "fixtures",
                "testdata",
                "examples",
                "example",
                "samples",
                "vendor",
                "third_party",
            ]
            .contains(&ancestor.rsplit('/').next().unwrap())
            {
                return Some(ancestor.into());
            }
            ancestor = parent(ancestor);
        }
        None
    }
    fn at(&self, base: &str) -> Option<String> {
        let init = join(base, "__init__.py");
        if self.files.contains(&init) {
            Some(init)
        } else {
            let module = format!("{base}.py");
            self.files.contains(&module).then_some(module)
        }
    }
    fn under(&self, root: &str, relative: &str) -> Option<String> {
        let parts: Vec<_> = relative.split('/').collect();
        let mut base = root.to_owned();
        for part in &parts[..parts.len().saturating_sub(1)] {
            base = join(&base, part);
            if self.files.contains(&join(&base, "__init__.py")) {
                continue;
            }
            if self.files.contains(&format!("{base}.py")) || !self.directories.contains(&base) {
                return None;
            }
        }
        self.at(&join(root, relative))
    }
    fn head(&self, root: &str, name: &str) -> Option<bool> {
        let base = join(root, name);
        if self.at(&base).is_some() {
            Some(true)
        } else {
            self.directories.contains(&base).then_some(false)
        }
    }

    pub(crate) fn resolve(&self, specifier: &str, from: &str) -> Option<String> {
        if specifier.is_empty()
            || !specifier
                .bytes()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, b'_' | b'.'))
        {
            return None;
        }
        let from = from.replace('\\', "/");
        let directory = parent(&from);
        let dots = specifier.bytes().take_while(|ch| *ch == b'.').count();
        if dots > 0 {
            let mut base = directory;
            for _ in 1..dots {
                if self.packages.contains(base) && !self.packages.contains(parent(base))
                    || base.is_empty()
                {
                    return None;
                }
                base = parent(base);
            }
            let rest = &specifier[dots..];
            if !rest.is_empty() {
                return self.under(base, &rest.replace('.', "/"));
            }
            let init = join(base, "__init__.py");
            return self.files.contains(&init).then_some(init);
        }
        let relative = specifier.replace('.', "/");
        let head = specifier.split('.').next().unwrap();
        let mut containing = Vec::new();
        let mut ancestor = Some(directory);
        while let Some(dir) = ancestor {
            let child = join(dir, head);
            if self.projects.contains(dir)
                || self.local.contains_key(dir)
                || (!self.packages.contains(dir)
                    && (dir == directory
                        || directory == child
                        || directory.starts_with(&format!("{child}/"))))
            {
                containing.push(dir);
            }
            ancestor = if dir.is_empty() {
                None
            } else {
                Some(parent(dir))
            };
        }
        let mut local_head = false;
        for root in &containing {
            if let Some(path) = self.under(root, &relative) {
                return Some(path);
            }
            local_head |= self.head(root, head) == Some(true);
        }
        if local_head {
            return None;
        }
        let reaches = |root: &str| {
            !containing.contains(&root)
                && (self.projects.contains(root)
                    || self
                        .local
                        .get(root)
                        .is_some_and(|scope| within(directory, scope)))
        };
        let mut cache = self.heads.borrow_mut();
        let heads = cache.entry(head.into()).or_insert_with(|| {
            self.projects
                .iter()
                .chain(self.local.keys())
                .filter_map(|root| {
                    self.head(root, head)
                        .map(|concrete| (root.clone(), concrete))
                })
                .collect()
        });
        let mut concrete = Vec::new();
        let mut namespaces = Vec::new();
        for (root, is_concrete) in heads.iter().filter(|(root, _)| reaches(root)) {
            if *is_concrete {
                concrete.push(root.as_str());
            } else {
                namespaces.push(root.as_str());
            }
        }
        if !concrete.is_empty() {
            return if concrete.len() == 1 {
                self.under(concrete[0], &relative)
            } else {
                None
            };
        }
        let mut found = None;
        for root in namespaces {
            let Some(path) = self.under(root, &relative) else {
                continue;
            };
            if found.as_ref().is_some_and(|prior| prior != &path) {
                return None;
            }
            found = Some(path);
        }
        if found.is_some() {
            return found;
        }
        self.script(specifier, &relative, &from, directory)
    }
    fn script(
        &self,
        specifier: &str,
        relative: &str,
        from: &str,
        directory: &str,
    ) -> Option<String> {
        if STDLIB
            .split_whitespace()
            .any(|name| Some(name) == specifier.split('.').next())
        {
            return None;
        }
        let taken: Vec<_> = self
            .imports
            .get(from)
            .into_iter()
            .flatten()
            .filter(|import| import.module == specifier && import.imported != "*")
            .map(|import| import.imported.as_str())
            .collect();
        if taken.is_empty() {
            return None;
        }
        let mut dir = directory;
        while !dir.is_empty() {
            if dir != directory && self.packages.contains(dir) {
                dir = parent(dir);
                continue;
            }
            if let Some(file) = self.under(dir, relative)
                && !self
                    .imports
                    .get(&file)
                    .into_iter()
                    .flatten()
                    .any(|import| import.module == specifier)
                && self
                    .defined
                    .get(&file)
                    .is_some_and(|defined| taken.iter().all(|name| defined.contains(*name)))
            {
                return Some(file);
            }
            dir = parent(dir);
        }
        None
    }
}

fn tables(source: &str) -> Result<Vec<(String, &str)>> {
    let regex = regex(r"(?m)^[ \t]*\[([^\[\]\n]+)\][ \t]*(?:#.*)?$")?;
    let headers = regex
        .captures_iter(source)
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut out = Vec::new();
    for (index, header) in headers.iter().enumerate() {
        out.push((
            trim(&header[1]).replace(['\'', '"'], ""),
            &source[header.get(0).unwrap().end()
                ..headers
                    .get(index + 1)
                    .map_or(source.len(), |next| next.get(0).unwrap().start())],
        ));
    }
    Ok(out)
}
fn declared_roots(service: &str, manifests: &BTreeMap<String, String>) -> Result<Vec<String>> {
    let mut out = Vec::new();
    let mut add = |raw: &str| {
        let raw = unquote(trim(raw));
        let raw = raw
            .strip_prefix("./")
            .or_else(|| (raw == ".").then_some(""))
            .unwrap_or(raw)
            .trim_end_matches('/');
        if !raw.is_empty() && !raw.starts_with("..") && !raw.starts_with('/') {
            out.push(join(service, raw));
        }
    };
    if let Some(source) = manifests.get(&join(service, "pyproject.toml")) {
        for (table, body) in tables(source)? {
            match table.as_str() {
                "tool.setuptools.packages.find" => {
                    if let Some(m) = regex(r"(?m)^\s*where\s*=\s*\[([^\]]*)\]")?.captures(body)? {
                        for item in m[1].split(',') {
                            add(item);
                        }
                    }
                }
                "tool.setuptools.package-dir" => {
                    if let Some(m) =
                        regex(r#"(?m)^\s*["']{2}\s*=\s*["']([^"']+)["']"#)?.captures(body)?
                    {
                        add(&m[1]);
                    }
                }
                "tool.setuptools" => {
                    if let Some(m) =
                        regex(r#"package-dir\s*=\s*\{[^}]*?["']{2}\s*=\s*["']([^"']+)["']"#)?
                            .captures(body)?
                    {
                        add(&m[1]);
                    }
                }
                "tool.poetry" => {
                    if let Some(m) =
                        regex(r"(?m)^\s*packages\s*=\s*\[([\s\S]*?)\]\s*$")?.captures(body)?
                    {
                        for m in regex(r#"\bfrom\s*=\s*["']([^"']+)["']"#)?.captures_iter(&m[1]) {
                            add(&m?[1]);
                        }
                    }
                }
                "tool.hatch.build.targets.wheel" => {
                    if let Some(m) =
                        regex(r"(?m)^\s*packages\s*=\s*\[([^\]]*)\]")?.captures(body)?
                    {
                        for item in m[1].split(',') {
                            let item = unquote(trim(item));
                            if item.contains('/') {
                                add(parent(item));
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
    if let Some(source) = manifests.get(&join(service, "setup.cfg")) {
        for (table, body) in tables(source)? {
            let pattern = match table.as_str() {
                "options" => r"(?m)^\s*package_dir\s*=\s*(?:\n\s+)?=\s*(\S+)\s*$",
                "options.packages.find" => r"(?m)^\s*where\s*=\s*(\S+)\s*$",
                _ => continue,
            };
            if let Some(m) = regex(pattern)?.captures(body)? {
                add(&m[1]);
            }
        }
    }
    Ok(out)
}

const STDLIB: &str = include_str!("python_stdlib.txt");

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn python_modules_match_independent_reference() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/upstream/resolution/python-modules/cases.json"
        ))
        .unwrap();
        for project in fixture["projects"].as_array().unwrap() {
            let paths: Vec<String> = serde_json::from_value(project["paths"].clone()).unwrap();
            let manifests = serde_json::from_value(project["files"].clone()).unwrap();
            let nodes = serde_json::from_value::<Vec<Node>>(project["nodes"].clone()).unwrap();
            let imports = serde_json::from_value(project["imports"].clone()).unwrap();
            let modules = PythonModules::new(paths, &manifests, &nodes, imports).unwrap();
            for query in project["queries"].as_array().unwrap() {
                assert_eq!(
                    modules
                        .resolve(
                            query["specifier"].as_str().unwrap(),
                            query["from"].as_str().unwrap()
                        )
                        .as_deref(),
                    query["expected"].as_str(),
                    "{} {query}",
                    project["name"]
                );
            }
        }
    }
}
