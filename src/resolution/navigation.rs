//! Framework navigation takes precedence over ordinary name resolution.
use super::super::{Edge, Graph};
use anyhow::Result;
use std::collections::{HashMap, HashSet};

impl Graph {
    pub(crate) fn apply_navigation(&mut self) -> Result<()> {
        if !self.frameworks.iter().any(|name| {
            matches!(
                name.as_str(),
                "nextjs"
                    | "react-router"
                    | "sveltekit-router"
                    | "expo-router"
                    | "tanstack-router"
                    | "vue-router"
                    | "angular-router"
            )
        }) {
            return Ok(());
        }
        let nodes: Vec<_> = self
            .files
            .values()
            .flat_map(|file| {
                file.analysis
                    .extraction
                    .iter()
                    .flat_map(|extraction| extraction.nodes.iter().cloned())
            })
            .collect();
        let sources = self
            .files
            .iter()
            .map(|(path, file)| (path.as_str(), file.source.as_str()))
            .collect();
        let manifests = self
            .manifests
            .iter()
            .map(|(path, source)| (path.as_str(), source.as_str()))
            .collect();
        let resolve_module =
            |specifier: &str, from: &str, language: &str| -> Result<Option<String>> {
                let files = self.module_files_as(from, specifier, language);
                Ok((files.len() == 1).then(|| files[0].clone()))
            };
        let routing = crate::frameworks::frontend::navigation::Navigation::with_context(
            &nodes,
            &self.frameworks,
            &sources,
            &resolve_module,
            &manifests,
        )?;
        let ids: HashMap<_, _> = self
            .nodes
            .iter()
            .enumerate()
            .filter_map(|(index, _)| self.metadata(index).map(|node| (node.id.as_str(), index)))
            .collect();
        let mut replaced = HashSet::new();
        let mut unresolved = HashSet::new();
        let mut navigation = Vec::new();
        for (path, file) in &self.files {
            let Some(extraction) = &file.analysis.extraction else {
                continue;
            };
            let lines = crate::frameworks::frontend::href::lines(&file.source);
            for raw in &extraction.unresolved_references {
                if raw.reference_kind != "calls" && !raw.from_node_id.ends_with(":angular") {
                    continue;
                }
                let Some(&from) = ids.get(raw.from_node_id.as_str()) else {
                    continue;
                };
                let mut reference = raw.clone();
                reference.file_path = Some(path.clone());
                reference.language = Some(file.analysis.language.clone());
                let start = self.metadata(from).map(|node| node.start_line);
                let (kind, targets) = if let Some(binding) = routing.angular_binding(&reference)? {
                    ("references", vec![binding])
                } else {
                    ("navigates", routing.resolve(&reference, &lines, start)?)
                };
                if targets.is_empty() {
                    continue;
                }
                replaced.insert((
                    from,
                    reference.line,
                    reference.column,
                    reference.reference_name.clone(),
                ));
                unresolved.insert((
                    from,
                    reference.line as usize,
                    reference
                        .reference_name
                        .rsplit('.')
                        .next()
                        .unwrap_or(&reference.reference_name)
                        .to_string(),
                ));
                for (ordinal, target) in targets.into_iter().enumerate() {
                    let Some(&to) = ids.get(target.node.id.as_str()) else {
                        continue;
                    };
                    let mut metadata = target.metadata;
                    metadata["confidence"] = 0.95.into();
                    metadata["resolvedBy"] = "framework".into();
                    if ordinal == 0 {
                        metadata["framework"] = target.framework.into();
                    }
                    metadata["refName"] = reference.reference_name.clone().into();
                    if kind != reference.reference_kind {
                        metadata["refKind"] = reference.reference_kind.clone().into();
                    }
                    navigation.push((
                        from,
                        to,
                        prometeu_extract::Edge {
                            source: reference.from_node_id.clone(),
                            target: target.node.id.clone(),
                            kind: kind.into(),
                            line: Some(reference.line),
                            column: Some(reference.column),
                            provenance: None,
                            metadata: Some(metadata),
                        },
                    ));
                }
            }
        }
        if navigation.is_empty() {
            return Ok(());
        }
        let old_edges = std::mem::take(&mut self.edges);
        let old_canonical = std::mem::take(&mut self.canonical_edges);
        for adjacency in self.outgoing.iter_mut().chain(&mut self.incoming) {
            adjacency.clear();
        }
        for (edge, canonical) in old_edges.into_iter().zip(old_canonical) {
            let overridden = if let Some(raw) = &canonical {
                raw.line
                    .zip(raw.column)
                    .zip(
                        raw.metadata
                            .as_ref()
                            .and_then(|m| m.get("refName"))
                            .and_then(serde_json::Value::as_str),
                    )
                    .is_some_and(|((line, column), name)| {
                        replaced.contains(&(edge.from, line, column, name.into()))
                    })
            } else {
                edge.to.is_none()
                    && unresolved.contains(&(edge.from, edge.line, edge.target.clone()))
            };
            if !overridden {
                self.push_edge(edge);
                self.canonical_edges.push(canonical);
            }
        }
        for (from, to, canonical) in navigation {
            self.push_edge(Edge {
                column: canonical.column,
                from,
                to: Some(to),
                target: self.nodes[to].symbol.name.clone(),
                line: canonical.line.unwrap() as usize,
                kind: canonical.kind.clone(),
                method: "framework_match",
                confidence: "high",
                ambiguous: false,
            });
            self.canonical_edges.push(Some(canonical));
        }
        Ok(())
    }
}
