//! Local, worktree-scoped code search. No model, network, or Node runtime is required.
mod auxiliary_sources;
mod classify;
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

pub use graph::{ContextFile, ContextSymbol, GraphEdge, GraphMode, GraphOptions, GraphResult};
pub use index::*;
pub use inspect::*;
pub use watch::WatchHandle;
