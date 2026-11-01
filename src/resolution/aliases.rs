//! TypeScript configuration aliases and base URLs from confined committed snapshots.
use super::workspace::{Manifests, entries};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};

pub(super) const CONFIGS: [&str; 3] = ["tsconfig.json", "jsconfig.json", "tsconfig.base.json"];
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Pattern {
    prefix: String,
    suffix: String,
    has_wildcard: bool,
    replacements: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct AliasMap {
    base_url: String,
    declares_base_url: bool,
    patterns: Vec<Pattern>,
}
#[derive(Default)]
struct Options {
    base: Option<String>,
    paths: Option<Value>,
    paths_dir: Option<String>,
}
pub(crate) struct Aliases {
    root: String,
    maps: BTreeMap<String, AliasMap>,
}
fn join(dir: &str, path: &str) -> String {
    if path.starts_with('/') || dir.is_empty() {
        path.into()
    } else {
        format!("{dir}/{path}")
    }
}
fn normalize(path: &str) -> String {
    let absolute = path.starts_with('/');
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.last().is_some_and(|part| *part != "..") {
                    parts.pop();
                } else if !absolute {
                    parts.push("..");
                }
            }
            other => parts.push(other),
        }
    }
    format!("{}{}", if absolute { "/" } else { "" }, parts.join("/"))
}
fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(dir, _)| dir)
}
fn resolve(dir: &str, path: &str) -> String {
    normalize(&join(dir, path))
}
pub(super) fn relative(root: &str, path: &str) -> Option<String> {
    let root = normalize(root);
    let path = normalize(path);
    if path == root {
        Some(String::new())
    } else {
        path.strip_prefix(&format!("{}/", root.trim_end_matches('/')))
            .map(|path| path.replace('\\', "/"))
    }
}
pub(super) fn parse(source: &str) -> Result<Option<Value>> {
    let mut out = String::new();
    let mut chars = source.chars().peekable();
    let mut string = false;
    while let Some(ch) = chars.next() {
        if string {
            out.push(ch);
            if ch == '\\' {
                if let Some(next) = chars.next() {
                    out.push(next);
                }
            } else if ch == '"' {
                string = false;
            }
        } else if ch == '"' {
            string = true;
            out.push(ch);
        } else if ch == '/' && chars.peek() == Some(&'/') {
            for next in chars.by_ref() {
                if next == '\n' {
                    out.push('\n');
                    break;
                }
            }
        } else if ch == '/' && chars.peek() == Some(&'*') {
            chars.next();
            while let Some(next) = chars.next() {
                if next == '*' && chars.peek() == Some(&'/') {
                    chars.next();
                    break;
                }
            }
        } else {
            out.push(ch);
        }
    }
    static TRAILING: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(&crate::parser::javascript_regex::pattern(r",(\s*[}\]])")).unwrap()
    });
    let stripped = TRAILING.replace_all(&out, "$1");
    Ok(serde_json::from_str::<Value>(&stripped)
        .ok()
        .filter(|v| v.is_object() || v.is_array()))
}
pub(super) fn extends(raw: &Value) -> Result<Vec<&str>> {
    match raw.get("extends") {
        Some(Value::String(value)) => Ok(vec![value]),
        Some(Value::Array(values)) => Ok(values.iter().filter_map(Value::as_str).collect()),
        None | Some(Value::Null) => Ok(Vec::new()),
        _ => anyhow::bail!("configuration extends must be a string or array"),
    }
}
pub(super) fn extends_candidates(root: &str, from: &str, spec: &str) -> Vec<String> {
    let from = resolve(root, parent(from));
    let mut bases = Vec::new();
    if spec.starts_with("./") || spec.starts_with("../") || spec.starts_with('/') {
        bases.push(resolve(&from, spec));
    } else {
        let mut dir = from.as_str();
        loop {
            bases.push(resolve(dir, &format!("node_modules/{spec}")));
            if dir == root || dir.is_empty() || dir == "/" {
                break;
            }
            dir = parent(dir);
        }
    }
    bases
        .into_iter()
        .flat_map(|base| {
            [
                base.clone(),
                format!("{base}.json"),
                format!("{base}/tsconfig.json"),
            ]
        })
        .filter_map(|candidate| relative(root, &candidate))
        .collect()
}
fn effective(
    file: &str,
    root: &str,
    raw: &BTreeMap<String, Option<Value>>,
    stack: &mut HashSet<String>,
    depth: usize,
) -> Result<Option<Options>> {
    if depth > 32 || stack.contains(file) {
        return Ok(None);
    }
    let Some(json) = raw.get(file).and_then(Option::as_ref) else {
        return Ok(None);
    };
    stack.insert(file.into());
    let mut out = Options::default();
    for spec in extends(json)? {
        let Some(target) = extends_candidates(root, file, spec)
            .into_iter()
            .find(|target| raw.contains_key(target))
        else {
            continue;
        };
        if let Some(inherited) = effective(&target, root, raw, stack, depth + 1)? {
            if inherited.base.is_some() {
                out.base = inherited.base;
            }
            if inherited.paths.is_some() {
                out.paths = inherited.paths;
                out.paths_dir = inherited.paths_dir;
            }
        }
    }
    stack.remove(file);
    let dir = resolve(root, parent(file));
    let options = json.get("compilerOptions").unwrap_or(&Value::Null);
    if let Some(base) = options.get("baseUrl").and_then(Value::as_str) {
        out.base = Some(resolve(&dir, base));
    }
    if let Some(paths) = options
        .get("paths")
        .filter(|v| v.is_object() || v.is_array())
    {
        out.paths = Some(paths.clone());
        out.paths_dir = Some(dir);
    }
    Ok(Some(out))
}
impl AliasMap {
    fn load(
        directory: &str,
        root: &str,
        raw: &BTreeMap<String, Option<Value>>,
    ) -> Result<Option<Self>> {
        let mut chosen = None;
        for config in CONFIGS {
            if let Some(options) =
                effective(&join(directory, config), root, raw, &mut HashSet::new(), 0)?
            {
                let has_paths = options.paths.is_some();
                if chosen.is_none() || has_paths {
                    chosen = Some(options);
                }
                if has_paths {
                    break;
                }
            }
        }
        let Some(options) = chosen else {
            return Ok(None);
        };
        let declares_base_url = options.base.is_some();
        let base_url = options
            .base
            .or(options.paths_dir)
            .unwrap_or_else(|| resolve(root, directory));
        let mut patterns = Vec::new();
        if let Some(paths) = options.paths {
            for (pattern, targets) in entries(&paths) {
                let Some(targets) = targets.as_array() else {
                    continue;
                };
                let replacements: Vec<String> = targets
                    .iter()
                    .filter_map(Value::as_str)
                    .map(Into::into)
                    .collect();
                if replacements.is_empty() {
                    continue;
                }
                let (prefix, suffix, has_wildcard) = pattern
                    .split_once('*')
                    .map_or((pattern.as_str(), "", false), |(prefix, suffix)| {
                        (prefix, suffix, true)
                    });
                patterns.push(Pattern {
                    prefix: prefix.into(),
                    suffix: suffix.into(),
                    has_wildcard,
                    replacements,
                });
            }
        }
        if patterns.is_empty() && !declares_base_url {
            return Ok(None);
        }
        patterns.sort_by_key(|pattern| {
            (
                std::cmp::Reverse(pattern.prefix.encode_utf16().count()),
                pattern.has_wildcard,
            )
        });
        Ok(Some(Self {
            base_url,
            declares_base_url,
            patterns,
        }))
    }
    fn apply(&self, module: &str, root: &str) -> Vec<String> {
        for pattern in &self.patterns {
            if !module.starts_with(&pattern.prefix) || !module.ends_with(&pattern.suffix) {
                continue;
            }
            let captured = if pattern.has_wildcard {
                if module.len() < pattern.prefix.len() + pattern.suffix.len() {
                    continue;
                }
                &module[pattern.prefix.len()..module.len() - pattern.suffix.len()]
            } else {
                if module != pattern.prefix {
                    continue;
                }
                ""
            };
            return pattern
                .replacements
                .iter()
                .filter_map(|target| {
                    let filled = if pattern.has_wildcard {
                        target.replacen('*', captured, 1)
                    } else {
                        target.clone()
                    };
                    relative(root, &resolve(&self.base_url, &filled))
                })
                .collect();
        }
        Vec::new()
    }
    fn own_candidates(&self, module: &str, root: &str) -> Vec<String> {
        let mut out = self.apply(module, root);
        if self.declares_base_url
            && !module.starts_with('/')
            && let Some(path) =
                relative(root, &resolve(&self.base_url, module)).filter(|p| !p.is_empty())
        {
            out.push(path);
        }
        out
    }
}
impl Aliases {
    pub(crate) fn new(manifests: &Manifests, root: &str) -> Result<Self> {
        let mut raw = BTreeMap::new();
        for (file, source) in manifests {
            raw.insert(file.clone(), parse(source)?);
        }
        let mut directories = HashSet::from([String::new()]);
        for file in manifests.keys() {
            if matches!(
                file.rsplit('/').next(),
                Some("tsconfig.json" | "jsconfig.json")
            ) {
                directories.insert(parent(file).into());
            }
        }
        let mut maps = BTreeMap::new();
        let mut bytes = 0;
        for directory in directories {
            if let Some(map) = AliasMap::load(&directory, root, &raw)? {
                bytes += serde_json::to_string(&map)?.len();
                ensure!(
                    bytes <= 8 * 1024 * 1024,
                    "configuration alias maps exceed 8 MiB"
                );
                maps.insert(directory, map);
            }
        }
        Ok(Self {
            root: root.into(),
            maps,
        })
    }
    fn nearest(&self, from: &str) -> Option<&AliasMap> {
        let mut dir = parent(from);
        while !dir.is_empty() {
            if let Some(map) = self.maps.get(dir) {
                return Some(map);
            }
            dir = parent(dir);
        }
        None
    }
    pub(super) fn prefix(&self, module: &str) -> bool {
        self.maps.get("").is_some_and(|map| {
            map.patterns
                .iter()
                .any(|p| !p.prefix.is_empty() && module.starts_with(&p.prefix))
        })
    }
    pub(super) fn aliases(&self, from: &str, module: &str) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(near) = self.nearest(from) {
            out.extend(near.apply(module, &self.root));
        }
        if let Some(root) = self.maps.get("") {
            out.extend(root.apply(module, &self.root));
        }
        out
    }
    pub(super) fn own(&self, from: &str, module: &str) -> Vec<String> {
        static SCHEME: std::sync::LazyLock<regex::Regex> =
            std::sync::LazyLock::new(|| regex::Regex::new(r"^[A-Za-z][A-Za-z0-9_+.-]*:").unwrap());
        if module.is_empty() || module.starts_with("//") || SCHEME.is_match(module) {
            return Vec::new();
        }
        self.nearest(from)
            .or_else(|| self.maps.get(""))
            .map_or_else(Vec::new, |map| map.own_candidates(module, &self.root))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::resolution::{PackageScopes, WorkspacePackages, modules::Modules};
    #[test]
    fn configuration_chains_and_module_paths_match_independent_reference() {
        let corpus: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/upstream/aliases/cases.json"
        ))
        .unwrap();
        for case in corpus["cases"].as_array().unwrap() {
            let manifests: Manifests = case["manifests"]
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
            let files: Vec<String> = serde_json::from_value(case["files"].clone()).unwrap();
            let aliases = Aliases::new(&manifests, "/fixture").unwrap();
            assert_eq!(
                serde_json::to_value(&aliases.maps).unwrap(),
                case["maps"],
                "{}",
                case["name"]
            );
            let mut dirs = HashSet::from([String::new()]);
            for file in manifests.keys().chain(files.iter()) {
                let mut dir = parent(file);
                while !dir.is_empty() {
                    dirs.insert(dir.to_string());
                    dir = parent(dir);
                }
            }
            let workspace = WorkspacePackages::from_snapshot(
                &manifests,
                &dirs.into_iter().collect::<Vec<_>>(),
                "/fixture",
            )
            .unwrap();
            let packages = PackageScopes::new(&manifests).unwrap();
            let model = Modules {
                workspace: workspace.as_ref(),
                aliases: &aliases,
                packages: &packages,
                browser: false,
            };
            for query in case["expected"].as_array().unwrap() {
                let actual = model.resolve(
                    query["from"].as_str().unwrap(),
                    query["source"].as_str().unwrap(),
                    query["language"].as_str().unwrap(),
                    |path| files.iter().any(|file| file == path),
                );
                assert_eq!(
                    actual.as_deref(),
                    query["target"].as_str(),
                    "{} {query}",
                    case["name"]
                );
            }
            let temporary = tempfile::tempdir().unwrap();
            let root = temporary.path().canonicalize().unwrap();
            for (path, source) in &manifests {
                std::fs::create_dir_all(root.join(path).parent().unwrap()).unwrap();
                std::fs::write(
                    root.join(path),
                    source.replace("/fixture", root.to_str().unwrap()),
                )
                .unwrap();
            }
            for path in &files {
                std::fs::create_dir_all(root.join(path).parent().unwrap()).unwrap();
                std::fs::write(root.join(path), "").unwrap();
            }
            let capability =
                cap_std::fs::Dir::open_ambient_dir(&root, cap_std::ambient_authority()).unwrap();
            let (captured, _) = crate::graph::workspace_snapshot(
                &capability,
                &root,
                files.iter().map(String::as_str),
            )
            .unwrap();
            let actual = Aliases::new(&captured, root.to_str().unwrap()).unwrap();
            let actual = serde_json::to_string(&actual.maps)
                .unwrap()
                .replace(root.to_str().unwrap(), "/fixture");
            let actual: Value = serde_json::from_str(&actual).unwrap();
            for (directory, map) in actual.as_object().unwrap() {
                assert_eq!(
                    map, &case["maps"][directory],
                    "snapshot {} {directory}",
                    case["name"]
                );
            }
            let captured_aliases = Aliases::new(&captured, root.to_str().unwrap()).unwrap();
            let captured_packages = PackageScopes::new(&captured).unwrap();
            let captured_model = Modules {
                workspace: workspace.as_ref(),
                aliases: &captured_aliases,
                packages: &captured_packages,
                browser: false,
            };
            for query in case["expected"].as_array().unwrap() {
                assert_eq!(
                    captured_model
                        .resolve(
                            query["from"].as_str().unwrap(),
                            query["source"].as_str().unwrap(),
                            query["language"].as_str().unwrap(),
                            |path| files.iter().any(|file| file == path)
                        )
                        .as_deref(),
                    query["target"].as_str(),
                    "snapshot {} {query}",
                    case["name"]
                );
            }
        }
    }
}
