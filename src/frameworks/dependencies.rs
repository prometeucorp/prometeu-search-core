//! The bounded manifest sample used by JavaScript framework detection.
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashSet};

pub(super) fn declared(manifests: &BTreeMap<String, String>, files: &[String]) -> HashSet<String> {
    declared_borrowed(
        &manifests
            .iter()
            .map(|(path, source)| (path.as_str(), source.as_str()))
            .collect(),
        &files.iter().map(String::as_str).collect::<Vec<_>>(),
    )
}

pub(crate) fn declared_borrowed(
    manifests: &BTreeMap<&str, &str>,
    files: &[&str],
) -> HashSet<String> {
    let mut directories = Vec::new();
    let mut seen = HashSet::new();
    for file in files {
        let parts: Vec<_> = file.split('/').collect();
        for depth in 1..=2 {
            if parts.len() > depth {
                let dir = format!("{}/", parts[..depth].join("/"));
                if seen.insert(dir.clone()) {
                    directories.push(dir);
                }
            }
        }
        if directories.len() > 192 {
            break;
        }
    }
    let mut selected = vec!["package.json".into()];
    for dir in directories {
        if selected.len() > 24 {
            break;
        }
        let path = format!("{dir}package.json");
        if !dir.contains("node_modules") && manifests.contains_key(path.as_str()) {
            selected.push(path);
        }
    }
    if selected.len() <= 24 {
        let script = regex::Regex::new(r"\.(?:[cm]?[jt]sx?|vue|svelte|astro)$").unwrap();
        let mut deep = BTreeMap::<usize, BTreeSet<String>>::new();
        for file in files
            .iter()
            .filter(|file| script.is_match(file) && !file.contains("node_modules"))
        {
            for (depth, (slash, _)) in file.match_indices('/').enumerate() {
                if depth + 1 >= 3 {
                    deep.entry(depth + 1)
                        .or_default()
                        .insert(file[..slash + 1].into());
                }
            }
        }
        let mut ordered = Vec::new();
        for directories in deep.values() {
            let mut directories: Vec<_> = directories.iter().collect();
            directories.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
            ordered.extend(directories);
        }
        for dir in ordered.into_iter().take(192) {
            if selected.len() > 24 {
                break;
            }
            let path = format!("{dir}package.json");
            if manifests.contains_key(path.as_str()) {
                selected.push(path);
            }
        }
    }
    let mut names = HashSet::new();
    for path in selected {
        if let Some(source) = manifests.get(path.as_str())
            && let Ok(package) = serde_json::from_str::<Value>(source)
        {
            for field in ["dependencies", "devDependencies", "peerDependencies"] {
                match &package[field] {
                    Value::Object(group) => names.extend(group.keys().cloned()),
                    Value::Array(group) => {
                        names.extend((0..group.len()).map(|index| index.to_string()))
                    }
                    _ => {}
                }
            }
        }
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn manifest_sampling_matches_independent_reference() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/upstream/frameworks/cases.json"
        ))
        .unwrap();
        for case in fixture["dependencies"].as_array().unwrap() {
            let manifests = serde_json::from_value(case["manifests"].clone()).unwrap();
            let files: Vec<String> = serde_json::from_value(case["files"].clone()).unwrap();
            let mut actual: Vec<_> = declared(&manifests, &files).into_iter().collect();
            actual.sort();
            let mut expected: Vec<String> =
                serde_json::from_value(case["expected"].clone()).unwrap();
            expected.sort();
            assert_eq!(actual, expected, "{}", case["name"]);
        }
    }
}
