//! Cargo workspace members and Rust module targets from committed snapshots.
use super::workspace::{self, Manifests};
use anyhow::{Context, Result, ensure};
use prometeu_extract::{Node, UnresolvedReference};
use std::collections::{BTreeMap, HashMap, HashSet};
#[path = "cargo_workspace/compiler.rs"]
mod compiler;
fn trim(value: &str) -> &str {
    value.trim_matches(crate::search::query_whitespace)
}
fn section(source: &str, wanted: &str) -> Option<String> {
    let mut active = false;
    let mut lines = Vec::new();
    for line in source.split('\n') {
        let text = trim(line);
        if !active {
            if text == format!("[{wanted}]") {
                active = true
            }
            continue;
        }
        if text.len() > 2
            && text.starts_with('[')
            && text.ends_with(']')
            && !text[1..text.len() - 1].contains(']')
        {
            break;
        }
        lines.push(line)
    }
    active.then(|| lines.join("\n"))
}
pub(crate) fn package_name(source: &str) -> Result<Option<String>> {
    let Some(section) = section(source, "package") else {
        return Ok(None);
    };
    Ok(
        crate::frameworks::source_pattern(r#"name\s*=\s*["']([^"'\n]+)["']"#)?
            .captures(&section)?
            .map(|m| trim(&m[1]).to_owned()),
    )
}
fn members(source: &str) -> Result<Vec<String>> {
    let Some(section) = section(source, "workspace") else {
        return Ok(Vec::new());
    };
    let Some(key) = crate::frameworks::source_pattern(r"\bmembers\b\s*=")?.find(&section)? else {
        return Ok(Vec::new());
    };
    let after = section[key.end()..].trim_start_matches(crate::search::query_whitespace);
    if !after.starts_with('[') {
        return Ok(Vec::new());
    }
    let mut quote = None;
    let mut escaped = false;
    let mut depth = 1usize;
    let mut end = None;
    for (at, ch) in after[1..].char_indices() {
        if let Some(q) = quote {
            if escaped {
                escaped = false
            } else if ch == '\\' {
                escaped = true
            } else if ch == q {
                quote = None
            }
            continue;
        }
        match ch {
            '\'' | '"' => quote = Some(ch),
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    end = Some(at + 1);
                    break;
                }
            }
            _ => {}
        }
    }
    let Some(end) = end else {
        return Ok(Vec::new());
    };
    let list = &after[1..end];
    let mut values = Vec::new();
    let mut current = String::new();
    quote = None;
    escaped = false;
    for ch in list.chars() {
        if let Some(q) = quote {
            if escaped {
                current.push(ch);
                escaped = false
            } else if ch == '\\' {
                escaped = true
            } else if ch == q {
                let value = trim(&current);
                if !value.is_empty() {
                    values.push(value.to_owned())
                }
                quote = None;
                current.clear()
            } else {
                current.push(ch)
            }
        } else if matches!(ch, '\'' | '"') {
            quote = Some(ch);
            current.clear()
        }
    }
    Ok(values)
}
fn units(value: &str) -> String {
    value
        .encode_utf16()
        .map(|u| {
            char::from_u32(if (0xd800..=0xdfff).contains(&u) {
                0xf0000 + u as u32
            } else {
                u as u32
            })
            .unwrap()
        })
        .collect()
}
struct Pattern {
    original: String,
    regex: fancy_regex::Regex,
}
impl Pattern {
    fn new(pattern: &str) -> Result<Self> {
        ensure!(
            pattern.encode_utf16().count() <= 4096,
            "Cargo workspace pattern exceeds 4096 UTF-16 units"
        );
        let mut opens = Vec::new();
        let mut escaped = false;
        let mut bracket = false;
        let mut nesting = 0usize;
        for (at, ch) in pattern.char_indices() {
            if escaped {
                escaped = false;
                continue;
            }
            if ch == '\\' {
                escaped = true;
                continue;
            }
            if ch == '[' {
                bracket = true
            } else if ch == ']' {
                bracket = false
            } else if !bracket {
                if matches!(ch, '{' | '(') {
                    nesting += 1;
                    ensure!(nesting <= 64, "Cargo workspace pattern nesting exceeds 64");
                } else if matches!(ch, '}' | ')') {
                    nesting = nesting.saturating_sub(1)
                }
                if ch == '{' {
                    opens.push(at)
                } else if ch == '}' {
                    opens.pop();
                }
            }
        }
        if !opens.is_empty() {
            // The pinned implementation falls back to an impossible regexp for
            // unfinished braces; its exact-input fast path still applies.
            return Ok(Self {
                original: pattern.to_owned(),
                regex: crate::frameworks::source_pattern("(?!)")?,
            });
        }
        let descriptor =
            compiler::make_re(&units(pattern), &compiler::CompileOptions::default(), false)
                .context("unsupported Cargo workspace pattern")?;
        ensure!(
            descriptor.source.len() <= 1024 * 1024,
            "Cargo workspace pattern expansion exceeds 1 MiB"
        );
        let regex = crate::frameworks::source_pattern(&descriptor.source)?;
        Ok(Self {
            original: pattern.to_owned(),
            regex,
        })
    }
    fn matches(&self, path: &str) -> Result<bool> {
        if path.is_empty() {
            return Ok(false);
        }
        if path == self.original {
            return Ok(true);
        }
        Ok(self.regex.is_match(&units(path))?)
    }
}
fn confined(path: &str) -> bool {
    let path = path.replace('\\', "/");
    if path.starts_with('/') {
        return false;
    }
    let mut depth = 0;
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if depth == 0 {
                    return false;
                }
                depth -= 1
            }
            _ => depth += 1,
        }
    }
    true
}
pub(crate) fn expand_members(
    source: &str,
    mut list: impl FnMut(&str) -> Result<Vec<String>>,
) -> Result<Vec<String>> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut work = 0usize;
    for member in members(source)? {
        if !confined(&member) {
            continue;
        }
        let first = member.find(['*', '?', '[', ']', '{', '}', '!']);
        let mut candidates = Vec::new();
        if let Some(first) = first {
            let prefix = &member[..first];
            let prefix = prefix.rfind('/').map_or("", |cut| &prefix[..cut]);
            let matcher = Pattern::new(&member)?;
            let mut stack = vec![(
                if prefix.is_empty() {
                    ".".to_owned()
                } else {
                    prefix.to_owned()
                },
                0,
                false,
            )];
            while let Some((dir, depth, check)) = stack.pop() {
                if check && matcher.matches(&dir)? {
                    candidates.push(dir.clone())
                }
                if depth > 5 {
                    continue;
                }
                work += 1;
                ensure!(
                    work <= 50_000,
                    "Cargo workspace glob traversal limit exceeded"
                );
                let children = list(&dir)?;
                let mut descendants = Vec::new();
                for child in children {
                    if child.starts_with('.')
                        || matches!(
                            child.as_str(),
                            "target" | "node_modules" | ".git" | "dist" | "build"
                        )
                    {
                        continue;
                    }
                    let relative = if dir == "." {
                        child
                    } else {
                        format!("{dir}/{child}")
                    };
                    descendants.push((relative, depth + 1, true));
                }
                stack.extend(descendants.into_iter().rev());
            }
        } else {
            candidates.push(member)
        }
        for candidate in candidates {
            let cleaned = candidate.replace('\\', "/");
            let cleaned = cleaned.strip_suffix('/').unwrap_or(&cleaned).to_owned();
            if seen.insert(cleaned.clone()) {
                out.push(cleaned)
            }
        }
    }
    Ok(out)
}
pub(crate) fn crate_map(
    manifests: &Manifests,
    directories: &[String],
) -> Result<BTreeMap<String, String>> {
    let Some(root) = manifests.get("Cargo.toml") else {
        return Ok(BTreeMap::new());
    };
    let children = workspace::directory_children(directories);
    let members = expand_members(root, |dir| {
        Ok(children
            .get(
                crate::auxiliary_sources::normalize("", dir)
                    .as_deref()
                    .unwrap_or(""),
            )
            .cloned()
            .unwrap_or_default())
    })?;
    let mut out = BTreeMap::new();
    for member in members {
        let Some(path) = crate::auxiliary_sources::normalize("", &format!("{member}/Cargo.toml"))
        else {
            continue;
        };
        let Some(source) = manifests.get(&path) else {
            continue;
        };
        if let Some(name) = package_name(source)?.filter(|name| !name.is_empty()) {
            out.insert(name.clone(), member.clone());
            out.insert(name.replace('-', "_"), member);
        }
    }
    Ok(out)
}
pub(crate) struct Modules<'a> {
    nodes: HashMap<&'a str, Vec<&'a Node>>,
    crates: BTreeMap<String, String>,
}
impl<'a> Modules<'a> {
    pub(crate) fn new(
        nodes: &'a [Node],
        manifests: &Manifests,
        directories: &[String],
    ) -> Result<Self> {
        let mut files: HashMap<&str, Vec<&Node>> = HashMap::new();
        for node in nodes {
            files.entry(&node.file_path).or_default().push(node);
        }
        Ok(Self {
            nodes: files,
            crates: crate_map(manifests, directories)?,
        })
    }
    pub(crate) fn resolve(&self, reference: &UnresolvedReference) -> Option<(&'a Node, f64)> {
        let name = &reference.reference_name;
        if name.is_empty() || !name.bytes().all(|b| b.is_ascii_lowercase() || b == b'_') {
            return None;
        }
        let mut paths = vec![
            (format!("src/{name}.rs"), false),
            (format!("src/{name}/mod.rs"), false),
        ];
        if let Some(member) = self.crates.get(name) {
            paths.extend([
                (format!("{member}/src/lib.rs"), true),
                (format!("{member}/src/main.rs"), true),
            ]);
        }
        for (path, workspace) in paths {
            if let Some(nodes) = self.nodes.get(path.as_str())
                && let Some(node) = nodes
                    .iter()
                    .find(|node| node.kind == "module")
                    .or_else(|| nodes.first())
            {
                return Some((node, if workspace { 0.95 } else { 0.6 }));
            }
        }
        None
    }
}

