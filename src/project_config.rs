//! Project configuration validation and gitignore-style path selection.
//! SPDX-License-Identifier: MIT. Provenance: crates/prometeu-extract/UPSTREAM.md.
use crate::{parser, source};
use anyhow::{Context, Result, ensure};
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use serde::{
    Deserialize, Serialize,
    de::{IgnoredAny, MapAccess, SeqAccess, Visitor},
};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::io::{ErrorKind, Read};
use std::path::{Component, Path, PathBuf};

pub(crate) const FILENAME: &str = "prometeu-search.json";

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Settings {
    pub extensions: BTreeMap<String, String>,
    pub include_ignored: Vec<String>,
    pub exclude: Vec<String>,
    pub include: Vec<String>,
    pub deprioritize: Vec<String>,
}

pub(crate) struct Config {
    pub settings: Settings,
    pub fingerprint: String,
    pub content_fingerprint: String,
    pub include: Patterns,
    pub include_ignored: Patterns,
    pub exclude: Patterns,
    pub deprioritize: Patterns,
}

impl Config {
    pub fn load(root: &cap_std::fs::Dir) -> Result<Self> {
        let bytes = match source::open(root, Path::new(FILENAME)) {
            Ok(mut file) => {
                ensure!(
                    file.metadata()?.is_file(),
                    "{FILENAME} must be a regular file"
                );
                let mut bytes = Vec::new();
                (&mut file)
                    .take(crate::index::MAX_FILE_BYTES + 1)
                    .read_to_end(&mut bytes)?;
                ensure!(
                    bytes.len() as u64 <= crate::index::MAX_FILE_BYTES,
                    "{FILENAME} exceeds 1 MiB"
                );
                Some(bytes)
            }
            Err(error) if error.kind() == ErrorKind::NotFound => None,
            Err(error) => return Err(error).context(format!("cannot read {FILENAME}")),
        };
        let settings = bytes.as_ref().map_or_else(Settings::default, |bytes| {
            parse(&String::from_utf8_lossy(bytes))
        });
        let fingerprint = blake3::hash(&serde_json::to_vec(&settings)?)
            .to_hex()
            .to_string();
        let content_fingerprint = bytes.as_ref().map_or_else(
            || "absent".to_string(),
            |bytes| blake3::hash(bytes).to_hex().to_string(),
        );
        Ok(Self {
            include: Patterns::new(&settings.include),
            include_ignored: Patterns::new(&settings.include_ignored),
            exclude: Patterns::new(&settings.exclude),
            deprioritize: Patterns::new(&settings.deprioritize),
            settings,
            fingerprint,
            content_fingerprint,
        })
    }

    pub fn language(&self, path: &Path) -> Option<&str> {
        // Detection keys off the final dot, including dotfiles and uppercase extensions.
        let name = path.file_name()?.to_str()?;
        let dot = name.rfind('.')?;
        self.settings
            .extensions
            .get(&name[dot..].to_lowercase())
            .map(String::as_str)
    }
}

fn warn(message: &str) {
    eprintln!("prometeu-search: ignoring {message} in {FILENAME}");
}

fn parse(source: &str) -> Settings {
    let Ok(value) = serde_json::from_str::<Value>(source) else {
        warn("invalid JSON");
        return Settings::default();
    };
    let mut settings = Settings::default();
    if value.get("extensions").is_some_and(Value::is_object) {
        // JSON object insertion order determines the winner when normalized keys collide.
        let extensions = serde_json::from_str::<ExtensionObject>(source).unwrap_or_default();
        for (raw, value) in extensions.0 {
            let mut extension = trim(&raw).to_lowercase();
            if !extension.starts_with('.') {
                extension.insert(0, '.');
            }
            let body = &extension[1..];
            if body.is_empty() || body.contains(['.', '/', '\\']) {
                warn(&format!("invalid extension {raw:?}"));
                continue;
            }
            let Some(language) = value
                .as_str()
                .filter(|language| parser::LANGUAGES.contains(language))
            else {
                warn(&format!("unsupported language for {raw:?}"));
                continue;
            };
            settings.extensions.insert(extension, language.to_string());
        }
    }
    settings.include_ignored = patterns(&value, "includeIgnored");
    settings.include = patterns(&value, "include");
    settings.exclude = patterns(&value, "exclude");
    settings.deprioritize = patterns(&value, "deprioritize");
    settings
}

