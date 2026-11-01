//! Local, worktree-scoped code search. No model, network, or Node runtime is required.
mod auxiliary_sources;
mod classify;
pub mod context;
mod frameworks;
mod graph;
pub mod graph_query;
pub mod hierarchy;
mod index;
mod inspect;
pub mod parser;
mod project_config;
pub mod search;
mod source;
mod sync;
mod synthesis;
mod traversal;
mod watch;

pub use graph::viewer;
pub use graph::{
    ContextFile, ContextSymbol, CyclesResult, DeadCodeEntry, DeadCodeExclusions, DeadCodeOptions,
    DeadCodeReport, DeadCodeResult, GraphEdge, GraphMode, GraphOptions, GraphResult, MetricsResult,
    NodeMetrics,
};
pub use graph::{ContextResult, HierarchyResult};
pub use graph::{GraphOperation, GraphQueryOptions, GraphQueryResult};
pub use index::*;
pub use inspect::*;
pub use watch::WatchHandle;
