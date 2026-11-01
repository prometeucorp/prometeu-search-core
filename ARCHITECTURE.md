# Architecture

Prometeu Search is a local search engine, independent of any desktop shell,
agent provider, account, or model API.

The [knowledge bundle](docs/README.md) indexes detailed guidance; the
[integration contract](docs/contracts/integration.md) defines executable, MCP,
distribution, and host lifecycle responsibilities.

`src/parser.rs` extracts symbols and call references with Tree-sitter. The
semantic extraction pass retains qualified names, enclosing scopes, exports,
imports, aliases, receiver evidence, and inheritance relationships. Traversal
retains a syntax-error flag for incomplete files. Files with source outside
named declarations receive a file entry. The parser does not run a compiler.

`src/index.rs` owns one canonical project root, SQLite schema, and lexical
search. `src/sync.rs` reconciles source atomically. Full recovery enumerates and
hashes eligible files; `sync_paths` prunes traversal to changed paths and their
ancestors. Both parse only content changes. Foreign keys and FTS triggers remove
stale extraction and search records together. Source access stays under the
pinned project directory.

`src/graph.rs` resolves the stored extraction metadata and provides exploration,
impact, paths, implementation hierarchies, and affected test candidates. Import,
scope, and receiver evidence precedes uncertain name matching. Relationship
responses retain resolution method, confidence, and ambiguity. Analysis has
explicit work and response limits. Graph metadata is cached per connection and
index generation; source is read from the same SQLite snapshot as the graph.
A generation change invalidates the cached graph, including when another client
commits changes. The graph remains heuristic, not compiler authority.

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
maximum burst delay. It retains changed paths; overflow, watcher errors,
directory topology, and ignore-rule changes require full reconciliation.
A filesystem lock elects one watcher per database. Followers retry acquisition
and reconcile after takeover. Updates serialize with SQLite transactions, a
busy timeout, and WAL.

CLI searches and MCP queries call `refresh` independently of watcher delivery.
The first refresh on each connection hashes all files. Later refreshes enumerate
metadata and hash only changed signatures, with full content recovery at least
every 60 seconds while queries or watching remain active. Signatures include
size and modification time, plus device, inode, and change time on Unix.
A file changed during a read gets an invalid signature so the next refresh reads
it again. Metadata-preserving changes can wait until full recovery. Explicit
`sync` always hashes every eligible file. Failed updates roll back atomically.

`src/main.rs` provides the CLI. `src/mcp.rs` provides newline-delimited JSON-RPC
over stdio, with a fixed worktree root, bounded requests and strict tool inputs.
MCP stdout carries only protocol messages. Closing stdin stops the process and
joins its watcher. The library exposes `Engine` for direct Rust embedding.

There is no daemon discovery, TCP listener, authentication system, async runtime,
plugin framework, or provider-specific adapter. The embedding host owns process
lifecycle. Separate agent processes can share the same worktree database and
watcher election without sharing writable indexes across worktrees.

Schema version 4 stores root ownership, generation, reconciliation time, files,
file signatures, serialized extraction metadata, symbol ordinals and source,
raw references, and an FTS5 index. The format is derived cache data, not a public
interchange format. Increment the version when extraction or storage changes
require rebuilding unchanged files. Unknown versions are rejected instead of
silently destroying or migrating another engine's data. Versions 1, 2, and 3 are
rejected without modification; version 3 extraction can retain incorrect loop
binding scopes in unchanged files. Stop clients and use a fresh cache or
deliberately remove the derived index before retrying.

Native Tree-sitter grammars and bundled SQLite keep the executable independent
of external runtimes.

`src/graph_query.rs` and `src/hierarchy.rs` operate on caller-owned canonical
records from `prometeu-extract`. They are pure library boundaries: the current
SQLite engine, executable, and MCP tools retain their existing contracts.
