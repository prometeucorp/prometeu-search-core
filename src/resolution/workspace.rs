//! Workspace and package-entry resolution from committed manifest snapshots.
#[path = "workspace/jsonc.rs"]
pub(super) mod jsonc;
use anyhow::{Context, Result, ensure};
use regex::Regex;
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};
use std::sync::LazyLock;

pub(crate) type Manifests = BTreeMap<String, String>;
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspacePackages {
    pub by_name: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub entry_by_name: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub entries_by_dir: BTreeMap<String, PackageEntries>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub scoped_by_name: BTreeMap<String, BTreeMap<String, String>>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub nested_roots: Vec<String>,
}
#[derive(Debug, Default, Serialize, Deserialize)]
pub(crate) struct PackageEntries {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exports: Option<Vec<(String, Vec<String>)>>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub browser: Vec<(String, Vec<String>)>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub main: Vec<String>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ImportTargets {
    pub declared: Vec<String>,
    pub browser: Vec<String>,
    pub directory: String,
    pub package_dir: String,
}
fn join(dir: &str, file: &str) -> String {
    if dir.is_empty() {
        file.into()
    } else {
        format!("{dir}/{file}")
    }
}
pub(super) fn browser_source(source: &str) -> Result<bool> {
    let safe = crate::synthesis::comments::blank_string_contents(
        &crate::synthesis::comments::strip_for_regex(source, "typescript"),
    )?;
    for name in ["document", "window"] {
        for (offset, _) in safe.match_indices(name) {
            if safe[..offset]
                .chars()
                .next_back()
                .is_some_and(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '$' | '.'))
            {
                continue;
            }
            if safe[offset + name.len()..]
                .trim_start_matches(crate::search::query_whitespace)
                .starts_with(['.', '['])
            {
                return Ok(true);
            }
        }
    }
    Ok(false)
}
fn trim(value: &str) -> &str {
    value.trim_matches(crate::search::query_whitespace)
}
fn json(manifests: &Manifests, dir: &str) -> Option<Value> {
    serde_json::from_str(manifests.get(&normalize(&join(dir, "package.json")))?).ok()
}
fn insert_unique(values: &mut Vec<String>, value: String) {
    if !values.contains(&value) {
        values.push(value);
    }
}
fn js_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}
pub(super) fn entries(value: &Value) -> Vec<(String, &Value)> {
    let mut out: Vec<_> = match value {
        Value::Object(map) => map.iter().map(|(k, v)| (k.clone(), v)).collect(),
        Value::Array(items) => items
            .iter()
            .enumerate()
            .map(|(i, v)| (i.to_string(), v))
            .collect(),
        _ => Vec::new(),
    };
    let index = |s: &str| {
        s.parse::<u32>()
            .ok()
            .filter(|&i| i < u32::MAX && i.to_string() == s)
    };
    out.sort_by(|(a, _), (b, _)| match (index(a), index(b)) {
        (Some(a), Some(b)) => a.cmp(&b),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        _ => std::cmp::Ordering::Equal,
    });
    out
}
/// POSIX lexical resolution, including absolute targets within the pinned root.
pub(super) fn target(root: &str, dir: &str, value: &str) -> Option<String> {
    let abs = if value.starts_with('/') {
        value.into()
    } else {
        format!("{root}/{dir}/{value}")
    };
    let normalized = normalize(&abs);
    let root = normalize(root);
    let relative = if normalized == root {
        String::new()
    } else {
        normalized
            .strip_prefix(&format!("{}/", root.trim_end_matches('/')))?
            .into()
    };
    (!relative.starts_with("..")).then(|| relative.replace('\\', "/"))
}
fn normalize(value: &str) -> String {
    let absolute = value.starts_with('/');
    let mut parts = Vec::new();
    for part in value.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.last().is_some_and(|p| *p != "..") {
                    parts.pop();
                } else if !absolute {
                    parts.push("..");
                }
            }
            _ => parts.push(part),
        }
    }
    format!("{}{}", if absolute { "/" } else { "" }, parts.join("/"))
}
fn package_target(root: &str, dir: &str, value: &str) -> Option<String> {
    let value = trim(value);
    if value.is_empty() {
        return None;
    }
    let normalized_dir = normalize(dir);
    let dir = normalized_dir.as_str();
    let resolved = target(root, dir, value)?;
    let inside = if dir.is_empty() {
        resolved.as_str()
    } else if resolved == dir {
        ""
    } else {
        resolved.strip_prefix(&format!("{dir}/"))?
    };
    (!inside.starts_with("..")).then_some(resolved)
}
pub(super) fn read_globs(manifests: &Manifests, dir: &str) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(pkg) = json(manifests, dir)
        && let Some(ws) = pkg.get("workspaces")
    {
        let values = ws
            .as_array()
            .or_else(|| ws.get("packages").and_then(Value::as_array));
        if let Some(values) = values {
            out.extend(values.iter().filter_map(Value::as_str).map(str::to_string));
        }
    }
    if let Some(yaml) = manifests.get(&join(dir, "pnpm-workspace.yaml")) {
        let mut active = false;
        for line in yaml.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)) {
            if line
                .trim_start_matches(crate::search::query_whitespace)
                .strip_prefix("packages")
                .is_some_and(|rest| {
                    rest.trim_start_matches(crate::search::query_whitespace)
                        .starts_with(':')
                })
            {
                active = true;
                continue;
            }
            if !active {
                continue;
            }
            if let Some(item) = trim(line)
                .strip_prefix('-')
                .map(trim)
                .filter(|v| !v.is_empty() && !v.contains(['\n', '\r', '\u{2028}', '\u{2029}']))
            {
                let item = item.strip_prefix(['\'', '"']).unwrap_or(item);
                let item = item.strip_suffix(['\'', '"']).unwrap_or(item);
                out.push(item.into());
            } else if !trim(line).is_empty() && !line.starts_with(crate::search::query_whitespace) {
                active = false;
            }
        }
    }
    out
}
pub(super) fn npm_dependencies(pkg: &Value) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for field in [
        "dependencies",
        "devDependencies",
        "optionalDependencies",
        "peerDependencies",
    ] {
        if let Some(deps) = pkg.get(field) {
            for (name, value) in entries(deps) {
                let Some(value) = value.as_str().map(trim) else {
                    continue;
                };
                if let Some((protocol, path)) = value.split_once(':')
                    && matches!(protocol, "file" | "link" | "portal")
                    && !trim(path).is_empty()
                    && !path.contains(['\n', '\r', '\u{2028}', '\u{2029}'])
                {
                    out.push((name, trim(path).into()));
                }
            }
        }
    }
    out
}
pub(super) fn ohpm_dependencies(source: &str) -> Result<Vec<(String, String)>> {
    let mut out = Vec::new();
    if let Some(pkg) = jsonc::parse(source)?
        && let Some(deps) = pkg.get("dependencies")
    {
        for (name, value) in entries(deps) {
            if let Some(value) = value
                .as_str()
                .and_then(|s| s.strip_prefix("file:"))
                .map(trim)
                .filter(|v| !v.is_empty())
            {
                out.push((name, value.into()));
            }
        }
    }
    Ok(out)
}
fn export_targets(value: &Value, depth: usize) -> Result<Vec<String>> {
    ensure!(depth <= 128, "workspace export nesting exceeds 128 levels");
    if let Some(value) = value.as_str() {
        return Ok(vec![value.into()]);
    }
    let mut out = Vec::new();
    if let Some(values) = value.as_array() {
        for value in values {
            out.extend(export_targets(value, depth + 1)?);
        }
    } else {
        let conditions = entries(value);
        for tier in 0..3 {
            for (key, value) in &conditions {
                let rank = match key.as_str() {
                    "import" | "require" | "node" | "module" | "module-sync" | "default" => 0,
                    "types" | "typings" => 2,
                    _ => 1,
                };
                if tier == rank {
                    out.extend(export_targets(value, depth + 1)?);
                }
            }
        }
    }
    Ok(out)
}
fn browser_targets(value: &Value, depth: usize) -> Result<Vec<String>> {
    ensure!(depth <= 128, "workspace browser nesting exceeds 128 levels");
    if !value.is_object() {
        return Ok(Vec::new());
    }
    if let Some(browser) = value.get("browser") {
        return export_targets(browser, depth + 1);
    }
    let mut out = Vec::new();
    for (_, value) in entries(value) {
        out.extend(browser_targets(value, depth + 1)?);
    }
    Ok(out)
}
fn package_entries(manifests: &Manifests, root: &str, dir: &str) -> Result<Option<PackageEntries>> {
    let Some(pkg) = json(manifests, dir) else {
        return Ok(None);
    };
    let mut out = PackageEntries::default();
    if let Some(exports) = pkg.get("exports").filter(|v| !v.is_null()) {
        let pairs = if exports.is_object()
            && entries(exports).iter().any(|(key, _)| key.starts_with('.'))
        {
            entries(exports)
                .into_iter()
                .filter(|(key, _)| key.starts_with('.'))
                .collect()
        } else {
            vec![(".".into(), exports)]
        };
        let mut entries_out = Vec::new();
        for (key, value) in pairs {
            let mut targets = Vec::new();
            for target in export_targets(value, 0)? {
                if let Some(target) = package_target(root, dir, &target) {
                    insert_unique(&mut targets, target);
                }
            }
            entries_out.push((key.clone(), targets));
            let browser: Vec<_> = browser_targets(value, 0)?
                .into_iter()
                .filter_map(|target| package_target(root, dir, &target))
                .collect();
            if !browser.is_empty() {
                out.browser.push((key, browser));
            }
        }
        out.exports = Some(entries_out);
    }
    for field in ["source", "module", "main", "types", "typings"] {
        if let Some(target) = pkg
            .get(field)
            .and_then(Value::as_str)
            .and_then(|value| package_target(root, dir, value))
        {
            insert_unique(&mut out.main, target);
        }
    }
    Ok((out.exports.is_some() || !out.main.is_empty()).then_some(out))
}

