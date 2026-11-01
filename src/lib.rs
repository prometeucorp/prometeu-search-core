//! Local, worktree-scoped code search. No model, network, or Node runtime is required.
mod graph;
pub mod graph_query;
pub mod hierarchy;
mod index;
pub mod parser;
mod source;
mod sync;
mod watch;

pub use graph::{ContextFile, ContextSymbol, GraphEdge, GraphMode, GraphOptions, GraphResult};
pub use index::*;
pub use watch::WatchHandle;
