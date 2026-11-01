//! Bounded public context assembly over the graph's committed SQLite snapshot.
use super::with_graph;
use crate::{
    Engine,
    context::{BuildContextOptions, ContextBuilder, TaskContext, TaskInput},
};
use anyhow::{Result, ensure};
use serde::Serialize;
use std::collections::{BTreeMap, HashSet};

#[derive(Debug, Serialize)]
pub struct ContextResult {
    pub generation: u64,
    pub truncated: bool,
    pub context: TaskContext,
    #[serde(skip)]
    markdown: String,
}
impl ContextResult {
    /// Markdown from the same snapshot and byte budget as the structured result.
    pub fn markdown(&self) -> &str {
        &self.markdown
    }
}

impl Engine {
    /// Assemble task context without reading live files or implicitly refreshing.
    pub fn context(
        &self,
        input: &TaskInput,
        options: &BuildContextOptions,
        max_bytes: usize,
    ) -> Result<ContextResult> {
        crate::index::validate_limits(1, max_bytes)?;
        let query = match input {
            TaskInput::Query(query) => query.clone(),
            TaskInput::Task { title, description } => format!(
                "{title}{}",
                description
                    .as_ref()
                    .filter(|description| !description.is_empty())
                    .map_or(String::new(), |description| format!(": {description}"))
            ),
        };
        ensure!(
            !query.trim().is_empty() && query.encode_utf16().count() <= 10_000,
            "context query must contain 1 to 10000 UTF-16 units"
        );
        ensure!(
            options.max_nodes > 0 && options.max_nodes <= usize::MAX / 15,
            "max_nodes must be positive and fit the ranking counters"
        );
        ensure!(
            options.search_limit > 0 && options.search_limit <= (i64::MAX as usize) / 15,
            "context search limit must be a positive SQLite count"
        );
        ensure!(
            options.min_score.is_finite() && (0.0..=1.0).contains(&options.min_score),
            "min_score must be between 0 and 1"
        );
        with_graph(self, |graph, connection| {
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
            nodes.sort_by(|a, b| {
                (&a.file_path, a.start_line, &a.id).cmp(&(&b.file_path, b.start_line, &b.id))
            });
            let edges = graph
                .canonical_edges
                .iter()
                .flatten()
                .cloned()
                .collect::<Vec<_>>();
            let sources = graph
                .files
                .iter()
                .map(|(path, file)| (path.as_str(), file.source.as_str()))
                .collect::<BTreeMap<_, _>>();
            let deprioritized = connection
                .prepare("SELECT path FROM files WHERE deprioritized=1")?
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<HashSet<_>>>()?;
            let generated = graph
                .files
                .iter()
                .map(|(path, file)| {
                    Ok((
                        path.as_str(),
                        crate::classify::generated_file(path, &file.source)?,
                    ))
                })
                .collect::<Result<BTreeMap<_, _>>>()?;
            let search = |query: &crate::context::ContextQuery| {
                crate::context::query::search(connection, &self.root, query)
            };
            let deprioritized = |path: &str| deprioritized.contains(path);
            let generated = |path: &str| generated.get(path).copied().unwrap_or(false);
            let builder = ContextBuilder::new(
                &nodes,
                &edges,
                &sources,
                &search,
                &deprioritized,
                &generated,
            )?;
            let mut result = ContextResult {
                generation: graph.generation,
                truncated: graph.truncated || graph.resolution_truncated.get(),
                context: builder.build(input, options)?,
                markdown: String::new(),
            };
            while serde_json::to_vec(&result)?.len() > max_bytes {
                result.truncated = true;
                if !result.context.code_blocks.is_empty() {
                    result
                        .context
                        .code_blocks
                        .truncate(result.context.code_blocks.len() / 2);
                } else if !result.context.subgraph.nodes.is_empty() {
                    result
                        .context
                        .subgraph
                        .nodes
                        .truncate(result.context.subgraph.nodes.len() / 2);
                } else {
                    anyhow::bail!("context metadata exceeds max_bytes");
                }
                reconcile(&mut result.context);
            }
            result.markdown = builder.markdown(&result.context);
            if result.markdown.len() > max_bytes {
                const MARKER: &str = "\n\n... (truncated) ...\n";
                let mut end = max_bytes - MARKER.len();
                while !result.markdown.is_char_boundary(end) {
                    end -= 1;
                }
                result.markdown.truncate(end);
                result.markdown.push_str(MARKER);
                result.truncated = true;
            }
            Ok(result)
        })
    }
}

fn reconcile(context: &mut TaskContext) {
    let retained = context
        .subgraph
        .nodes
        .iter()
        .map(|node| node.id.as_str())
        .collect::<HashSet<_>>();
    context.subgraph.edges.retain(|edge| {
        retained.contains(edge.source.as_str()) && retained.contains(edge.target.as_str())
    });
    context
        .subgraph
        .roots
        .retain(|id| retained.contains(id.as_str()));
    context
        .entry_points
        .retain(|node| retained.contains(node.id.as_str()));
    context
        .code_blocks
        .retain(|block| retained.contains(block.node.id.as_str()));
    context.related_files = context
        .subgraph
        .nodes
        .iter()
        .map(|node| node.file_path.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    context
        .related_files
        .sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
    context.stats.node_count = context.subgraph.nodes.len();
    context.stats.edge_count = context.subgraph.edges.len();
    context.stats.file_count = context.related_files.len();
    context.stats.code_block_count = context.code_blocks.len();
    context.stats.total_code_size = context
        .code_blocks
        .iter()
        .map(|block| block.content.encode_utf16().count())
        .sum();
    let names = context
        .entry_points
        .iter()
        .take(3)
        .map(|node| node.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let more = if context.entry_points.len() > 3 {
        format!(" and {} more", context.entry_points.len() - 3)
    } else {
        String::new()
    };
    context.summary = format!(
        "Found {} relevant code symbols across {} files. Key entry points: {names}{more}. {} relationships identified.",
        context.stats.node_count, context.stats.file_count, context.stats.edge_count
    );
}
