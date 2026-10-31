//! Local, worktree-scoped code search. No model, network, or Node runtime is required.
mod graph;
mod index;
pub mod mcp;
pub mod parser;
mod source;
mod sync;
mod watch;

pub use graph::{
    ContextFile, ContextSymbol, GraphEdge, GraphMode, GraphOptions, GraphResult, Overview,
    OverviewEdge, OverviewFile,
};
pub use index::*;
pub use watch::WatchHandle;
