//! Task context assembled from one committed extraction and graph snapshot.
mod formatter;
pub(crate) mod query;
mod selection;
mod tokens;
use anyhow::{Result, ensure};
pub use formatter::{format_bytes, format_context_as_json, format_subgraph_tree};
use prometeu_extract::{Edge, Node};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
pub use tokens::extract_symbols_from_query;

pub const MAX_QUERY_SYMBOLS: usize = 64;
pub const MAX_QUERY_TERMS: usize = 128;
pub const LOW_CONFIDENCE_MARKER: &str = "### ⚠️ Low-confidence match";
const HIGH_VALUE_KINDS: &[&str] = &[
    "function",
    "method",
    "class",
    "interface",
    "type_alias",
    "struct",
    "union",
    "trait",
    "component",
    "route",
    "variable",
    "constant",
    "enum",
    "module",
    "namespace",
];
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct FindContextOptions {
    pub search_limit: usize,
    pub traversal_depth: usize,
    pub max_nodes: usize,
    pub min_score: f64,
    pub edge_kinds: Vec<String>,
    pub node_kinds: Option<Vec<String>>,
    pub seed_names: Vec<String>,
}
impl Default for FindContextOptions {
    fn default() -> Self {
        Self {
            search_limit: 3,
            traversal_depth: 1,
            max_nodes: 20,
            min_score: 0.3,
            edge_kinds: vec![],
            node_kinds: None,
            seed_names: vec![],
        }
    }
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct BuildContextOptions {
    pub max_nodes: usize,
    pub max_code_blocks: usize,
    pub max_code_block_size: usize,
    pub include_code: bool,
    pub search_limit: usize,
    pub traversal_depth: usize,
    pub min_score: f64,
}
impl Default for BuildContextOptions {
    fn default() -> Self {
        Self {
            max_nodes: 20,
            max_code_blocks: 5,
            max_code_block_size: 1500,
            include_code: true,
            search_limit: 3,
            traversal_depth: 1,
            min_score: 0.3,
        }
    }
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum TaskInput {
    Query(String),
    Task {
        title: String,
        description: Option<String>,
    },
}
impl TaskInput {
    fn query(&self) -> String {
        match self {
            Self::Query(query) => query.clone(),
            Self::Task { title, description } => {
                match description.as_deref().filter(|s| !s.is_empty()) {
                    Some(description) => format!("{title}: {description}"),
                    None => title.clone(),
                }
            }
        }
    }
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextSubgraph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub roots: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub confidence: String,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeBlock {
    pub content: String,
    pub file_path: String,
    pub start_line: u32,
    pub end_line: u32,
    pub language: String,
    pub node: Node,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextStats {
    pub node_count: usize,
    pub edge_count: usize,
    pub file_count: usize,
    pub code_block_count: usize,
    pub total_code_size: usize,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskContext {
    pub query: String,
    pub subgraph: ContextSubgraph,
    pub entry_points: Vec<Node>,
    pub code_blocks: Vec<CodeBlock>,
    pub related_files: Vec<String>,
    pub summary: String,
    pub stats: ContextStats,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ContextQuery {
    Text {
        query: String,
        kinds: Vec<String>,
        limit: usize,
    },
    ExactNames {
        names: Vec<String>,
        kinds: Vec<String>,
        limit: usize,
    },
    Substring {
        text: String,
        kinds: Vec<String>,
        exclude_prefix: bool,
        limit: usize,
    },
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ContextHit {
    pub id: String,
    pub score: f64,
}
pub type ContextQueryFn<'a> = dyn Fn(&ContextQuery) -> Result<Vec<ContextHit>> + 'a;
pub type PathPredicate<'a> = dyn Fn(&str) -> bool + 'a;

pub struct ContextBuilder<'a> {
    nodes: Vec<&'a Node>,
    by_id: HashMap<&'a str, &'a Node>,
    edges: &'a [Edge],
    outgoing: HashMap<&'a str, Vec<usize>>,
    incoming: HashMap<&'a str, Vec<usize>>,
    sources: &'a BTreeMap<&'a str, &'a str>,
    query: &'a ContextQueryFn<'a>,
    deprioritized: &'a PathPredicate<'a>,
    generated: &'a PathPredicate<'a>,
}
impl<'a> ContextBuilder<'a> {
    pub fn new(
        nodes: impl IntoIterator<Item = &'a Node>,
        edges: &'a [Edge],
        sources: &'a BTreeMap<&'a str, &'a str>,
        query: &'a ContextQueryFn<'a>,
        deprioritized: &'a PathPredicate<'a>,
        generated: &'a PathPredicate<'a>,
    ) -> Result<Self> {
        let nodes: Vec<_> = nodes.into_iter().collect();
        let by_id = nodes.iter().map(|node| (node.id.as_str(), *node)).collect();
        let (mut outgoing, mut incoming) =
            (HashMap::<_, Vec<_>>::new(), HashMap::<_, Vec<_>>::new());
        for (index, edge) in edges.iter().enumerate() {
            outgoing
                .entry(edge.source.as_str())
                .or_default()
                .push(index);
            incoming
                .entry(edge.target.as_str())
                .or_default()
                .push(index);
        }
        for indices in outgoing.values_mut() {
            indices.sort_by(|a, b| {
                let (a, b) = (&edges[*a], &edges[*b]);
                a.kind
                    .cmp(&b.kind)
                    .then(a.target.cmp(&b.target))
                    .then(a.line.cmp(&b.line))
                    .then(a.column.cmp(&b.column))
            });
        }
        for indices in incoming.values_mut() {
            indices.sort_by(|a, b| {
                let (a, b) = (&edges[*a], &edges[*b]);
                a.kind
                    .cmp(&b.kind)
                    .then(a.source.cmp(&b.source))
                    .then(a.line.cmp(&b.line))
                    .then(a.column.cmp(&b.column))
            });
        }
        Ok(Self {
            nodes,
            by_id,
            edges,
            outgoing,
            incoming,
            sources,
            query,
            deprioritized,
            generated,
        })
    }
    pub fn build(&self, input: &TaskInput, options: &BuildContextOptions) -> Result<TaskContext> {
        let query = input.query();
        let subgraph = self.find(
            &query,
            &FindContextOptions {
                search_limit: options.search_limit,
                traversal_depth: options.traversal_depth,
                max_nodes: options.max_nodes,
                min_score: options.min_score,
                ..Default::default()
            },
        )?;
        let entry_points: Vec<_> = subgraph
            .roots
            .iter()
            .filter_map(|id| subgraph.nodes.iter().find(|n| &n.id == id).cloned())
            .collect();
        let mut code_blocks = Vec::new();
        if options.include_code {
            let mut priority: Vec<_> = entry_points.iter().collect();
            for kinds in [&["function", "method"][..], &["class"][..]] {
                priority.extend(subgraph.nodes.iter().filter(|n| {
                    !subgraph.roots.contains(&n.id) && kinds.contains(&n.kind.as_str())
                }));
            }
            for node in priority {
                if code_blocks.len() >= options.max_code_blocks {
                    break;
                }
                if let Some(content) = self.get_code(&node.id).filter(|code| !code.is_empty()) {
                    let content = if utf16_len(&content) > options.max_code_block_size {
                        format!(
                            "{}\n... (truncated) ...",
                            utf16_prefix(&content, options.max_code_block_size)
                        )
                    } else {
                        content
                    };
                    code_blocks.push(CodeBlock {
                        content,
                        file_path: node.file_path.clone(),
                        start_line: node.start_line,
                        end_line: node.end_line,
                        language: node.language.clone(),
                        node: node.clone(),
                    });
                }
            }
        }
        let mut related_files: Vec<_> = subgraph
            .nodes
            .iter()
            .map(|n| n.file_path.clone())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        related_files.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
        let names = entry_points
            .iter()
            .take(3)
            .map(|n| n.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        let more = if entry_points.len() > 3 {
            format!(" and {} more", entry_points.len() - 3)
        } else {
            String::new()
        };
        let summary = format!(
            "Found {} relevant code symbols across {} files. Key entry points: {names}{more}. {} relationships identified.",
            subgraph.nodes.len(),
            related_files.len(),
            subgraph.edges.len()
        );
        let stats = ContextStats {
            node_count: subgraph.nodes.len(),
            edge_count: subgraph.edges.len(),
            file_count: related_files.len(),
            code_block_count: code_blocks.len(),
            total_code_size: code_blocks.iter().map(|b| utf16_len(&b.content)).sum(),
        };
        Ok(TaskContext {
            query,
            subgraph,
            entry_points,
            code_blocks,
            related_files,
            summary,
            stats,
        })
    }
    pub fn get_code(&self, id: &str) -> Option<String> {
        let node = self.by_id.get(id)?;
        if node.kind == "constant" && matches!(node.language.as_str(), "yaml" | "properties") {
            return Some(
                node.signature
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .unwrap_or(if node.qualified_name.is_empty() {
                        &node.name
                    } else {
                        &node.qualified_name
                    })
                    .to_string(),
            );
        }
        let source = self.sources.get(node.file_path.as_str())?;
        let lines: Vec<_> = source.split('\n').collect();
        Some(
            lines
                .iter()
                .skip(node.start_line.saturating_sub(1) as usize)
                .take(
                    node.end_line
                        .saturating_sub(node.start_line.saturating_sub(1))
                        as usize,
                )
                .copied()
                .collect::<Vec<_>>()
                .join("\n"),
        )
    }
    pub fn markdown(&self, context: &TaskContext) -> String {
        formatter::markdown(context, self.generated)
            + &formatter::call_paths(&context.subgraph)
            + &if context.subgraph.confidence == "low" {
                formatter::low_confidence(&context.entry_points)
            } else {
                String::new()
            }
    }
}
fn utf16_len(source: &str) -> usize {
    source.encode_utf16().count()
}
fn utf16_prefix(source: &str, limit: usize) -> String {
    String::from_utf16_lossy(&source.encode_utf16().take(limit).collect::<Vec<_>>())
}
fn is_test(path: &str) -> bool {
    crate::classify::is_test_file(path)
}
fn directory(path: &str) -> &str {
    path.rsplit_once('/')
        .map(|(dir, _)| if dir.is_empty() { "/" } else { dir })
        .unwrap_or(".")
}
#[cfg(test)]
mod tests;
