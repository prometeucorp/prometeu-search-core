//! Final type visibility and generic parameter gates across resolver strategies.
use super::*;

type Site = (usize, usize, Option<usize>, usize);
pub(in crate::graph) type VisibleTargets = HashMap<Site, Option<usize>>;

impl Graph {
    pub(in crate::graph) fn visible_type_targets(
        &self,
        inheritance: bool,
    ) -> Result<VisibleTargets> {
        if !self
            .files
            .values()
            .any(|file| type_parameters::LANGUAGES.contains(&file.analysis.language.as_str()))
        {
            return Ok(HashMap::new());
        }
        let nodes: Vec<_> = self
            .files
            .values()
            .filter_map(|file| file.analysis.extraction.as_ref())
            .flat_map(|extraction| extraction.nodes.iter().cloned())
            .collect();
        let sources: BTreeMap<_, _> = self
            .files
            .iter()
            .map(|(path, file)| (path.as_str(), file.source.as_str()))
            .collect();
        let indices: HashMap<_, _> = self
            .nodes
            .iter()
            .enumerate()
            .filter_map(|(index, _)| self.metadata(index).map(|node| (node.id.as_str(), index)))
            .collect();
        let no_module = |_: &str, _: &str, _: &str| Ok(None);
        let mut visibility = self
            .files
            .values()
            .any(|file| file.analysis.language == "swift")
            .then(|| crate::frameworks::mobile::Bindings::new(&nodes, &sources, &[], &no_module))
            .transpose()?;
        let supers = self
            .edges
            .iter()
            .filter(|edge| matches!(edge.kind.as_str(), "extends" | "implements"))
            .filter_map(|edge| {
                let from = self.metadata(edge.from)?;
                if from.language != "scala" {
                    return None;
                }
                Some((from.name.as_str(), self.metadata(edge.to?)?.name.as_str()))
            });
        let mut parameters = type_parameters::Parameters::new(&nodes, &sources, supers)?;
        let canonical: HashMap<_, _> = nodes.iter().map(|node| (node.id.as_str(), node)).collect();
        let mut targets = HashMap::new();
        for (path, file) in &self.files {
            if !type_parameters::LANGUAGES.contains(&file.analysis.language.as_str()) {
                continue;
            }
            for reference in self.resolution_references(path) {
                if matches!(reference.kind.as_str(), "extends" | "implements") != inheritance {
                    continue;
                }
                let Some(from) = reference
                    .caller
                    .and_then(|ordinal| file.nodes.get(ordinal))
                    .copied()
                else {
                    continue;
                };
                let Some(raw) = self.canonical_reference(from, &reference) else {
                    continue;
                };
                let mut raw = raw.clone();
                raw.file_path = Some(path.clone());
                raw.language = Some(file.analysis.language.clone());
                if file.analysis.language != "swift" && !type_parameters::applies(&raw)? {
                    continue;
                }
                for target in self.resolve(path, from, &reference).0 {
                    let Some(node) = self
                        .metadata(target)
                        .and_then(|node| canonical.get(node.id.as_str()).copied())
                    else {
                        continue;
                    };
                    let selected = if let Some(visibility) = &mut visibility {
                        let candidate = crate::frameworks::mobile::Binding {
                            target_node_id: node.id.clone(),
                            confidence: 1.0,
                            also_targets: Vec::new(),
                            metadata: None,
                        };
                        visibility
                            .swift_type_target(&raw, candidate)?
                            .and_then(|binding| {
                                canonical.get(binding.target_node_id.as_str()).copied()
                            })
                    } else {
                        Some(node)
                    };
                    let selected = match selected {
                        Some(node) => parameters.gate(&raw, node)?,
                        None => None,
                    };
                    let selected = selected.and_then(|node| indices.get(node.id.as_str()).copied());
                    if selected == Some(target) {
                        continue;
                    }
                    targets.insert((from, reference.line, reference.column, target), selected);
                }
            }
        }
        Ok(targets)
    }

    pub(in crate::graph) fn apply_type_visibility(
        targets: &VisibleTargets,
        from: usize,
        reference: &Reference,
        candidates: &mut Vec<usize>,
    ) {
        candidates.retain_mut(|candidate| {
            match targets.get(&(from, reference.line, reference.column, *candidate)) {
                Some(Some(target)) => {
                    *candidate = *target;
                    true
                }
                Some(None) => false,
                None => true,
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    #[test]
    fn swift_final_type_decisions_match_indexed_reference() -> Result<()> {
        let corpus: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/upstream/frameworks/swift-type-gate.json"
        ))?;
        let mut differences = Vec::new();
        for case in corpus["cases"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|case| !case["origin"].as_str().unwrap().starts_with("supplemental"))
        {
            let temporary = tempfile::tempdir()?;
            let root = temporary.path().join("project");
            let database = temporary.path().join("index.sqlite");
            for (file, source) in case["sources"].as_object().unwrap() {
                let path = root.join(file);
                std::fs::create_dir_all(path.parent().unwrap())?;
                std::fs::write(path, source.as_str().unwrap())?;
            }
            let type_ids: HashSet<_> = case["nodes"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|node| {
                    matches!(
                        node["kind"].as_str(),
                        Some("class" | "struct" | "enum" | "interface" | "protocol" | "type_alias")
                    )
                })
                .map(|node| node["id"].as_str().unwrap())
                .collect();
            let mut selected = BTreeMap::new();
            for answer in case["answers"].as_array().unwrap() {
                if !answer["input"]["targetNodeId"]
                    .as_str()
                    .is_some_and(|id| type_ids.contains(id))
                {
                    continue;
                }
                let raw = &answer["reference"];
                let key = (
                    raw["fromNodeId"].as_str().unwrap().to_string(),
                    raw["referenceName"].as_str().unwrap().to_string(),
                    raw["line"].as_u64().unwrap() as u32,
                    raw["column"].as_u64().unwrap() as u32,
                );
                selected.insert(
                    key,
                    answer["expected"]["targetNodeId"]
                        .as_str()
                        .map(str::to_string),
                );
            }
            let mut engine = crate::Engine::open(&root, Some(&database))?;
            engine.sync()?;
            for reopened in [false, true] {
                if reopened {
                    drop(engine);
                    for path in case["sources"].as_object().unwrap().keys() {
                        std::fs::write(root.join(path), "changed live source")?;
                    }
                    engine = crate::Engine::open(&root, Some(&database))?;
                }
                let graph = Graph::load(&engine.connection, 1)?;
                for ((from, name, line, column), expected) in &selected {
                    let actual: std::collections::BTreeSet<_> = graph
                        .canonical_edges
                        .iter()
                        .flatten()
                        .filter(|edge| {
                            edge.source == *from
                                && edge.line == Some(*line)
                                && edge.column == Some(*column)
                                && edge
                                    .metadata
                                    .as_ref()
                                    .is_some_and(|metadata| metadata["refName"] == *name)
                        })
                        .map(|edge| edge.target.as_str())
                        .collect();
                    let wanted: std::collections::BTreeSet<_> =
                        expected.as_deref().into_iter().collect();
                    if actual != wanted {
                        differences.push(json!({"case":case["origin"], "reopened":reopened, "from":from,"name":name,"line":line,"column":column,"actual":actual,"expected":wanted}));
                    }
                }
            }
        }
        let path = std::env::temp_dir().join("prometeu-swift-type-differences.json");
        std::fs::write(&path, serde_json::to_vec_pretty(&differences)?)?;
        ensure!(
            differences.is_empty(),
            "{} Swift type references differ; records in {}",
            differences.len(),
            path.display()
        );
        Ok(())
    }
}
