//! Pascal parenthesis-free routine reads distinguish calls from method values.
use super::*;
use prometeu_extract::UnresolvedReference;

pub(super) struct Answer<'a> {
    pub node: &'a Node,
    pub confidence: f64,
    pub resolved_by: &'static str,
    pub kind: &'static str,
}

impl Graph {
    pub(super) fn pascal_value(
        &self,
        reference: &UnresolvedReference,
    ) -> Result<Option<Answer<'_>>> {
        let Some(parts) = crate::frameworks::source_pattern(
            r"^\{(call|read|ref|recv|args)(?:=([^@}]+))?(?:@([^}]+))?\}\.(.+)$",
        )?
        .captures(&reference.reference_name)?
        else {
            return Ok(None);
        };
        let mut mode = parts[1].to_string();
        let path = reference.file_path.as_deref().unwrap_or("");
        let caller = self.files[path]
            .nodes
            .iter()
            .filter_map(|&index| self.metadata(index))
            .find(|node| node.id == reference.from_node_id);
        let owner = parts.get(3).map(|part| part.as_str()).or_else(|| {
            caller.and_then(|node| {
                node.qualified_name
                    .rsplit_once("::")
                    .map(|(owner, _)| owner)
            })
        });
        let mut found = None;
        let names: Vec<_> = parts[4].split('.').collect();
        if names.len() == 1 {
            found = self.pascal_scoped_value(path, owner, names[0]);
        } else {
            let mut receiver = if let Some(typed) = names[0].strip_prefix(':') {
                self.lookup(path, owner.unwrap_or(""), typed)
                    .0
                    .first()
                    .copied()
            } else if names[0].eq_ignore_ascii_case("self") {
                owner
                    .and_then(|owner| self.files[path].qualified.get(&owner.to_lowercase()))
                    .and_then(|owners| owners.first())
                    .copied()
            } else {
                self.pascal_scoped_value(path, owner, names[0])
                    .map(|(index, _, _)| index)
            };
            for (position, name) in names.iter().enumerate().skip(1) {
                let Some(index) = receiver else {
                    break;
                };
                let Some(node) = self.metadata(index) else {
                    break;
                };
                let type_index = if matches!(node.kind.as_str(), "class" | "struct" | "interface") {
                    Some(index)
                } else {
                    self.pascal_declared_type(node)?.and_then(|ty| {
                        self.lookup(&node.file_path, &node.qualified_name, &ty)
                            .0
                            .first()
                            .copied()
                    })
                };
                receiver = type_index.and_then(|owner| self.pascal_member(owner, name));
                if position + 1 == names.len() {
                    found = receiver.map(|target| (target, "instance-method", 0.9));
                }
            }
        }
        let Some((index, resolved_by, confidence)) = found else {
            return Ok(None);
        };
        let Some(node) = self.metadata(index) else {
            return Ok(None);
        };
        if !matches!(node.kind.as_str(), "function" | "method") {
            return Ok(None);
        }
        let keyword = self.files[&node.file_path]
            .source
            .lines()
            .nth(node.start_line.saturating_sub(1) as usize)
            .unwrap_or("")
            .to_lowercase();
        let yields_value = node.return_type.is_some()
            || crate::frameworks::source_pattern(r"\b(?:function|constructor)\s")?
                .is_match(&keyword)?;
        let signature = node.signature.as_deref().unwrap_or("").trim();
        let needs_arguments = signature.strip_prefix('(').is_some_and(|parameters| {
            let mut depth = 1;
            let end = parameters
                .char_indices()
                .find_map(|(index, ch)| {
                    if ch == '(' {
                        depth += 1;
                    } else if ch == ')' {
                        depth -= 1;
                    }
                    (depth == 0).then_some(index)
                })
                .unwrap_or(parameters.len());
            parameters[..end]
                .split(';')
                .any(|parameter| !parameter.trim().is_empty() && !parameter.contains('='))
        });
        if let Some(target) = parts.get(2) {
            let target = target.as_str();
            let declared = if let Some(ty) = target.strip_prefix(':') {
                Some(ty.to_owned())
            } else {
                if let Some(node) = self
                    .pascal_scoped_value(path, owner, target)
                    .and_then(|(index, _, _)| self.metadata(index))
                {
                    self.pascal_declared_type(node)?
                } else {
                    None
                }
            };
            if let Some(declared) = declared {
                let declared = declared
                    .rsplit_once(':')
                    .map_or(declared.as_str(), |(_, ty)| ty)
                    .trim();
                let routine_type =
                    self.pascal_routine_type(path, owner.unwrap_or(""), declared, 0)?;
                mode = if routine_type { "ref" } else { "read" }.into();
            }
        }
        if mode == "ref"
            && let Some(ty) = node.return_type.as_deref()
            && self.pascal_routine_type(&node.file_path, &node.qualified_name, ty, 0)?
        {
            mode = "read".into();
        }
        let pointer = mode != "args"
            && (mode == "ref" || needs_arguments || (mode != "call" && !yields_value));
        if pointer && matches!(mode.as_str(), "call" | "recv")
            || needs_arguments && keyword.contains("constructor ") && mode != "args"
        {
            return Ok(None);
        }
        Ok(Some(Answer {
            node,
            confidence,
            resolved_by,
            kind: if pointer { "references" } else { "calls" },
        }))
    }

    fn pascal_routine_type(&self, path: &str, scope: &str, ty: &str, depth: usize) -> Result<bool> {
        if depth >= 8 {
            return Ok(false);
        }
        if crate::frameworks::source_pattern(r"(?i)^(?:reference\s+to\s+)?(?:procedure|function)\b|^(?:TProc|TFunc|TPredicate|TComparison|TThreadProcedure|TListSortCompare|TStringListSortCompare)(?:<|$)")?.is_match(ty)? { return Ok(true); }
        for index in self.lookup(path, scope, ty).0 {
            let Some(node) = self
                .metadata(index)
                .filter(|node| node.kind == "type_alias")
            else {
                continue;
            };
            let source = self.files[&node.file_path]
                .source
                .lines()
                .skip(node.start_line.saturating_sub(1) as usize)
                .take(node.end_line.saturating_sub(node.start_line) as usize + 1)
                .collect::<Vec<_>>()
                .join("\n");
            if let Some((_, declared)) = source.split_once('=')
                && self.pascal_routine_type(
                    &node.file_path,
                    &node.qualified_name,
                    declared.trim().trim_end_matches(';'),
                    depth + 1,
                )?
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn pascal_declared_type(&self, node: &Node) -> Result<Option<String>> {
        if let Some(ty) = &node.return_type {
            return Ok(Some(ty.clone()));
        }
        if let Some(ty) = node
            .signature
            .as_deref()
            .and_then(|signature| signature.rsplit_once(':').map(|(_, ty)| ty.trim()))
        {
            return Ok(Some(ty.into()));
        }
        let source = self.files[&node.file_path]
            .source
            .lines()
            .skip(node.start_line.saturating_sub(1) as usize)
            .take(node.end_line.saturating_sub(node.start_line) as usize + 1)
            .collect::<Vec<_>>()
            .join("\n");
        Ok(crate::frameworks::source_pattern(&format!(
            r"(?i)\b{}\b(?:\s*,\s*\w+)*\s*:\s*([\w.]+(?:<[^<>]*>)?)",
            ::regex::escape(&node.name)
        ))?
        .captures(&source)?
        .map(|parts| parts[1].to_string()))
    }

    fn pascal_member(&self, owner: usize, name: &str) -> Option<usize> {
        let mut queue = std::collections::VecDeque::from([(owner, 0)]);
        let mut seen = HashSet::new();
        while let Some((owner, depth)) = queue.pop_front() {
            if depth >= 8 || !seen.insert(owner) {
                continue;
            }
            let node = self.metadata(owner)?;
            let qualified = format!("{}::{name}", node.qualified_name).to_lowercase();
            if let Some(&member) = self.files[&node.file_path]
                .qualified
                .get(&qualified)
                .and_then(|members| members.first())
            {
                return Some(member);
            }
            for &index in &self.outgoing[owner] {
                let edge = &self.edges[index];
                if matches!(edge.kind.as_str(), "extends" | "implements")
                    && let Some(target) = edge.to
                {
                    queue.push_back((target, depth + 1));
                }
            }
        }
        None
    }

    fn pascal_scoped_value(
        &self,
        path: &str,
        owner: Option<&str>,
        name: &str,
    ) -> Option<(usize, &'static str, f64)> {
        if let Some(owner) = owner {
            for &index in &self.files[path].nodes {
                if self.metadata(index).is_some_and(|node| {
                    matches!(node.kind.as_str(), "class" | "struct" | "interface")
                        && node
                            .qualified_name
                            .split('<')
                            .next()
                            .unwrap_or("")
                            .eq_ignore_ascii_case(owner)
                }) && let Some(target) = self.pascal_member(index, name)
                {
                    return Some((target, "instance-method", 0.9));
                }
            }
        }
        let declaration = |path: &str| {
            self.files[path].nodes.iter().copied().find(|&index| {
                self.metadata(index).is_some_and(|node| {
                    node.name.eq_ignore_ascii_case(name)
                        && !node.qualified_name.contains("::")
                        && !matches!(node.kind.as_str(), "file" | "import" | "method")
                })
            })
        };
        if let Some(index) = declaration(path) {
            return Some((index, "exact-match", 0.9));
        }
        for import in self.files[path].analysis.imports.iter().rev() {
            for file in self
                .files
                .keys()
                .filter(|file| unit_matches(file, &import.module))
            {
                if let Some(index) = declaration(file) {
                    return Some((index, "import", 0.85));
                }
            }
        }
        None
    }
}

fn unit_matches(path: &str, unit: &str) -> bool {
    let stem = path
        .rsplit('/')
        .next()
        .unwrap_or(path)
        .rsplit_once('.')
        .map_or(path, |(stem, _)| stem);
    stem.eq_ignore_ascii_case(unit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Engine;
    use serde_json::Value;
    #[test]
    fn pascal_parenthesis_free_values_match_reference_graphs() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/upstream/resolution/pascal-values/cases.json"
        ))
        .unwrap();
        let normalize = |edges: Vec<prometeu_extract::Edge>| {
            let mut values = edges
                .into_iter()
                .filter(|edge| {
                    edge.metadata
                        .as_ref()
                        .and_then(|value| value["refName"].as_str())
                        .is_some_and(|name| name.starts_with('{'))
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
            std::fs::create_dir_all(&root).unwrap();
            for (path, source) in project["files"].as_object().unwrap() {
                let path = root.join(path);
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(path, source.as_str().unwrap()).unwrap();
            }
            let mut engine =
                Engine::open(&root, Some(&temporary.path().join("index.sqlite"))).unwrap();
            engine.sync().unwrap();
            let graph = Graph::load(&engine.connection, 1).unwrap();
            let actual = normalize(graph.canonical_edges.into_iter().flatten().collect());
            let expected = normalize(serde_json::from_value(project["edges"].clone()).unwrap());
            if actual != expected {
                differences.push(serde_json::json!({"project":project["name"],"missing":expected.iter().filter(|edge|!actual.contains(edge)).collect::<Vec<_>>(),"extra":actual.iter().filter(|edge|!expected.contains(edge)).collect::<Vec<_>>() }));
            }
        }
        if !differences.is_empty() {
            std::fs::write(
                std::env::temp_dir().join("prometeu-pascal-value-diffs.json"),
                serde_json::to_vec_pretty(&differences).unwrap(),
            )
            .unwrap();
        }
        assert!(
            differences.is_empty(),
            "{} Pascal value mismatches; see temp/prometeu-pascal-value-diffs.json",
            differences.len()
        );
    }
}