#[derive(Default)]
struct ExtensionObject(Vec<(String, Value)>);

impl<'de> Deserialize<'de> for ExtensionObject {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        struct Root;
        impl<'de> Visitor<'de> for Root {
            type Value = ExtensionObject;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("configuration object")
            }
            fn visit_map<A: MapAccess<'de>>(
                self,
                mut map: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                let mut extensions = ExtensionObject::default();
                while let Some(key) = map.next_key::<String>()? {
                    if key == "extensions" {
                        extensions = map.next_value::<ExtensionEntries>()?.0;
                    } else {
                        map.next_value::<IgnoredAny>()?;
                    }
                }
                Ok(extensions)
            }
        }
        deserializer.deserialize_map(Root)
    }
}

struct ExtensionEntries(ExtensionObject);
impl<'de> Deserialize<'de> for ExtensionEntries {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        struct Entries;
        impl<'de> Visitor<'de> for Entries {
            type Value = ExtensionEntries;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("extension map")
            }
            fn visit_map<A: MapAccess<'de>>(
                self,
                mut map: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                let mut entries: Vec<(String, Value)> = Vec::new();
                let mut positions: HashMap<String, usize> = HashMap::new();
                while let Some((key, value)) = map.next_entry::<String, Value>()? {
                    if let Some(index) = positions.get(&key).copied() {
                        entries[index].1 = value;
                    } else {
                        positions.insert(key.clone(), entries.len());
                        entries.push((key, value));
                    }
                }
                // ECMAScript enumerates canonical array-index keys before other properties.
                entries.sort_by_key(|(key, _)| {
                    key.parse::<u32>()
                        .ok()
                        .filter(|n| *n < u32::MAX && n.to_string() == *key)
                        .map_or((1, 0), |n| (0, n))
                });
                Ok(ExtensionEntries(ExtensionObject(entries)))
            }
            fn visit_seq<A: SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                while seq.next_element::<IgnoredAny>()?.is_some() {}
                Ok(ExtensionEntries(ExtensionObject::default()))
            }
            fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<Self::Value, E> {
                Ok(ExtensionEntries(ExtensionObject::default()))
            }
            fn visit_bool<E: serde::de::Error>(
                self,
                _: bool,
            ) -> std::result::Result<Self::Value, E> {
                self.visit_unit()
            }
            fn visit_str<E: serde::de::Error>(
                self,
                _: &str,
            ) -> std::result::Result<Self::Value, E> {
                self.visit_unit()
            }
            fn visit_i64<E: serde::de::Error>(self, _: i64) -> std::result::Result<Self::Value, E> {
                self.visit_unit()
            }
            fn visit_u64<E: serde::de::Error>(self, _: u64) -> std::result::Result<Self::Value, E> {
                self.visit_unit()
            }
            fn visit_f64<E: serde::de::Error>(self, _: f64) -> std::result::Result<Self::Value, E> {
                self.visit_unit()
            }
        }
        deserializer.deserialize_any(Entries)
    }
}

fn patterns(value: &Value, key: &str) -> Vec<String> {
    let Some(value) = value.get(key) else {
        return Vec::new();
    };
    let Some(values) = value.as_array() else {
        warn(&format!("non-array {key}"));
        return Vec::new();
    };
    values
        .iter()
        .filter_map(|value| {
            if let Some(pattern) = value
                .as_str()
                .map(trim)
                .filter(|pattern| !pattern.is_empty())
            {
                Some(pattern.to_string())
            } else {
                warn(&format!("invalid {key} pattern"));
                None
            }
        })
        .collect()
}

