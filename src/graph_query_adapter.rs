//! Public graph operations over a single committed generation.
use super::with_graph;
use crate::Engine;
use crate::graph_query::{GraphSnapshot, TraversalOptions};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GraphOperation {
    #[default]
    Bfs,
    Dfs,
    Callers,
    Callees,
    CallSites,
    CallGraph,
    TypeGraph,
    Usages,
    Ancestors,
    Children,
    Impact,
    Path,
    SymbolContext,
    Dependencies,
    Dependents,
    Exports,
    Modules,
    Qualified,
    Filtered,
}

#[derive(Debug, Clone, Serialize)]
pub struct GraphQueryOptions {
    pub operation: GraphOperation,
    pub id: Option<i64>,
    pub target: Option<i64>,
    pub path: Option<String>,
    pub pattern: Option<String>,
    pub traversal: TraversalOptions,
    pub max_edges: usize,
    pub max_bytes: usize,
    pub include_edges: bool,
}

impl Default for GraphQueryOptions {
    fn default() -> Self {
        Self::for_operation(GraphOperation::Bfs)
    }
}

impl GraphQueryOptions {
    /// Use the operation's native defaults before overriding individual budgets.
    pub fn for_operation(operation: GraphOperation) -> Self {
        use GraphOperation::*;
        Self {
            operation,
            id: None,
            target: None,
            path: None,
            pattern: None,
            traversal: TraversalOptions {
                max_depth: match operation {
                    Callers | Callees => 1,
                    CallGraph => 2,
                    Impact => 3,
                    _ => usize::MAX,
                },
                limit: match operation {
                    Bfs | Dfs => 1000,
                    Impact => 10000,
                    _ => usize::MAX,
                },
                ..Default::default()
            },
            max_edges: 50000,
            max_bytes: usize::MAX,
            include_edges: true,
        }
    }
}

impl<'de> Deserialize<'de> for GraphQueryOptions {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Default, Deserialize)]
        #[serde(default, deny_unknown_fields)]
        struct Fields {
            operation: GraphOperation,
            id: Option<i64>,
            target: Option<i64>,
            path: Option<String>,
            pattern: Option<String>,
            traversal: Option<serde_json::Map<String, Value>>,
            max_edges: Option<usize>,
            max_bytes: Option<usize>,
            include_edges: Option<bool>,
        }
        let fields = Fields::deserialize(deserializer)?;
        let mut options = Self::for_operation(fields.operation);
        options.id = fields.id;
        options.target = fields.target;
        options.path = fields.path;
        options.pattern = fields.pattern;
        if let Some(traversal) = fields.traversal {
            let mut defaults =
                serde_json::to_value(&options.traversal).map_err(serde::de::Error::custom)?;
            defaults.as_object_mut().unwrap().extend(traversal);
            options.traversal =
                serde_json::from_value(defaults).map_err(serde::de::Error::custom)?;
        }
        if let Some(max_edges) = fields.max_edges {
            options.max_edges = max_edges;
        }
        if let Some(max_bytes) = fields.max_bytes {
            options.max_bytes = max_bytes;
        }
        if let Some(include_edges) = fields.include_edges {
            options.include_edges = include_edges;
        }
        Ok(options)
    }
}

#[derive(Debug, Serialize)]
pub struct GraphQueryResult {
    pub generation: u64,
    pub operation: GraphOperation,
    pub data: Value,
    pub ambiguous: bool,
    pub truncated: bool,
}

impl GraphQueryOptions {
    pub fn validate(&self) -> Result<()> {
        let options = self;
        use GraphOperation::*;
        crate::index::validate_limits(1, options.max_bytes)?;
        options.traversal.validate()?;
        let node_query = !matches!(
            options.operation,
            Dependencies | Dependents | Exports | Modules | Qualified | Filtered
        );
        ensure!(
            node_query == options.id.is_some(),
            "id is required only for symbol graph operations"
        );
        ensure!(
            options.id.is_none_or(|id| id > 0) && options.target.is_none_or(|id| id > 0),
            "id and target must be positive"
        );
        ensure!(
            (options.operation == Path) == options.target.is_some(),
            "target is required only for path queries"
        );
        let file_query = matches!(options.operation, Dependencies | Dependents | Exports);
        ensure!(
            file_query == options.path.is_some(),
            "path is required only for file graph operations"
        );
        if let Some(path) = &options.path {
            crate::inspect::indexed_path(path)?;
        }
        ensure!(
            (options.operation == Qualified) == options.pattern.is_some(),
            "pattern is required only for qualified-name queries"
        );
        ensure!(
            options
                .pattern
                .as_ref()
                .is_none_or(|pattern| pattern.len() <= 1024),
            "pattern exceeds 1024 bytes"
        );
        Ok(())
    }
}

