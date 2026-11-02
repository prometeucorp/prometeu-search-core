//! Callable values use lexical scope and unique targets instead of fuzzy names.
use super::*;
use prometeu_extract::UnresolvedReference;

pub(super) fn handled(language: &str) -> bool {
    !matches!(
        language,
        "typescript" | "tsx" | "javascript" | "jsx" | "python"
    )
}

impl Graph {
    pub(super) fn this_callable_value(
        &self,
        from: usize,
        reference: &UnresolvedReference,
    ) -> (Option<&Node>, f64) {
        let Some(caller) = self.metadata(from) else {
            return (None, 0.0);
        };
        let Some(member) = reference.reference_name.strip_prefix("this.") else {
            return (None, 0.0);
        };
        let owner = if matches!(
            caller.kind.as_str(),
            "class" | "struct" | "interface" | "trait" | "protocol" | "module"
        ) {
            caller.qualified_name.as_str()
        } else {
            let Some((owner, _)) = caller.qualified_name.rsplit_once("::") else {
                return (None, 0.0);
            };
            owner
        };
        let qualified = format!("{owner}::{member}");
        let own = self.files[&caller.file_path]
            .nodes
            .iter()
            .filter_map(|&index| self.metadata(index))
            .filter(|node| {
                node.qualified_name == qualified
                    && node.id != caller.id
                    && matches!(node.kind.as_str(), "function" | "method")
            })
            .min_by_key(|node| node.start_line);
        if own.is_some() {
            return (own, 0.95);
        }
        let owners = self.files[&caller.file_path]
            .qualified
            .get(owner)
            .into_iter()
            .flatten();
        for &index in owners {
            for target in self.members(index, member, &mut HashSet::new()) {
                if let Some(target) = self.metadata(target)
                    && matches!(target.kind.as_str(), "function" | "method")
                    && code_family(&target.language) == code_family(&caller.language)
                {
                    return (Some(target), 0.85);
                }
            }
        }
        (None, 0.0)
    }
}