struct Segment {
    text: String,
    globstar: bool,
    wildcard: bool,
    pattern: Option<Regex>,
}
fn utf16_view(value: &str) -> String {
    value
        .encode_utf16()
        .map(|unit| {
            char::from_u32(if (0xd800..=0xdfff).contains(&unit) {
                0xf0000 + unit as u32
            } else {
                unit as u32
            })
            .unwrap()
        })
        .collect()
}
fn compile(pattern: &str) -> Result<Vec<Segment>> {
    static ALTERNATIVES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\{([^{}]*)\}").unwrap());
    pattern
        .split('/')
        .filter(|s| !s.is_empty() && *s != ".")
        .map(|text| {
            let globstar = text == "**";
            let wildcard = text.contains(['*', '?', '{']);
            let pattern = if wildcard && !globstar {
                let mut source = String::new();
                let mut previous_star = false;
                for ch in utf16_view(text).chars() {
                    if ch == '*' {
                        if !previous_star {
                            source.push_str("[^/]*");
                        }
                        previous_star = true;
                        continue;
                    }
                    previous_star = false;
                    if ch == '?' {
                        source.push_str("[^/]");
                    } else {
                        if ".+^$()|[]\\".contains(ch) {
                            source.push('\\');
                        }
                        source.push(ch);
                    }
                }
                let source = ALTERNATIVES
                    .replace_all(&source, |cap: &regex::Captures<'_>| {
                        format!("(?:{})", cap[1].replace(',', "|"))
                    })
                    .replace('{', r"\{")
                    .replace('}', r"\}");
                Some(Regex::new(&format!("^{source}$")).context("invalid workspace pattern")?)
            } else {
                None
            };
            Ok(Segment {
                text: text.into(),
                globstar,
                wildcard,
                pattern,
            })
        })
        .collect()
}
fn closure(glob: &[Segment], mut states: BTreeSet<usize>) -> BTreeSet<usize> {
    for state in states.clone() {
        let mut next = state;
        while glob.get(next).is_some_and(|s| s.globstar) {
            next += 1;
            states.insert(next);
        }
    }
    states
}
fn step(glob: &[Segment], states: &BTreeSet<usize>, name: &str, walking: bool) -> BTreeSet<usize> {
    let mut next = BTreeSet::new();
    let hidden =
        walking && (name.starts_with('.') || matches!(name, "node_modules" | "bower_components"));
    for &state in states {
        if let Some(seg) = glob.get(state) {
            if seg.globstar {
                if !hidden && !(walking && matches!(name, "test" | "tests")) {
                    next.insert(state);
                }
            } else if seg.pattern.as_ref().map_or(seg.text == name, |pattern| {
                !hidden && pattern.is_match(&utf16_view(name))
            }) {
                next.insert(state + 1);
            }
        }
    }
    closure(glob, next)
}
pub(super) fn expand_glob(
    pattern: &str,
    mut read_dir: impl FnMut(&str) -> Result<Vec<String>>,
) -> Result<Vec<String>> {
    let norm = pattern.replace('\\', "/").trim_end_matches('/').to_string();
    let glob = compile(&norm)?;
    let Some(first) = glob.iter().position(|s| s.wildcard) else {
        return Ok(vec![norm]);
    };
    let base = glob[..first]
        .iter()
        .map(|s| s.text.as_str())
        .collect::<Vec<_>>()
        .join("/");
    let start = closure(&glob, BTreeSet::from([first]));
    let mut out = if start.contains(&glob.len()) {
        vec![if base.is_empty() {
            ".".into()
        } else {
            base.clone()
        }]
    } else {
        Vec::new()
    };
    let mut level = vec![(base, start)];
    let mut read = 0;
    for _ in 0..=8 {
        if level.is_empty() {
            break;
        }
        let mut next = Vec::new();
        for (rel, states) in level {
            read += 1;
            if read > 10_000 {
                return Ok(out);
            }
            let mut names = read_dir(&rel)?;
            names.sort_by(|a, b| js_cmp(a, b));
            for name in names {
                let advanced = step(&glob, &states, &name, true);
                if advanced.is_empty() {
                    continue;
                }
                let child = join(&rel, &name);
                if advanced.contains(&glob.len()) {
                    out.push(child.clone());
                }
                if advanced.iter().any(|&s| s < glob.len()) {
                    next.push((child, advanced));
                }
            }
        }
        level = next;
    }
    Ok(out)
}
pub(super) fn glob_matches(pattern: &str, dir: &str) -> Result<bool> {
    let pattern = pattern.replace('\\', "/");
    let glob = compile(pattern.trim_end_matches('/'))?;
    let mut states = closure(&glob, BTreeSet::from([0]));
    for name in dir.split('/').filter(|p| !p.is_empty() && *p != ".") {
        states = step(&glob, &states, name, false);
        if states.is_empty() {
            return Ok(false);
        }
    }
    Ok(states.contains(&glob.len()))
}
pub(super) fn skip_dependency_dir(name: &str) -> bool {
    name.starts_with('.')
        || matches!(
            name,
            "node_modules" | "oh_modules" | "build" | "dist" | "out" | "oh-package-lock.json5"
        )
}
pub(super) fn directory_children(directories: &[String]) -> BTreeMap<String, Vec<String>> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for dir in directories {
        if dir.is_empty() {
            continue;
        }
        let (parent, name) = dir.rsplit_once('/').unwrap_or(("", dir));
        out.entry(parent.into()).or_default().push(name.into());
    }
    for names in out.values_mut() {
        names.sort_by(|a, b| js_cmp(a, b));
        names.dedup();
    }
    out
}
pub(crate) fn load_manifests(connection: &Connection) -> Result<Manifests> {
    let manifests: Option<String> = connection
        .query_row(
            "SELECT value FROM metadata WHERE key='project_manifests'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    let Some(manifests) = manifests else {
        return Ok(BTreeMap::new());
    };
    ensure!(
        manifests.len() <= 64 * 1024 * 1024,
        "workspace manifest snapshot exceeds limit"
    );
    let manifests: Manifests = serde_json::from_str(&manifests)?;
    ensure!(
        manifests.values().map(String::len).sum::<usize>() <= 8 * 1024 * 1024,
        "workspace manifest contents exceed 8 MiB"
    );
    Ok(manifests)
}
impl WorkspacePackages {
    pub(crate) fn load(connection: &Connection, manifests: &Manifests) -> Result<Option<Self>> {
        let directories: Option<String> = connection
            .query_row(
                "SELECT value FROM metadata WHERE key='project_directories'",
                [],
                |r| r.get(0),
            )
            .optional()?;
        let directories: Vec<String> = if let Some(directories) = directories {
            ensure!(
                directories.len() <= 16 * 1024 * 1024,
                "workspace directory snapshot exceeds limit"
            );
            serde_json::from_str(&directories)?
        } else {
            Vec::new()
        };
        let root: String =
            connection.query_row("SELECT value FROM metadata WHERE key='root'", [], |r| {
                r.get(0)
            })?;
        Self::from_snapshot(manifests, &directories, &root)
    }
    pub(super) fn from_snapshot(
        manifests: &Manifests,
        directories: &[String],
        root: &str,
    ) -> Result<Option<Self>> {
        let mut out = Self::default();
        let children = directory_children(directories);
        let directories: HashSet<_> = directories
            .iter()
            .map(String::as_str)
            .chain(std::iter::once(""))
            .collect();
        let patterns = read_globs(manifests, "");
        let excluded: Vec<_> = patterns
            .iter()
            .filter_map(|p| p.strip_prefix('!'))
            .collect();
        for pattern in patterns.iter().filter(|p| !p.starts_with('!')) {
            for dir in expand_glob(pattern, |dir| {
                Ok(children.get(dir).cloned().unwrap_or_default())
            })? {
                let mut skip = false;
                for pattern in &excluded {
                    if glob_matches(pattern, &dir)? {
                        skip = true;
                        break;
                    }
                }
                if skip {
                    continue;
                }
                let key = normalize(&dir);
                if let Some(pkg) = json(manifests, &key)
                    && let Some(name) = pkg
                        .get("name")
                        .and_then(Value::as_str)
                        .filter(|name| !name.is_empty())
                {
                    out.by_name.entry(name.into()).or_insert(dir);
                }
            }
        }
        let mut ohpm: BTreeMap<String, String> = BTreeMap::new();
        let mut ambiguous = HashSet::new();
        let mut npm: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
        let mut workspace_roots = Vec::new();
        let mut queue = VecDeque::from([(String::new(), 0)]);
        let mut visited = 0;
        while let Some((rel, depth)) = queue.pop_front() {
            visited += 1;
            if visited > 8000 {
                break;
            }
            if depth < 6 {
                for name in children.get(&rel).into_iter().flatten() {
                    if !skip_dependency_dir(name) {
                        queue.push_back((join(&rel, name), depth + 1));
                    }
                }
            }
            if !rel.is_empty()
                && manifests.contains_key(&join(&rel, "package.json"))
                && !read_globs(manifests, &rel).is_empty()
            {
                workspace_roots.push(rel.clone());
            }
            if let Some(pkg) = json(manifests, &rel) {
                let mut deps = BTreeMap::new();
                for (name, path) in npm_dependencies(&pkg) {
                    if let Some(path) = target(root, &rel, &path)
                        && !path.is_empty()
                        && !path.split('/').any(|p| p == "node_modules")
                        && directories.contains(path.as_str())
                    {
                        deps.insert(name, path);
                    }
                }
                if !deps.is_empty() {
                    npm.insert(rel.clone(), deps);
                }
            }
            if let Some(source) = manifests.get(&join(&rel, "oh-package.json5")) {
                for (name, path) in ohpm_dependencies(source)? {
                    let Some(path) = target(root, &rel, &path) else {
                        continue;
                    };
                    if let Some(old) = ohpm.get(&name) {
                        if old != &path {
                            ohpm.remove(&name);
                            ambiguous.insert(name);
                        }
                    } else if !ambiguous.contains(&name) {
                        ohpm.insert(name, path);
                    }
                }
            }
        }
        for (name, dir) in ohpm {
            if out.by_name.contains_key(&name) {
                continue;
            }
            if let Some(source) = manifests.get(&join(&dir, "oh-package.json5"))
                && let Some(pkg) = jsonc::parse(source)?
                && let Some(main) = pkg
                    .get("main")
                    .and_then(Value::as_str)
                    .map(trim)
                    .filter(|s| !s.is_empty())
                && let Some(entry) = target(root, &dir, main)
            {
                out.entry_by_name.insert(name.clone(), entry);
            }
            out.by_name.insert(name, dir);
        }
        if let Some(root_deps) = npm.remove("") {
            for (name, dir) in root_deps {
                out.by_name.entry(name).or_insert(dir);
            }
        }
        let members: HashSet<_> = out.by_name.values().collect();
        out.nested_roots = workspace_roots
            .into_iter()
            .filter(|dir| !members.contains(dir))
            .collect();
        if out.by_name.is_empty() && npm.is_empty() && out.nested_roots.is_empty() {
            return Ok(None);
        }
        let package_dirs: BTreeSet<_> = out
            .by_name
            .values()
            .chain(npm.values().flat_map(|names| names.values()))
            .collect();
        for dir in package_dirs {
            if let Some(entries) = package_entries(manifests, root, dir)? {
                out.entries_by_dir.insert(dir.clone(), entries);
            }
        }
        out.scoped_by_name = npm;
        Ok(Some(out))
    }
    pub(super) fn leaves_root(&self, file: &str, target: &str) -> bool {
        let file = file.replace('\\', "/");
        let target = target.replace('\\', "/");
        self.nested_roots
            .iter()
            .filter(|dir| file.starts_with(&format!("{dir}/")))
            .max_by_key(|dir| dir.encode_utf16().count())
            .is_some_and(|root| !target.starts_with(&format!("{root}/")))
    }
    fn package<'a>(&'a self, import: &str, from: Option<&str>) -> Option<(&'a str, &'a str)> {
        let longest = |names: &'a BTreeMap<String, String>| {
            names
                .iter()
                .filter(|(name, _)| {
                    import == name.as_str() || import.starts_with(&format!("{name}/"))
                })
                .max_by_key(|(name, _)| name.encode_utf16().count())
                .map(|(n, d)| (n.as_str(), d.as_str()))
        };
        if let Some(from) = from.filter(|s| !s.is_empty()) {
            let from = from.replace('\\', "/");
            let mut dir = from.as_str();
            while !dir.is_empty() {
                dir = dir.rsplit_once('/').map_or("", |(dir, _)| dir);
                if let Some(names) = self.scoped_by_name.get(dir)
                    && let Some(found) = longest(names)
                {
                    return Some(found);
                }
            }
        }
        longest(&self.by_name)
            .filter(|(_, dir)| from.is_none_or(|from| !self.leaves_root(from, dir)))
    }
    pub(super) fn targets(&self, import: &str, from: Option<&str>) -> Option<ImportTargets> {
        let (name, dir) = self.package(import, from)?;
        let subpath = &import[name.len()..];
        let directory = collapse_slashes(&format!("{dir}{subpath}"));
        let entries = self.entries_by_dir.get(dir);
        let mut declared =
            if let Some(entry) = self.entry_by_name.get(name).filter(|_| subpath.is_empty()) {
                vec![entry.clone()]
            } else {
                let mut out = entries
                    .and_then(|e| e.exports.as_ref())
                    .map_or_else(Vec::new, |exports| {
                        exported_targets(exports, &format!(".{subpath}"))
                    });
                if subpath.is_empty()
                    && let Some(entries) = entries
                {
                    for target in &entries.main {
                        insert_unique(&mut out, target.clone());
                    }
                }
                out
            };
        let browser = entries.map_or_else(Vec::new, |e| {
            exported_targets(&e.browser, &format!(".{subpath}"))
        });
        Some(ImportTargets {
            declared: std::mem::take(&mut declared),
            browser,
            directory,
            package_dir: dir.into(),
        })
    }
    pub(super) fn resolve(&self, import: &str, from: Option<&str>) -> Option<String> {
        let (name, dir) = self.package(import, from)?;
        let tail = &import[name.len()..];
        Some(
            self.entry_by_name
                .get(name)
                .filter(|_| tail.is_empty())
                .cloned()
                .unwrap_or_else(|| collapse_slashes(&format!("{dir}{tail}"))),
        )
    }
    pub(super) fn module_dirs(&self) -> Vec<String> {
        self.by_name
            .values()
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }
    #[cfg(test)]
    pub(super) fn package_dir<'a>(&'a self, file: &str) -> Option<&'a str> {
        self.by_name
            .values()
            .chain(self.scoped_by_name.values().flat_map(|m| m.values()))
            .filter(|dir| !dir.is_empty() && file.starts_with(&format!("{dir}/")))
            .min_by_key(|dir| dir.encode_utf16().count())
            .map(String::as_str)
    }
}
fn collapse_slashes(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        if ch != '/' || !out.ends_with('/') {
            out.push(ch);
        }
    }
    out
}
fn exported_targets(exports: &[(String, Vec<String>)], key: &str) -> Vec<String> {
    if let Some((_, targets)) = exports.iter().find(|(name, _)| name == key) {
        return targets.clone();
    }
    let mut best: Option<(usize, &str, &Vec<String>)> = None;
    for (pattern, targets) in exports {
        let Some((prefix, suffix)) = pattern.split_once('*') else {
            continue;
        };
        if suffix.contains('*')
            || key.len() < prefix.len() + suffix.len()
            || !key.starts_with(prefix)
            || !key.ends_with(suffix)
        {
            continue;
        }
        let length = prefix.encode_utf16().count();
        if best.is_some_and(|(old, _, _)| length <= old) {
            continue;
        }
        best = Some((
            length,
            &key[prefix.len()..key.len() - suffix.len()],
            targets,
        ));
    }
    best.map_or_else(Vec::new, |(_, matched, targets)| {
        targets
            .iter()
            .map(|target| target.replace('*', matched))
            .collect()
    })
}

