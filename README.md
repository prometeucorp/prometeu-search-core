# Prometeu Search

Local code search for coding agents, usable as a standalone command, MCP server,
or linked Rust library. No Node runtime, model, network service, or separately
installed SQLite is required.

## Indexing and search

Native extraction covers 41 language identifiers, including component templates
and custom formats. Framework detection, route synthesis, cross-file resolution,
and canonical ranked search operate on committed SQLite snapshots. Relationships
retain explicit uncertainty; they are not compiler proof. Complete behavioral
parity across arbitrary source programs is not established.

Project settings in `prometeu-search.json` control extension mappings, inclusion,
exclusion, embedded repositories, and ranking preferences. Native Git traversal
needs no Git executable. Each worktree owns a separate cache; reconciliation,
watcher handoff, and content-hash recovery preserve snapshot consistency.

Schema version 6 rejects caches created by versions 1 through 5 without modifying
them. Stop all clients and rebuild the derived index when upgrading.

## Standalone usage

Build with stable Rust and a C compiler for native grammars and bundled SQLite:

```sh
cargo install --path . --locked
prometeu-search --root /path/to/project search 'kind:function session'
prometeu-search --root /path/to/project callers save
prometeu-search --root /path/to/project explore save
prometeu-search --root /path/to/project serve
```

The existing CLI and MCP interfaces expose search, callers/callees, exploration,
impact, paths, implementations, affected tests, and index status. MCP tools are
`code_search`, `code_explore`, and `code_search_status`. Their existing limits
remain 1–50 results, 1024–65536 serialized bytes, and graph depth 1–8.
The richer library APIs below do not impose those transport limits.

## Rust embedding

Add `prometeu-search` as a path dependency, keeping `crates/prometeu-extract`
beside its root manifest. Open an `Engine` with host-selected worktree and cache
paths, then call `sync` or `refresh` before querying.

`Engine::search_with_options` supports candidate offsets and optional byte
budgets; its default count is 100. `Engine::files`, `node`, and `file` inspect
indexed source and metadata. The source APIs in `parser` support all 41
identifiers, explicit extension mappings, and framework-aware extraction.
Pure `graph_query::GraphSnapshot` and `hierarchy` functions accept host-owned
canonical records. Library reads never implicitly refresh live source.

See [architecture](ARCHITECTURE.md), [host integration](docs/contracts/integration.md),
[graph queries](docs/contracts/graph-queries.md), and [project configuration](docs/contracts/project-configuration.md).
Run `cargo fmt --all --check`, `cargo clippy --all-targets --locked -- -D warnings`,
and `cargo test --locked` before committing. Native Windows builds are unsupported.
