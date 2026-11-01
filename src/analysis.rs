//! Snapshot graph measurements and import components.
#[path = "analysis/dead_code.rs"]
mod dead_code;
use super::{Graph, with_graph};
use crate::index::validate_limits;
use crate::{Engine, SymbolLocation};
use anyhow::{Context, Result, ensure};
pub use dead_code::{
    DeadCodeEntry, DeadCodeExclusions, DeadCodeOptions, DeadCodeReport, DeadCodeResult,
};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashSet};

#[derive(Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeMetrics {
    pub incoming_edge_count: usize,
    pub outgoing_edge_count: usize,
    pub call_count: usize,
    pub caller_count: usize,
    pub child_count: usize,
    pub depth: usize,
}

#[derive(Debug, Serialize)]
pub struct MetricsResult {
    pub generation: u64,
    pub node: Option<SymbolLocation>,
    pub metrics: NodeMetrics,
    pub ambiguous: bool,
    pub truncated: bool,
}

#[derive(Debug, Serialize)]
pub struct CyclesResult {
    pub generation: u64,
    /// Strongly connected groups, not ordered dependency paths.
    pub groups: Vec<Vec<String>>,
    pub total: usize,
    pub ambiguous: bool,
    pub truncated: bool,
}

impl Engine {
    /// Measure resolved graph edges around a search-result ID in one snapshot.
    pub fn metrics(&self, id: i64, max_bytes: usize) -> Result<MetricsResult> {
        validate_limits(1, max_bytes)?;
        ensure!(id > 0, "id must be positive");
        with_graph(self, |graph, _| {
            let index = *graph
                .ids
                .get(&id)
                .context("symbol ID is not present in this indexed snapshot")?;
            let mut metrics = NodeMetrics::default();
            let mut ambiguous = false;
            for &edge in &graph.incoming[index] {
                let edge = &graph.edges[edge];
                metrics.incoming_edge_count += 1;
                metrics.caller_count += usize::from(edge.kind == "calls");
                ambiguous |= edge.ambiguous || edge.confidence == "low";
            }
            for &edge in &graph.outgoing[index] {
                let edge = &graph.edges[edge];
                if edge.to.is_none() {
                    continue;
                }
                metrics.outgoing_edge_count += 1;
                metrics.call_count += usize::from(edge.kind == "calls");
                metrics.child_count += usize::from(edge.kind == "contains");
                ambiguous |= edge.ambiguous || edge.confidence == "low";
            }
            let mut current = index;
            let mut visited = HashSet::new();
            while visited.insert(current) {
                let Some(parent) = graph.incoming[current]
                    .iter()
                    .map(|&edge| &graph.edges[edge])
                    .find(|edge| edge.kind == "contains")
                else {
                    break;
                };
                metrics.depth += 1;
                current = parent.from;
            }
            let mut result = MetricsResult {
                generation: graph.generation,
                node: Some(graph.location(index)),
                metrics,
                ambiguous,
                truncated: graph.truncated,
            };
            if serde_json::to_vec(&result)?.len() > max_bytes {
                result.node = None;
                result.truncated = true;
            }
            Ok(result)
        })
    }

    /// Return mutually importing file groups, smallest first, from the indexed graph.
    pub fn cycles(&self, limit: usize, max_bytes: usize) -> Result<CyclesResult> {
        validate_limits(limit, max_bytes)?;
        with_graph(self, |graph, _| {
            let (pairs, ambiguous) = import_pairs(graph);
            let groups = file_cycles(&pairs);
            let total = groups.len();
            let mut result = CyclesResult {
                generation: graph.generation,
                groups: groups.into_iter().take(limit).collect(),
                total,
                ambiguous,
                truncated: graph.truncated || total > limit,
            };
            while serde_json::to_vec(&result)?.len() > max_bytes {
                result.groups.pop();
                result.truncated = true;
            }
            Ok(result)
        })
    }
}