pub(super) fn esm(language: &str) -> bool {
    matches!(
        language,
        "typescript" | "tsx" | "javascript" | "jsx" | "arkts" | "vue" | "svelte" | "astro"
    )
}
fn extensions(language: &str) -> &'static [&'static str] {
    match language {
        "typescript" => &[
            ".ts",
            ".tsx",
            ".d.ts",
            ".js",
            ".jsx",
            "/index.ts",
            "/index.tsx",
            "/index.js",
            "/index.jsx",
        ],
        "arkts" => &[
            ".ets",
            ".ts",
            ".d.ts",
            ".js",
            "/Index.ets",
            "/index.ets",
            "/index.ts",
            "/index.js",
        ],
        "javascript" => &[
            ".js",
            ".jsx",
            ".mjs",
            ".cjs",
            ".xsjs",
            ".xsjslib",
            ".ts",
            ".tsx",
            "/index.js",
            "/index.jsx",
            "/index.ts",
            "/index.tsx",
        ],
        "tsx" => &[
            ".tsx",
            ".ts",
            ".d.ts",
            ".js",
            ".jsx",
            "/index.tsx",
            "/index.ts",
            "/index.js",
            "/index.jsx",
        ],
        "jsx" => &[
            ".jsx",
            ".js",
            ".tsx",
            ".ts",
            "/index.jsx",
            "/index.js",
            "/index.tsx",
            "/index.ts",
        ],
        "svelte" => &[
            ".ts",
            ".js",
            ".svelte",
            ".tsx",
            ".jsx",
            "/index.ts",
            "/index.js",
            "/index.svelte",
        ],
        "vue" => &[
            ".ts",
            ".js",
            ".vue",
            ".tsx",
            ".jsx",
            "/index.ts",
            "/index.js",
            "/index.vue",
        ],
        "astro" => &[
            ".ts",
            ".js",
            ".astro",
            ".tsx",
            ".jsx",
            "/index.ts",
            "/index.js",
            "/index.astro",
        ],
        _ => &[],
    }
}
fn emitted(target: &str, exists: &impl Fn(&str) -> bool) -> Option<String> {
    for (extension, sources) in [
        (".js", &[".ts", ".tsx", ".d.ts"][..]),
        (".jsx", &[".tsx"][..]),
        (".mjs", &[".mts", ".d.mts"][..]),
        (".cjs", &[".cts", ".d.cts"][..]),
    ] {
        if let Some(stem) = target.strip_suffix(extension) {
            return sources
                .iter()
                .map(|ext| format!("{stem}{ext}"))
                .find(|path| exists(path));
        }
    }
    None
}
pub(super) fn indexed(
    target: &str,
    language: &str,
    exists: &impl Fn(&str) -> bool,
    as_is_first: bool,
) -> Option<String> {
    let as_is = normalize(target);
    if as_is_first && exists(&as_is) {
        return Some(as_is);
    }
    for extension in extensions(language) {
        let candidate = if target.is_empty() {
            let Some(index) = extension.strip_prefix('/') else {
                continue;
            };
            index.to_string()
        } else {
            normalize(&format!("{target}{extension}"))
        };
        if exists(&candidate) {
            return Some(candidate);
        }
    }
    if !as_is_first && exists(&as_is) {
        return Some(as_is);
    }
    emitted(&as_is, exists)
}
fn built_sources(target: &str, package: &str) -> Vec<String> {
    static ROOT: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^(?:dist|build|lib|out|output|es|esm|cjs|umd)/").unwrap());
    static FORMAT: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r"^(?:esm|cjs|es|mjs|module|modules|commonjs|types|typings|umd|node|browser|src)/",
        )
        .unwrap()
    });
    static EXT: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?:\.d)?\.(?:[cm]?[jt]s|[jt]sx)$").unwrap());
    let prefix = if package.is_empty() {
        String::new()
    } else {
        format!("{package}/")
    };
    let Some(path) = target.strip_prefix(&prefix) else {
        return Vec::new();
    };
    let Some(root) = ROOT.find(path) else {
        return Vec::new();
    };
    let mirrored = &path[root.end()..];
    let mut stems = vec![mirrored];
    if let Some(format) = FORMAT.find(mirrored) {
        stems.push(&mirrored[format.end()..]);
    }
    let mut out = Vec::new();
    for stem in stems {
        let stem = EXT.replace(stem, "");
        if !stem.is_empty() && !stem.starts_with("src/") {
            insert_unique(&mut out, format!("{prefix}src/{stem}"));
        }
    }
    out
}
impl ImportTargets {
    pub(super) fn indexed(
        &self,
        language: &str,
        browser: bool,
        exists: impl Fn(&str) -> bool,
    ) -> Option<String> {
        let declared = |targets: &[String]| {
            targets.iter().find_map(|target| {
                indexed(target, language, &exists, true).or_else(|| {
                    built_sources(target, &self.package_dir)
                        .iter()
                        .find_map(|source| indexed(source, language, &exists, true))
                })
            })
        };
        if browser && let Some(found) = declared(&self.browser) {
            return Some(found);
        }
        declared(&self.declared)
            .or_else(|| indexed(&self.directory, language, &exists, false))
            .or_else(|| {
                built_sources(&self.directory, &self.package_dir)
                    .iter()
                    .find_map(|source| indexed(source, language, &exists, true))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn corpus() -> Value {
        serde_json::from_str(include_str!(
            "../../tests/fixtures/upstream/workspace/cases.json"
        ))
        .unwrap()
    }
    #[test]
    fn relative_and_conventional_alias_modules_follow_reference_order() {
        let corpus: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/upstream/workspace/modules.json"
        ))
        .unwrap();
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("project");
        std::fs::create_dir(&root).unwrap();
        for file in corpus["files"].as_array().unwrap() {
            let path = root.join(file.as_str().unwrap());
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "").unwrap();
        }
        let mut engine =
            crate::Engine::open(&root, Some(&temporary.path().join("search.sqlite"))).unwrap();
        engine.sync().unwrap();
        let graph =
            crate::graph::Graph::load(&engine.connection, engine.status().unwrap().generation)
                .unwrap();
        for case in corpus["cases"].as_array().unwrap() {
            let actual = graph.module_files_as(
                case["from"].as_str().unwrap(),
                case["source"].as_str().unwrap(),
                case["language"].as_str().unwrap(),
            );
            let expected: Vec<_> = case["expected"].as_str().into_iter().collect();
            assert_eq!(actual, expected, "{case}");
        }
    }

