# Architecture

Prometeu Search is a local search engine, independent of any desktop shell,
agent provider, account, or model API.

`src/parser.rs` converts Tree-sitter syntax into symbol ranges and lexical call
references. It uses an iterative traversal and retains a syntax-error flag for
incomplete files. It does not pretend to perform compiler name resolution.
Files with source outside declarations receive a file entry so imports and
top-level calls remain searchable without replacing named symbol callers.

`src/index.rs` owns one canonical project root and SQLite database. Reconciliation
enumerates eligible files, hashes content, parses changed files, and commits the
whole update atomically. Foreign keys and FTS triggers remove stale graph/search
records. Raw call names are retained; query-time matching lets new definitions
resolve old references without reparsing callers. Every resolution is explicitly
`name_match` or `unresolved`.

Search reads source stored with the indexed symbol. It never slices a changed
file using old byte offsets. Each response reads one database snapshot and
includes its generation. Files can still change during or after reconciliation;
the index is not a filesystem snapshot or a compiler authority. Callers must read
live source before applying edits that depend on exact content.

`src/source.rs` pins the project directory for each reconciliation and opens each
source path component relative to directory handles, rejecting symlinks throughout
the path. File type and size checks use the opened handle. A replaced source or
ancestor cannot redirect reads outside the selected tree; a failed open rolls back
reconciliation.

`src/watch.rs` uses native notifications with a 250 ms quiet period and a 2 s
maximum burst delay. A filesystem lock elects one watcher per database. Followers
retry acquisition and reconcile after takeover. All clients serialize updates
with SQLite transactions, a busy timeout, and WAL. Notifications are an
optimization: CLI searches and MCP queries reconcile independently so missed
events do not silently leave an indefinitely stale index.

`src/main.rs` provides the CLI. `src/mcp.rs` provides newline-delimited JSON-RPC
over stdio, with a fixed worktree root, bounded requests and strict tool inputs.
MCP stdout carries only protocol messages. Closing stdin stops the process and
joins its watcher. The library exposes `Engine` for direct Rust embedding.

There is no daemon discovery, TCP listener, authentication system, async runtime,
plugin framework, or provider-specific adapter. The embedding host owns process
lifecycle. Separate agent processes can share the same worktree database and
watcher election without sharing writable indexes across worktrees.

Schema version 2 stores root ownership, generation, reconciliation time, files,
symbols, raw references, and an FTS5 index. The format is derived cache data,
not a public interchange format. Increment the version when extraction or
storage changes require rebuilding unchanged files. Unknown versions are rejected
instead of silently destroying or migrating another engine's data.
Version 1 caches are rejected because unchanged files need extraction again
to include their previously omitted file-level source and calls. Stop all
clients and choose a fresh cache path or remove the derived index before retrying.

Native Tree-sitter grammars and bundled SQLite keep the executable independent
of external runtimes.
