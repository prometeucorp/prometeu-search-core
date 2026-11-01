use super::{Graph, with_graph};
use crate::Engine;
use anyhow::Result;
use serde::Serialize;
use std::collections::{BTreeMap, HashSet};

const MAX_FILES: usize = 1_000;
const MAX_EDGES: usize = 4_000;
const MAX_BYTES: usize = 512 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct OverviewFile {
    pub path: String,
    pub language: String,
    pub symbols: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct OverviewEdge {
    pub from: String,
    pub to: String,
    pub count: usize,
    /// At least one relationship has weak evidence; all relationships remain static candidates.
    pub uncertain: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Overview {
    pub generation: u64,
    pub files: Vec<OverviewFile>,
    pub edges: Vec<OverviewEdge>,
    pub truncated: bool,
}

impl Engine {
    /// Bounded file graph from one indexed snapshot. Refresh first when freshness is required.
    pub fn overview(&self) -> Result<Overview> {
        with_graph(self, |graph, _| graph.overview())
    }
}

impl Graph {
    fn overview(&self) -> Result<Overview> {
        if let Some(overview) = self.overview_cache.borrow().as_ref() {
            return Ok(overview.clone());
        }
        let overview = self.build_overview()?;
        *self.overview_cache.borrow_mut() = Some(overview.clone());
        Ok(overview)
    }

    fn build_overview(&self) -> Result<Overview> {
        let mut result = Overview {
            generation: self.generation,
            files: Vec::new(),
            edges: Vec::new(),
            truncated: self.truncated,
        };
        // Account for JSON escaping and commas without repeatedly serializing the whole graph.
        let mut bytes = serde_json::to_vec(&result)?.len();
        for (path, file) in &self.files {
            let entry = OverviewFile {
                path: path.clone(),
                language: file.analysis.language.clone(),
                symbols: file.nodes.len(),
            };
            let size = serde_json::to_vec(&entry)?.len() + 1;
            if result.files.len() == MAX_FILES || bytes + size > MAX_BYTES {
                result.truncated = true;
                break;
            }
            bytes += size;
            result.files.push(entry);
        }
        let included: HashSet<_> = result.files.iter().map(|file| file.path.as_str()).collect();
        let mut pairs = BTreeMap::<(&str, &str), (usize, bool)>::new();
        for edge in &self.edges {
            let Some(to) = edge.to else { continue };
            let from = self.nodes[edge.from].path.as_str();
            let to = self.nodes[to].path.as_str();
            if from == to || !included.contains(from) || !included.contains(to) {
                continue;
            }
            let entry = pairs.entry((from, to)).or_default();
            entry.0 += 1;
            entry.1 |= edge.ambiguous || matches!(edge.confidence, "low" | "unknown");
        }
        for ((from, to), (count, uncertain)) in pairs {
            let edge = OverviewEdge {
                from: from.into(),
                to: to.into(),
                count,
                uncertain,
            };
            let size = serde_json::to_vec(&edge)?.len() + 1;
            if result.edges.len() == MAX_EDGES || bytes + size > MAX_BYTES {
                result.truncated = true;
                break;
            }
            bytes += size;
            result.edges.push(edge);
        }
        Ok(result)
    }
}