fn trim(value: &str) -> &str {
    value.trim_matches(|ch: char| matches!(ch, '\u{9}'..='\u{d}' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}'))
}

#[derive(Clone)]
pub(crate) struct Patterns(Gitignore);

impl Patterns {
    pub fn new(patterns: &[String]) -> Self {
        Self::with_case(patterns, true)
    }

    pub fn with_case(patterns: &[String], ignore_case: bool) -> Self {
        let mut builder = GitignoreBuilder::new("");
        // The reference matcher is case-insensitive, independently of the host filesystem.
        builder
            .case_insensitive(ignore_case)
            .expect("case-insensitive matcher");
        for pattern in patterns {
            if let Err(error) = builder.add_line(None, pattern) {
                warn(&format!("invalid path pattern {pattern:?}: {error}"));
            }
        }
        Self(builder.build().unwrap_or_else(|error| {
            warn(&format!("path patterns: {error}"));
            Gitignore::empty()
        }))
    }

    pub fn matches(&self, path: &Path, directory: bool) -> bool {
        self.decision(path, directory) == Some(true)
    }

    pub fn decision(&self, path: &Path, directory: bool) -> Option<bool> {
        // A negated child cannot revive an ignored parent directory. Checking only
        // the leaf (or the nearest matching ancestor) would violate gitignore rules.
        let mut prefix = PathBuf::new();
        let mut parts = path.components().peekable();
        let mut result = None;
        while let Some(part) = parts.next() {
            if !matches!(part, Component::Normal(_)) {
                return None;
            }
            prefix.push(part);
            let matched = self.0.matched(&prefix, parts.peek().is_some() || directory);
            if matched.is_ignore() {
                return Some(true);
            }
            if matched.is_whitelist() {
                result = Some(false);
            }
        }
        result
    }
}

/// Literal leading directories limit force-inclusion walks to opted-in subtrees.
pub(crate) fn static_roots(patterns: &[String]) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for pattern in patterns {
        let trimmed = pattern.trim_start_matches('/');
        let segments: Vec<_> = trimmed
            .trim_end_matches('/')
            .split('/')
            .filter(|part| !part.is_empty())
            .collect();
        let mut leading: Vec<_> = segments
            .iter()
            .copied()
            .take_while(|part| !part.contains(['*', '?', '[', ']', '{', '}', '!']))
            .collect();
        if leading.len() == segments.len() && !trimmed.ends_with('/') {
            leading.pop();
        }
        if leading.is_empty() {
            return vec![PathBuf::new()];
        }
        let root: PathBuf = leading.iter().collect();
        // Patterns are untrusted paths; walking can never escape the pinned worktree.
        if root
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
        {
            roots.push(root);
        }
    }
    roots.sort();
    roots.dedup();
    let mut result: Vec<PathBuf> = Vec::new();
    for root in roots {
        if !result.iter().any(|other| root.starts_with(other)) {
            result.push(root);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalized_configuration_and_matching_equal_reference() {
        let fixture: Value =
            serde_json::from_str(include_str!("../tests/fixtures/upstream/config/cases.json"))
                .unwrap();
        for case in fixture["cases"].as_array().unwrap() {
            let settings = parse(case["source"].as_str().unwrap());
            assert_eq!(
                serde_json::to_value(&settings).unwrap(),
                case["expected"],
                "{}",
                case["id"]
            );
            for sample in case["matches"].as_array().unwrap() {
                let matcher = Patterns::new(match sample["field"].as_str().unwrap() {
                    "include" => &settings.include,
                    "exclude" => &settings.exclude,
                    "includeIgnored" => &settings.include_ignored,
                    "deprioritize" => &settings.deprioritize,
                    _ => unreachable!(),
                });
                assert_eq!(
                    matcher.matches(
                        Path::new(sample["path"].as_str().unwrap()),
                        sample["directory"].as_bool().unwrap()
                    ),
                    sample["expected"].as_bool().unwrap(),
                    "{}: {}",
                    case["id"],
                    sample
                );
            }
        }
    }
}