fn import_pairs(graph: &Graph) -> (Vec<(String, String)>, bool) {
    let mut pairs = BTreeSet::new();
    let mut ambiguous = false;
    for edge in &graph.edges {
        if edge.kind != "imports" {
            continue;
        }
        let Some(to) = edge.to else {
            continue;
        };
        let from = &graph.nodes[edge.from].path;
        let to = &graph.nodes[to].path;
        if from == to {
            continue;
        }
        ambiguous |= edge.ambiguous || edge.confidence == "low";
        pairs.insert((from.clone(), to.clone()));
    }
    (pairs.into_iter().collect(), ambiguous)
}

/// Iterative Tarjan traversal avoids native stack growth on deep import graphs.
fn file_cycles(pairs: &[(String, String)]) -> Vec<Vec<String>> {
    let names: Vec<_> = pairs
        .iter()
        .flat_map(|(from, to)| [from.clone(), to.clone()])
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let ids: BTreeMap<_, _> = names
        .iter()
        .enumerate()
        .map(|(index, name)| (name.as_str(), index))
        .collect();
    let mut adjacency = vec![Vec::new(); names.len()];
    for (from, to) in pairs {
        adjacency[ids[from.as_str()]].push(ids[to.as_str()]);
    }
    for targets in &mut adjacency {
        targets.sort_unstable();
        targets.dedup();
    }
    let mut indices = vec![None; names.len()];
    let mut low = vec![0; names.len()];
    let mut active = vec![false; names.len()];
    let mut stack = Vec::new();
    let mut counter = 0;
    let mut groups = Vec::new();
    for start in 0..names.len() {
        if indices[start].is_some() {
            continue;
        }
        indices[start] = Some(counter);
        low[start] = counter;
        counter += 1;
        stack.push(start);
        active[start] = true;
        let mut work = vec![(start, 0)];
        while let Some(&mut (node, ref mut position)) = work.last_mut() {
            if *position < adjacency[node].len() {
                let next = adjacency[node][*position];
                *position += 1;
                if let Some(index) = indices[next] {
                    if active[next] {
                        low[node] = low[node].min(index);
                    }
                } else {
                    indices[next] = Some(counter);
                    low[next] = counter;
                    counter += 1;
                    stack.push(next);
                    active[next] = true;
                    work.push((next, 0));
                }
                continue;
            }
            work.pop();
            if Some(low[node]) == indices[node] {
                let mut group = Vec::new();
                loop {
                    let next = stack.pop().unwrap();
                    active[next] = false;
                    group.push(names[next].clone());
                    if next == node {
                        break;
                    }
                }
                if group.len() > 1 {
                    group.sort();
                    groups.push(group);
                }
            }
            if let Some(&(parent, _)) = work.last() {
                low[parent] = low[parent].min(low[node]);
            }
        }
    }
    groups.sort_by(|a, b| a.len().cmp(&b.len()).then_with(|| a[0].cmp(&b[0])));
    groups
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import_components_match_independent_reference() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../tests/fixtures/upstream/analysis/cycles.json"
        ))
        .unwrap();
        for case in fixture["cases"].as_array().unwrap() {
            let pairs = case["pairs"]
                .as_array()
                .unwrap()
                .iter()
                .map(|pair| {
                    (
                        pair["source"].as_str().unwrap().to_owned(),
                        pair["target"].as_str().unwrap().to_owned(),
                    )
                })
                .collect::<Vec<_>>();
            assert_eq!(
                serde_json::to_value(file_cycles(&pairs)).unwrap(),
                case["expected"],
                "{}",
                case["id"]
            );
        }
    }

    #[test]
    fn deep_import_graphs_use_no_recursive_stack() {
        let mut pairs: Vec<_> = (0..30_000)
            .map(|index| (format!("{index:05}"), format!("{:05}", index + 1)))
            .collect();
        assert!(file_cycles(&pairs).is_empty());
        pairs.push(("30000".into(), "00000".into()));
        let groups = file_cycles(&pairs);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].len(), 30_001);
    }
}
