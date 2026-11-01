//! Cached React binding answers from one committed graph snapshot.
use super::*;
#[derive(Default)]
pub(in crate::graph) struct ReactBindings {
    references: HashMap<(usize, String, String, usize, usize), usize>,
    metadata: HashMap<(usize, usize, String), (f64, bool)>,
    pending: HashMap<String, Vec<Reference>>,
}
impl Graph {
    pub(in crate::graph) fn initialize_react(&mut self) -> Result<()> {
        if !self.frameworks.iter().any(|name| name == "react") {
            return Ok(());
        }
        let mut nodes: Vec<_> = self
            .files
            .values()
            .filter_map(|file| file.analysis.extraction.as_ref())
            .flat_map(|extraction| extraction.nodes.iter().cloned())
            .collect();
        nodes.sort_by(|a, b| {
            (&a.file_path, a.start_line, &a.id).cmp(&(&b.file_path, b.start_line, &b.id))
        });
        let sources: BTreeMap<_, _> = self
            .files
            .iter()
            .map(|(path, file)| (path.as_str(), file.source.as_str()))
            .collect();
        let languages: HashMap<_, _> = nodes
            .iter()
            .map(|node| (node.id.as_str(), node.language.as_str()))
            .collect();
        let mut references = Vec::new();
        for (path, file) in &self.files {
            if let Some(extraction) = &file.analysis.extraction {
                for reference in &extraction.unresolved_references {
                    let mut reference = reference.clone();
                    reference.file_path = Some(path.clone());
                    if reference.language.is_none() {
                        reference.language = Some(
                            languages
                                .get(reference.from_node_id.as_str())
                                .copied()
                                .unwrap_or(&file.analysis.language)
                                .to_owned(),
                        );
                    }
                    references.push(reference);
                }
            }
        }
        let imports = self.synthesis_imports()?;
        let mut bindings = HashMap::new();
        for binding in &imports {
            bindings
                .entry((binding.file.as_str(), binding.local_name.as_str()))
                .or_insert(binding);
        }
        let defaults = std::cell::RefCell::new(HashMap::new());
        let resolve_module = |specifier: &str, from: &str, language: &str| {
            let paths = self.module_files_as(from, specifier, language);
            (paths.len() == 1).then(|| paths[0].clone())
        };
        let resolve_import = |reference: &prometeu_extract::UnresolvedReference| {
            self.synthesis_import_reference(reference, &bindings, &defaults)
        };
        let answers = crate::synthesis::react_references(
            &nodes,
            &sources,
            &references,
            &imports,
            &resolve_module,
            &resolve_import,
        )?;
        let indices: HashMap<_, _> = self
            .nodes
            .iter()
            .enumerate()
            .filter_map(|(index, _)| self.metadata(index).map(|node| (node.id.as_str(), index)))
            .collect();
        let mut cached = ReactBindings::default();
        for (reference, answer) in references.iter().zip(answers) {
            let Some(answer) = answer else { continue };
            if reference.reference_kind == "function_ref" {
                continue;
            }

            let Some((&from, &to)) = indices
                .get(reference.from_node_id.as_str())
                .zip(indices.get(answer.target_node_id.as_str()))
            else {
                continue;
            };
            // A unique same-file declaration scores 0.9 in the exact-name
            // strategy, above React's 0.8/0.85 heuristic. Preserve that winner.
            if answer.confidence < 0.9 && self.react_local_name_wins(reference, to) {
                continue;
            }
            cached
                .pending
                .entry(self.nodes[from].path.clone())
                .or_default()
                .push(Reference {
                    column: Some(reference.column as usize),
                    caller: Some(self.nodes[from].ordinal),
                    target: reference.reference_name.clone(),
                    kind: reference.reference_kind.clone(),
                    line: reference.line as usize,
                    qualifier: None,
                    receiver_type: None,
                    shadowed: false,
                    owner: None,
                });
            cached.references.insert(
                (
                    from,
                    reference.reference_name.clone(),
                    reference.reference_kind.clone(),
                    reference.line as usize,
                    reference.column as usize,
                ),
                to,
            );
            cached.metadata.insert(
                (from, to, reference.reference_name.clone()),
                (
                    answer.confidence,
                    answer.metadata.is_some_and(|m| m["layout"] == true),
                ),
            );
        }
        self.react_bindings = cached;
        Ok(())
    }
    fn react_local_name_wins(
        &self,
        reference: &prometeu_extract::UnresolvedReference,
        target: usize,
    ) -> bool {
        let Some(node) = self.metadata(target) else {
            return false;
        };
        if Some(node.file_path.as_str()) != reference.file_path.as_deref()
            || node.name != reference.reference_name
        {
            return false;
        }
        let own: Vec<_> = self.files[&node.file_path]
            .nodes
            .iter()
            .filter_map(|&index| self.metadata(index))
            .filter(|other| {
                other.name == node.name
                    && !matches!(other.kind.as_str(), "file" | "import" | "export")
            })
            .collect();
        if own.len() != 1 {
            return false;
        }
        if node.id == reference.from_node_id {
            return self.names.get(&node.name).is_some_and(|names| {
                names
                    .iter()
                    .filter_map(|&index| self.metadata(index))
                    .filter(|other| !matches!(other.kind.as_str(), "file" | "import" | "export"))
                    .count()
                    == 1
            });
        }
        let Some((owner, _)) = node.qualified_name.rsplit_once("::") else {
            return node.kind != "method";
        };
        if node.kind != "method"
            && self.files[&node.file_path]
                .nodes
                .iter()
                .filter_map(|&index| self.metadata(index))
                .filter(|other| other.qualified_name == owner)
                .all(|other| matches!(other.kind.as_str(), "function" | "method"))
        {
            return true;
        }
        self.files[&node.file_path]
            .nodes
            .iter()
            .filter_map(|&index| self.metadata(index))
            .find(|other| other.id == reference.from_node_id)
            .is_some_and(|from| {
                from.qualified_name == owner
                    || from.qualified_name.starts_with(&format!("{owner}::"))
            })
    }
    pub(super) fn append_react_references(&self, path: &str, references: &mut Vec<Reference>) {
        let mut seen: HashSet<_> = references
            .iter()
            .map(|r| {
                (
                    r.caller,
                    r.qualifier
                        .as_ref()
                        .map_or_else(|| r.target.clone(), |q| format!("{q}::{}", r.target))
                        .replace("->", "::")
                        .replace('.', "::"),
                    r.kind.clone(),
                    r.line,
                    r.column,
                )
            })
            .collect();
        for reference in self.react_bindings.pending.get(path).into_iter().flatten() {
            if seen.insert((
                reference.caller,
                reference.target.replace("->", "::").replace('.', "::"),
                reference.kind.clone(),
                reference.line,
                reference.column,
            )) {
                references.push(reference.clone());
            }
        }
    }
    pub(super) fn react_reference(
        &self,
        from: usize,
        name: &str,
        kind: &str,
        line: usize,
        column: Option<usize>,
    ) -> Option<usize> {
        self.react_bindings
            .references
            .get(&(
                from,
                name.to_owned(),
                kind.to_owned(),
                line,
                column.unwrap_or(0),
            ))
            .copied()
    }
    pub(super) fn react_confidence(&self, from: usize, to: usize, name: &str) -> Option<f64> {
        self.react_bindings
            .metadata
            .get(&(from, to, name.to_owned()))
            .map(|&(confidence, _)| confidence)
    }
    pub(super) fn react_layout(&self, from: usize, to: usize, name: &str) -> bool {
        self.react_bindings
            .metadata
            .get(&(from, to, name.to_owned()))
            .is_some_and(|&(_, layout)| layout)
    }
    pub(super) fn react_claims(&self, from: usize, name: &str) -> bool {
        self.frameworks.iter().any(|f| f == "react")
            && self.metadata(from).is_some_and(|node| {
                matches!(
                    node.language.as_str(),
                    "typescript" | "javascript" | "tsx" | "jsx"
                )
            })
            && (name.starts_with("lazy-import:")
                || name.starts_with("layout:")
                || name.strip_prefix("import:").is_some_and(|tail| {
                    tail.rsplit_once('#').is_some_and(|(path, name)| {
                        !path.is_empty()
                            && !name.is_empty()
                            && name
                                .bytes()
                                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'$'))
                            && !name.as_bytes()[0].is_ascii_digit()
                    })
                }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    #[test]
    fn persisted_react_framework_edges_match_complete_reference_records() {
        let corpus: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/upstream/synthesis/react-bindings.json"
        ))
        .unwrap();
        let mut differences = Vec::new();
        for case in corpus["cases"].as_array().unwrap() {
            let temporary = tempfile::tempdir().unwrap();
            let root = temporary.path().join("project");
            std::fs::create_dir(&root).unwrap();
            let database = temporary.path().join("index.sqlite");
            for (path, source) in case["files"]
                .as_object()
                .unwrap()
                .iter()
                .chain(case["assets"].as_object().unwrap())
            {
                let path = root.join(path);
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(path, source.as_str().unwrap()).unwrap();
            }
            let mut engine = crate::Engine::open(&root, Some(&database)).unwrap();
            engine.sync().unwrap();
            drop(engine);
            let engine = crate::Engine::open(&root, Some(&database)).unwrap();
            let graph = Graph::load(&engine.connection, 1).unwrap();
            let mut actual: Vec<_> = graph
                .canonical_edges
                .iter()
                .flatten()
                .filter(|edge| {
                    edge.metadata
                        .as_ref()
                        .is_some_and(|m| m["framework"] == "react")
                })
                .map(|edge| serde_json::to_value(edge).unwrap())
                .collect();
            let canonical: Vec<prometeu_extract::Edge> =
                serde_json::from_value(case["edges"].clone()).unwrap();
            let mut expected: Vec<_> = canonical
                .iter()
                .map(|edge| serde_json::to_value(edge).unwrap())
                .collect();
            actual.sort_by_key(Value::to_string);
            expected.sort_by_key(Value::to_string);
            if actual != expected {
                std::fs::write(
                    "/tmp/prometeu-react-actual.json",
                    serde_json::to_string_pretty(&actual).unwrap(),
                )
                .unwrap();
                std::fs::write(
                    "/tmp/prometeu-react-expected.json",
                    serde_json::to_string_pretty(&expected).unwrap(),
                )
                .unwrap();
            }
            if actual != expected {
                differences.push(serde_json::json!({"id":case["id"],"origin":case["origin"],"actual":actual,"expected":expected}));
            }
        }
        if !differences.is_empty() {
            std::fs::write(
                "/tmp/prometeu-react-differences.json",
                serde_json::to_string_pretty(&differences).unwrap(),
            )
            .unwrap();
        }
        assert!(
            differences.is_empty(),
            "{} project differences; see /tmp/prometeu-react-differences.json",
            differences.len()
        );
    }
}