impl Engine {
    /// Query canonical graph records without reading live files or refreshing.
    pub fn graph_query(&self, options: &GraphQueryOptions) -> Result<GraphQueryResult> {
        use GraphOperation::*;
        options.validate()?;
        let file_path = options
            .path
            .as_deref()
            .map(crate::inspect::indexed_path)
            .transpose()?;
        with_graph(self, |graph, _| {
            let canonical_id = |id| -> Result<String> {
                let &index = graph
                    .ids
                    .get(&id)
                    .context("symbol ID is not present in this indexed snapshot")?;
                Ok(graph
                    .metadata(index)
                    .context("symbol has no canonical extraction record")?
                    .id
                    .clone())
            };
            let id = options
                .id
                .map(canonical_id)
                .transpose()?
                .unwrap_or_default();
            let target = options
                .target
                .map(canonical_id)
                .transpose()?
                .unwrap_or_default();
            let mut nodes = graph
                .files
                .values()
                .filter_map(|f| f.analysis.extraction.as_ref())
                .flat_map(|e| e.nodes.iter().cloned())
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
            let snapshot = GraphSnapshot::new(&nodes, &edges)?;
            let path = file_path.as_deref().unwrap_or_default();
            let incoming = options.traversal.direction == crate::graph_query::Direction::Incoming;
            let data = match options.operation {
                Bfs => serde_json::to_value(snapshot.breadth_first(&id, &options.traversal)?)?,
                Dfs => serde_json::to_value(snapshot.depth_first(&id, &options.traversal)?)?,
                Callers => {
                    serde_json::to_value(snapshot.calls(&id, true, options.traversal.max_depth)?)?
                }
                Callees => serde_json::to_value(snapshot.calls(
                    &id,
                    false,
                    options.traversal.max_depth,
                )?)?,
                CallSites => serde_json::to_value(snapshot.call_sites(&id, incoming))?,
                CallGraph => {
                    serde_json::to_value(snapshot.call_graph(&id, options.traversal.max_depth)?)?
                }
                TypeGraph => serde_json::to_value(snapshot.type_graph(&id))?,
                Usages => serde_json::to_value(snapshot.usages(&id))?,
                Ancestors => serde_json::to_value(snapshot.ancestors(&id))?,
                Children => serde_json::to_value(snapshot.children(&id))?,
                Impact => serde_json::to_value(snapshot.impact(
                    &id,
                    options.traversal.max_depth,
                    options.traversal.limit,
                    options.max_edges,
                )?)?,
                Path => serde_json::to_value(snapshot.find_path(
                    &id,
                    &target,
                    &options.traversal.edge_kinds,
                ))?,
                SymbolContext => serde_json::to_value(snapshot.symbol_context(&id))?,
                Dependencies | Dependents => {
                    let mut paths =
                        snapshot.file_dependencies(path, options.operation == Dependents);
                    paths.sort();
                    json!(paths)
                }
                Exports => json!(snapshot.exports(path)),
                Modules => json!(snapshot.module_structure()),
                Qualified => {
                    json!(snapshot.qualified_names(options.pattern.as_deref().unwrap_or_default())?)
                }
                Filtered => json!(snapshot.filtered(
                    |node| options.traversal.node_kinds.is_empty()
                        || options.traversal.node_kinds.contains(&node.kind),
                    options.include_edges
                )),
            };
            // A bounded response never silently presents an omitted branch as absent.
            let mut result = GraphQueryResult {
                generation: graph.generation,
                operation: options.operation,
                truncated: graph.truncated
                    || graph.resolution_truncated.get()
                    || data.get("truncated") == Some(&Value::Bool(true)),
                ambiguous: graph
                    .edges
                    .iter()
                    .any(|e| e.ambiguous || e.confidence == "low"),
                data,
            };
            let graph_shape = matches!(
                options.operation,
                Bfs | Dfs | CallGraph | TypeGraph | Impact | Filtered
            );
            if options.operation == Modules {
                let object = result.data.as_object_mut().unwrap();
                if object.len() > options.traversal.limit {
                    retain_module_count(object, options.traversal.limit);
                    result.truncated = true;
                }
                for files in object.values_mut() {
                    bound_collections(files, options.traversal.limit, &mut result.truncated, false);
                }
            } else if options.operation == Path
                && result
                    .data
                    .as_array()
                    .is_some_and(|rows| rows.len() > options.traversal.limit)
            {
                result.data = Value::Null;
                result.truncated = true;
            } else {
                bound_collections(
                    &mut result.data,
                    options.traversal.limit,
                    &mut result.truncated,
                    graph_shape,
                );
            }
            while options.max_bytes != usize::MAX
                && serde_json::to_vec(&result)?.len() > options.max_bytes
            {
                result.truncated = true;
                let reduced = if options.operation == Modules {
                    shrink_modules(&mut result.data)
                } else {
                    options.operation != Path && shrink(&mut result.data, graph_shape)
                };
                if !reduced {
                    result.data = Value::Null;
                }
            }
            Ok(result)
        })
    }
}

