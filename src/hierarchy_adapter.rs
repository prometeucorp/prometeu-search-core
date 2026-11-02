//! Bounded hierarchy queries over the committed graph snapshot.
use super::with_graph;
use crate::{
    Engine,
    hierarchy::{TypeHierarchy, build_type_hierarchy},
};
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Serialize)]
pub struct HierarchyResult {
    pub generation: u64,
    pub hierarchy: Option<TypeHierarchy>,
    pub ambiguous: bool,
    pub truncated: bool,
}

impl Engine {
    /// Read ancestry, subtypes, and member override matches for a search-result ID.
    pub fn hierarchy(&self, id: i64, max_bytes: usize) -> Result<HierarchyResult> {
        crate::index::validate_limits(1, max_bytes)?;
        ensure!(id > 0, "id must be positive");
        with_graph(self, |graph, _| {
            let &index = graph
                .ids
                .get(&id)
                .context("symbol ID is not present in this indexed snapshot")?;
            let focus = &graph.nodes[index];
            let focus = graph.files[&focus.path]
                .analysis
                .extraction
                .as_ref()
                .and_then(|extraction| extraction.nodes.get(focus.ordinal))
                .context("symbol has no canonical extraction record")?;
            let mut nodes = graph
                .files
                .values()
                .filter_map(|file| file.analysis.extraction.as_ref())
                .flat_map(|extraction| extraction.nodes.iter().cloned())
                .collect::<Vec<_>>();
            for node in &mut nodes {
                node.is_exported.get_or_insert(false);
                node.is_async.get_or_insert(false);
                node.is_static.get_or_insert(false);
                node.is_abstract.get_or_insert(false);
            }
            let edges = graph
                .canonical_edges
                .iter()
                .flatten()
                .cloned()
                .collect::<Vec<_>>();
            let hierarchy = build_type_hierarchy(&nodes, &edges, &focus.id, true)?;
            let ambiguous = hierarchy.as_ref().is_some_and(|hierarchy| {
                hierarchy
                    .ancestors
                    .iter()
                    .chain(&hierarchy.descendants)
                    .any(|entry| {
                        entry.edge.metadata.as_ref().is_some_and(|metadata| {
                            metadata["ambiguous"] == true
                                || metadata["confidence"]
                                    .as_f64()
                                    .is_some_and(|score| score < 0.7)
                        })
                    })
            });
            let mut result = HierarchyResult {
                generation: graph.generation,
                truncated: graph.truncated
                    || graph.resolution_truncated.get()
                    || hierarchy
                        .as_ref()
                        .is_some_and(|hierarchy| hierarchy.bounded),
                ambiguous,
                hierarchy,
            };
            while serde_json::to_vec(&result)?.len() > max_bytes {
                result.truncated = true;
                let Some(hierarchy) = result.hierarchy.as_mut() else {
                    break;
                };
                if !hierarchy.overrides.is_empty() {
                    hierarchy.overrides.clear();
                } else if !hierarchy.descendants.is_empty() {
                    let removed = hierarchy
                        .descendants
                        .split_off(hierarchy.descendants.len() / 2);
                    let mut counts = HashMap::<&str, usize>::new();
                    for entry in &removed {
                        *counts.entry(&entry.parent_id).or_default() += 1;
                    }
                    for entry in &mut hierarchy.descendants {
                        entry.hidden_subtypes +=
                            counts.get(entry.node.id.as_str()).copied().unwrap_or(0);
                    }
                    hierarchy.bounded = true;
                } else if !hierarchy.ancestors.is_empty() {
                    hierarchy.ancestors.truncate(hierarchy.ancestors.len() / 2);
                } else {
                    result.hierarchy = None;
                }
            }
            Ok(result)
        })
    }
}