    #[test]
    fn manifests_and_import_targets_match_independent_reference() {
        let corpus = corpus();
        for case in corpus["cases"].as_array().unwrap() {
            let files: Manifests = case["files"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(path, source)| {
                    (
                        path.clone(),
                        source.as_str().unwrap().replace("$ROOT", "/fixture"),
                    )
                })
                .collect();
            let dirs: Vec<String> = serde_json::from_value(case["directories"].clone()).unwrap();
            let loaded = WorkspacePackages::from_snapshot(&files, &dirs, "/fixture").unwrap();
            assert_eq!(
                serde_json::to_value(&loaded).unwrap(),
                case["expected"],
                "{}",
                case["name"]
            );
            for attempt in case["attempts"].as_array().unwrap() {
                let import = attempt["importPath"].as_str().unwrap();
                let from = attempt.get("fromFile").and_then(Value::as_str);
                assert_eq!(
                    serde_json::to_value(loaded.as_ref().and_then(|ws| ws.targets(import, from)))
                        .unwrap(),
                    attempt["targets"],
                    "{} {attempt}",
                    case["name"]
                );
                assert_eq!(
                    serde_json::to_value(loaded.as_ref().and_then(|ws| ws.resolve(import, from)))
                        .unwrap(),
                    attempt["resolved"],
                    "{} {attempt}",
                    case["name"]
                );
            }
            if let Some(ws) = loaded {
                for member in case["membership"].as_array().unwrap() {
                    let file = member["file"].as_str().unwrap();
                    assert_eq!(
                        serde_json::to_value(ws.package_dir(file)).unwrap(),
                        member["packageDir"],
                        "{} {member}",
                        case["name"]
                    );
                    let actual: BTreeSet<_> = dirs
                        .iter()
                        .filter(|dir| ws.leaves_root(file, &format!("{dir}/other.ts")))
                        .map(String::as_str)
                        .collect();
                    let expected: BTreeSet<_> = member["leaves"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|s| s.as_str().unwrap())
                        .collect();
                    assert_eq!(actual, expected, "{} {file}", case["name"]);
                }
            }
        }
    }
    #[test]
    fn indexed_workspace_graphs_match_reference_and_survive_reopen() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/upstream/workspace/projects.json"
        ))
        .unwrap();
        for case in fixture["cases"].as_array().unwrap() {
            let temporary = tempfile::tempdir().unwrap();
            let root = temporary.path().join("project");
            std::fs::create_dir(&root).unwrap();
            let database = temporary.path().join("search.sqlite");
            for (file, source) in case["files"].as_object().unwrap() {
                let path = root.join(file);
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(path, source.as_str().unwrap()).unwrap();
            }
            let mut engine = crate::Engine::open(&root, Some(&database)).unwrap();
            engine.sync().unwrap();
            let expected: BTreeSet<_> = case["edges"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|e| e["kind"] != "contains")
                .map(|e| {
                    (
                        e["source"].as_str().unwrap().to_string(),
                        e["target"].as_str().unwrap().to_string(),
                        e["kind"].as_str().unwrap().to_string(),
                    )
                })
                .collect();
            let check = |engine: &crate::Engine| {
                let graph = crate::graph::Graph::load(
                    &engine.connection,
                    engine.status().unwrap().generation,
                )
                .unwrap();
                let actual: BTreeSet<_> = graph
                    .canonical_edges
                    .iter()
                    .flatten()
                    .filter(|e| e.kind != "contains")
                    .map(|e| (e.source.clone(), e.target.clone(), e.kind.clone()))
                    .collect();
                let labels: std::collections::HashMap<_, _> = case["nodes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|n| {
                        (
                            n["id"].as_str().unwrap(),
                            format!(
                                "{}:{}",
                                n["filePath"].as_str().unwrap(),
                                n["qualifiedName"].as_str().unwrap()
                            ),
                        )
                    })
                    .collect();
                let label = |edge: &(String, String, String)| {
                    (
                        labels
                            .get(edge.0.as_str())
                            .cloned()
                            .unwrap_or_else(|| edge.0.clone()),
                        labels
                            .get(edge.1.as_str())
                            .cloned()
                            .unwrap_or_else(|| edge.1.clone()),
                        edge.2.clone(),
                    )
                };
                assert!(
                    actual == expected,
                    "{} missing={:?} extra={:?}",
                    case["name"],
                    expected.difference(&actual).map(label).collect::<Vec<_>>(),
                    actual.difference(&expected).map(label).collect::<Vec<_>>()
                );
            };
            check(&engine);
            for file in case["files"].as_object().unwrap().keys() {
                std::fs::write(root.join(file), "changed_on_disk").unwrap();
            }
            drop(engine);
            let engine = crate::Engine::open(&root, Some(&database)).unwrap();
            check(&engine);
        }
    }

    #[test]
    fn tolerant_jsonc_values_match_reference_recovery() {
        for case in corpus()["jsonCases"].as_array().unwrap() {
            let actual = jsonc::parse(case["source"].as_str().unwrap())
                .unwrap()
                .unwrap_or(Value::Null);
            // JSON numbers are JavaScript doubles; compare their numeric values recursively.
            fn same(a: &Value, b: &Value) -> bool {
                match (a, b) {
                    (Value::Number(a), Value::Number(b)) => a.as_f64() == b.as_f64(),
                    (Value::Array(a), Value::Array(b)) => {
                        a.len() == b.len() && a.iter().zip(b).all(|(a, b)| same(a, b))
                    }
                    (Value::Object(a), Value::Object(b)) => {
                        a.len() == b.len()
                            && a.iter().all(|(k, a)| b.get(k).is_some_and(|b| same(a, b)))
                    }
                    _ => a == b,
                }
            }
            assert!(
                same(&actual, &case["expected"]),
                "JSONC {case}: got {actual}"
            );
        }
    }
    #[test]
    fn secure_snapshot_preserves_reference_workspace_discovery() {
        for case in corpus()["cases"].as_array().unwrap() {
            let temporary = tempfile::tempdir().unwrap();
            let path = temporary.path().canonicalize().unwrap();
            for dir in case["directories"].as_array().unwrap() {
                std::fs::create_dir_all(path.join(dir.as_str().unwrap())).unwrap();
            }
            for (file, source) in case["files"].as_object().unwrap() {
                std::fs::write(
                    path.join(file),
                    source
                        .as_str()
                        .unwrap()
                        .replace("$ROOT", path.to_str().unwrap()),
                )
                .unwrap();
            }
            let root = crate::source::open_root(&path).unwrap();
            let (manifests, dirs) =
                crate::graph::workspace_snapshot(&root, &path, std::iter::empty()).unwrap();
            let actual =
                WorkspacePackages::from_snapshot(&manifests, &dirs, path.to_str().unwrap())
                    .unwrap();
            assert_eq!(
                serde_json::to_value(actual).unwrap(),
                case["expected"],
                "snapshot {}",
                case["name"]
            );
        }
    }
}