fn bound_collections(data: &mut Value, limit: usize, truncated: &mut bool, graph_shape: bool) {
    match data {
        Value::Array(rows) => {
            if rows.len() > limit {
                *truncated = true;
                rows.truncate(limit);
            }
        }
        Value::Object(object) => {
            let nodes_trimmed = graph_shape
                && object
                    .get("nodes")
                    .and_then(Value::as_array)
                    .is_some_and(|nodes| nodes.len() > limit);
            for (key, value) in object.iter_mut() {
                if (graph_shape && key == "nodes") || (!graph_shape && key != "focal") {
                    bound_collections(value, limit, truncated, false);
                }
            }
            if nodes_trimmed {
                prune_graph(object);
            }
        }
        _ => {}
    }
}

fn prune_graph(object: &mut serde_json::Map<String, Value>) {
    let Some(nodes) = object.get("nodes").and_then(Value::as_array) else {
        return;
    };
    let ids: HashSet<_> = nodes
        .iter()
        .filter_map(|n| n["id"].as_str().map(str::to_owned))
        .collect();
    if let Some(edges) = object.get_mut("edges").and_then(Value::as_array_mut) {
        edges.retain(|e| {
            e["source"].as_str().is_some_and(|id| ids.contains(id))
                && e["target"].as_str().is_some_and(|id| ids.contains(id))
        });
    }
    if let Some(roots) = object.get_mut("roots").and_then(Value::as_array_mut) {
        roots.retain(|v| v.as_str().is_some_and(|id| ids.contains(id)));
    }
}

fn shrink(data: &mut Value, graph_shape: bool) -> bool {
    match data {
        Value::Array(rows) if !rows.is_empty() => {
            rows.truncate(rows.len() / 2);
            true
        }
        Value::Object(object) => {
            let key = object
                .iter()
                .filter(|(_, v)| v.as_array().is_some_and(|rows| !rows.is_empty()))
                .max_by_key(|(_, v)| serde_json::to_vec(v).map_or(0, |v| v.len()))
                .map(|(k, _)| k.clone());
            if let Some(key) = key {
                shrink(object.get_mut(&key).unwrap(), false);
                if graph_shape && key == "nodes" {
                    prune_graph(object);
                }
                true
            } else {
                false
            }
        }
        _ => false,
    }
}

fn retain_module_count(object: &mut serde_json::Map<String, Value>, limit: usize) {
    let mut retained = 0;
    object.retain(|_, _| {
        retained += 1;
        retained <= limit
    });
}

fn shrink_modules(data: &mut Value) -> bool {
    let Some(object) = data.as_object_mut().filter(|object| !object.is_empty()) else {
        return false;
    };
    if object.len() > 1 {
        retain_module_count(object, object.len() / 2);
    } else if !shrink(object.values_mut().next().unwrap(), false) {
        object.clear();
    }
    true
}
