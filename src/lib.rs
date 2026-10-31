//! Local, worktree-scoped code search. No model, network, or Node runtime is required.
mod index;
pub mod parser;
mod source;
mod watch;

pub use index::*;
pub use watch::WatchHandle;