pub(crate) fn load_directories(connection: &rusqlite::Connection) -> Result<Vec<String>> {
    use rusqlite::OptionalExtension;
    let raw: Option<String> = connection
        .query_row(
            "SELECT value FROM metadata WHERE key='project_directories'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    let Some(raw) = raw else {
        return Ok(Vec::new());
    };
    ensure!(
        raw.len() <= 16 * 1024 * 1024,
        "Cargo directory snapshot exceeds limit"
    );
    let paths: Vec<String> = serde_json::from_str(&raw)?;
    ensure!(
        paths.iter().map(String::len).sum::<usize>() <= 8 * 1024 * 1024,
        "Cargo directory contents exceed limit"
    );
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    fn corpus() -> Value {
        serde_json::from_str(include_str!(
            "../../tests/fixtures/upstream/frameworks/cargo-workspace.json"
        ))
        .unwrap()
    }
    #[test]
    fn pinned_workspace_globs_and_manifests() {
        let corpus = corpus();
        let mut differences = Vec::new();
        for case in corpus["globs"].as_array().unwrap() {
            let pattern = case["pattern"].as_str().unwrap();
            let matcher = Pattern::new(pattern);
            for answer in case["answers"].as_array().unwrap() {
                let actual = matcher
                    .as_ref()
                    .ok()
                    .and_then(|matcher| matcher.matches(answer["path"].as_str().unwrap()).ok());
                if actual != answer["expected"].as_bool() {
                    differences.push(json!({"pattern":pattern,"answer":answer,"actual":actual,"error":matcher.as_ref().err().map(ToString::to_string)}));
                }
            }
        }
        for case in corpus["packages"].as_array().unwrap() {
            assert_eq!(
                serde_json::to_value(package_name(case["source"].as_str().unwrap()).unwrap())
                    .unwrap(),
                case["expected"]
            );
        }
        for case in corpus["maps"].as_array().unwrap() {
            let manifests = serde_json::from_value(case["manifests"].clone()).unwrap();
            let directories =
                serde_json::from_value::<Vec<String>>(case["directories"].clone()).unwrap();
            assert_eq!(
                serde_json::to_value(crate_map(&manifests, &directories).unwrap()).unwrap(),
                case["expected"],
                "{}",
                case["id"]
            );
        }
        if !differences.is_empty() {
            std::fs::write(
                "/tmp/prometeu-cargo-glob-differences.json",
                serde_json::to_string_pretty(&differences).unwrap(),
            )
            .unwrap();
        }
        assert!(
            differences.is_empty(),
            "{} glob mismatches; see /tmp/prometeu-cargo-glob-differences.json",
            differences.len()
        );
    }
    #[test]
    fn pinned_rust_module_lookup() {
        let corpus = corpus();
        for case in corpus["projects"].as_array().unwrap() {
            let mut raw = case["nodes"].clone();
            for node in raw.as_array_mut().unwrap() {
                node["updatedAt"] = json!(0);
            }
            let nodes = serde_json::from_value::<Vec<Node>>(raw).unwrap();
            let manifests = case["files"]
                .as_object()
                .unwrap()
                .iter()
                .filter(|(file, _)| file.ends_with("Cargo.toml"))
                .map(|(file, source)| (file.clone(), source.as_str().unwrap().to_owned()))
                .collect();
            let mut directories = std::collections::BTreeSet::new();
            for file in case["files"].as_object().unwrap().keys() {
                let mut parent = std::path::Path::new(file).parent();
                while let Some(path) = parent {
                    if !path.as_os_str().is_empty() {
                        directories.insert(path.to_str().unwrap().to_owned());
                    }
                    parent = path.parent();
                }
            }
            let modules = Modules::new(
                &nodes,
                &manifests,
                &directories.into_iter().collect::<Vec<_>>(),
            )
            .unwrap();
            for answer in case["answers"].as_array().unwrap() {
                let reference = serde_json::from_value(answer["reference"].clone()).unwrap();
                let actual = modules
                    .resolve(&reference)
                    .map(|(node, confidence)| json!({"target":node.id,"confidence":confidence}));
                let expected = answer["expected"]
                    .as_object()
                    .map(|e| json!({"target":e["targetNodeId"],"confidence":e["confidence"]}));
                assert_eq!(actual, expected, "{} {:?}", case["id"], reference);
            }
        }
    }
    #[test]
    fn glob_limits_fail_explicitly() {
        assert!(Pattern::new(&"*".repeat(4097)).is_err());
        assert!(Pattern::new(&format!("{}a{}", "{".repeat(65), "}".repeat(65))).is_err());
    }
}

#[cfg(test)]
#[path = "cargo_workspace/persisted.rs"]
mod persisted;