pub(super) fn select<'a>(
    nodes: &[&'a Node],
    caller: Option<&Node>,
    sources: &BTreeMap<&str, &str>,
    scopes: &crate::frameworks::python_resolution::Bindings<'a>,
    reference: &UnresolvedReference,
) -> Result<(Option<&'a Node>, f64)> {
    let language = reference.language.as_deref().unwrap_or("");
    let file = reference.file_path.as_deref().unwrap_or("");
    let name = reference.reference_name.as_str();
    if name.starts_with("this.") || name.contains('.') {
        return Ok((None, 0.0));
    }
    let qualified = name.contains("::");
    let bare_functions = matches!(language, "cpp" | "php" | "arkts");
    let member = name.rsplit("::").next().unwrap_or(name);
    let named = nodes
        .iter()
        .copied()
        .filter(|node| {
            node.name == member
                && node.id != reference.from_node_id
                && code_family(&node.language) == code_family(language)
                && (node.kind == "function"
                    || (node.kind == "method" && (qualified || !bare_functions)))
                && (!qualified
                    || node.qualified_name == name
                    || node.qualified_name.ends_with(&format!("::{name}")))
        })
        .collect::<Vec<_>>();
    let mut candidates = Vec::new();
    for &node in &named {
        if !qualified && !scopes.lexically_reachable(node, reference)? {
            continue;
        }
        if !qualified
            && language == "dart"
            && node.kind == "method"
            && let Some((owner, _)) = node.qualified_name.rsplit_once("::")
            && let Some(owner) = owner.rsplit("::").next()
        {
            let head = sources
                .get(node.file_path.as_str())
                .and_then(|source| {
                    source
                        .lines()
                        .nth(node.start_line.saturating_sub(1) as usize)
                })
                .unwrap_or("");
            let head = String::from_utf16_lossy(
                &head
                    .encode_utf16()
                    .skip(node.start_column as usize)
                    .collect::<Vec<_>>(),
            );
            if crate::frameworks::source_pattern(&format!(
                r"^(?:(?:const|factory|external)\s+)*{}\s*\.\s*{}(?![\w$])",
                ::regex::escape(owner),
                ::regex::escape(&node.name)
            ))?
            .is_match(&head)?
            {
                continue;
            }
        }
        if !qualified && language == "swift" && node.kind == "method" {
            let owner = caller.and_then(|node| {
                node.qualified_name
                    .rsplit_once("::")
                    .map(|(owner, _)| owner)
            });
            let scope = node
                .qualified_name
                .rsplit_once("::")
                .map(|(owner, _)| owner);
            if !owner.zip(scope).is_some_and(|(owner, scope)| {
                owner == scope
                    || owner.ends_with(&format!("::{scope}"))
                    || scope.ends_with(&format!("::{owner}"))
            }) {
                continue;
            }
        }
        candidates.push(node);
    }
    let same_file = candidates
        .iter()
        .copied()
        .filter(|node| node.file_path == file)
        .collect::<Vec<_>>();
    if !same_file.is_empty() {
        if !qualified
            && language == "swift"
            && same_file.len() > 1
            && same_file.iter().all(|node| node.kind == "method")
        {
            return Ok((None, 0.0));
        }
        return Ok((
            same_file.iter().copied().min_by_key(|node| node.start_line),
            if qualified || same_file.len() > 1 {
                0.9
            } else {
                0.95
            },
        ));
    }
    Ok((
        if candidates.len() == 1 && (qualified || named.len() == 1) {
            Some(candidates[0])
        } else {
            None
        },
        if qualified { 0.9 } else { 0.8 },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Engine;
    use serde_json::Value;
    #[test]
    fn callable_values_match_reference_projects_and_reopened_snapshots() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/upstream/resolution/function-values/cases.json"
        ))
        .unwrap();
        let normalize = |edges: Vec<prometeu_extract::Edge>| {
            let mut values = edges
                .into_iter()
                .filter(|edge| {
                    edge.metadata
                        .as_ref()
                        .is_some_and(|value| value["fnRef"] == true)
                })
                .map(|edge| {
                    let mut value = serde_json::to_value(edge).unwrap();
                    value.sort_all_objects();
                    value.to_string()
                })
                .collect::<Vec<_>>();
            values.sort();
            values
        };
        let mut differences = Vec::new();
        for project in fixture["projects"].as_array().unwrap() {
            let temporary = tempfile::tempdir().unwrap();
            let root = temporary.path().join("project");
            let database = temporary.path().join("index.sqlite");
            std::fs::create_dir_all(&root).unwrap();
            for (path, source) in project["files"].as_object().unwrap() {
                let path = root.join(path);
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(path, source.as_str().unwrap()).unwrap();
            }
            let expected = normalize(serde_json::from_value(project["edges"].clone()).unwrap());
            let mut engine = Engine::open(&root, Some(&database)).unwrap();
            engine.sync().unwrap();
            for reopened in [false, true] {
                if reopened {
                    drop(engine);
                    for path in project["files"].as_object().unwrap().keys() {
                        std::fs::write(root.join(path), "changed live source").unwrap();
                    }
                    engine = Engine::open(&root, Some(&database)).unwrap();
                }
                let graph = Graph::load(&engine.connection, 1).unwrap();
                let actual = normalize(graph.canonical_edges.into_iter().flatten().collect());
                if actual != expected {
                    differences.push(serde_json::json!({"project":project["name"],"reopened":reopened,"missing":expected.iter().filter(|edge| !actual.contains(edge)).collect::<Vec<_>>(),"extra":actual.iter().filter(|edge| !expected.contains(edge)).collect::<Vec<_>>() }));
                }
            }
        }
        if !differences.is_empty() {
            std::fs::write(
                std::env::temp_dir().join("prometeu-function-value-diffs.json"),
                serde_json::to_vec_pretty(&differences).unwrap(),
            )
            .unwrap();
        }
        assert!(
            differences.is_empty(),
            "{} callback graph mismatches; see temp/prometeu-function-value-diffs.json",
            differences.len()
        );
    }
}
