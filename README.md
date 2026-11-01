# Prometeu Search

Local code search for coding agents. Run it as a standalone command or MCP server,
or bundle the same executable with Prometeu. No Node runtime, model, API key,
embedding service, or separately installed SQLite is required.

## What works

- Tree-sitter parsing of TypeScript, TSX, JavaScript/JSX, Rust, and Ruby.
- SQLite FTS5 search over names, source, and paths, with source snippets and locations.
- File entries for imports and top-level source outside named declarations.
- Import-, scope-, and receiver-aware candidate relationships, with explicit uncertainty.
- Bounded exploration that assembles source and graph context in one request.
- Multi-hop impact, symbol paths, implementation hierarchies, and affected test candidates.
- A separate persistent index for each canonical worktree directory.
- Content-hash recovery and targeted file updates; deleted and renamed files disappear from the graph.
- Automatic background updates while the MCP server runs, including watcher handoff between sessions.
- Reconciliation before CLI searches and MCP tool queries; repeated queries skip hashing unchanged file metadata.
- Bounded JSON results and a three-tool MCP surface.
- A Rust library for hosts that prefer linking instead of running a subprocess.

Relationships use static import, scope, and receiver evidence, with name matching
as an uncertain fallback. They are not compiler proof and can contain multiple
candidates. Search is lexical, not semantic embedding search.
No token savings are claimed without comparative agent evaluations.

Ruby support includes `.rb`, `.rake`, `.gemspec`, `.ru`, `Gemfile`, `Rakefile`, and
`Guardfile`. Classes, modules, instance methods, singleton methods, and call
references are indexed. Dynamic dispatch and metaprogramming remain unresolved;
embedded Ruby templates (`.erb`) are not parsed.

## Standalone usage

Build with the current stable Rust toolchain and a C compiler for the bundled
Tree-sitter grammars and SQLite. End users of packaged binaries need neither.

```sh
cargo install --path . --locked
prometeu-search --root /path/to/project search 'session launch'
prometeu-search --root /path/to/project callers save
prometeu-search --root /path/to/project callees start
prometeu-search --root /path/to/project explore 'session launch'
prometeu-search --root /path/to/project impact save --depth 3
prometeu-search --root /path/to/project path start save --depth 4
prometeu-search --root /path/to/project implementations Store
prometeu-search --root /path/to/project affected src/store.ts --depth 5
prometeu-search --root /path/to/project status
```

The first search creates the index automatically. `index` explicitly reconciles
it, and `status` reads its last committed state. Commands write compact JSON to
stdout and diagnostics to stderr.

Use `--limit 5 --max-bytes 4096` to reduce returned context. `max_bytes` bounds the
complete serialized result, including escaped source and metadata. Limits are
1–50 results and 1024–65536 bytes; defaults are 10 and 12000. A result's
`snippet_truncated` and the response's `truncated` distinguish clipped source
from omitted matches. `start_line` and `end_line` describe the complete symbol.

Prebuilt binaries are produced as artifacts by manually running the **Binaries**
GitHub Actions workflow for macOS Apple Silicon/Intel and Linux x86_64/ARM64.
Native Windows builds are not supported.
They contain the executable, this README, and license notices. GNU/Linux builds
use their documented runner's glibc baseline; they are not fully static binaries.
There is no release or package-registry publication yet.

## MCP

Configure any stdio MCP host to run:

```json
{
  "mcpServers": {
    "prometeu-search": {
      "command": "/absolute/path/to/prometeu-search",
      "args": ["--root", "/absolute/path/to/project", "serve"]
    }
  }
}
```

`code_search` accepts `query`, optional `mode` (`search`, `callers`, `callees`),
`limit`, and `max_bytes`. `code_explore` accepts a query and optional mode
(`explore`, `impact`, `path`, `implementations`, or `affected`), worktree-relative
`path`, positive `line` (requires `path`), `depth` (1–8, default 2), `limit`, and
`max_bytes`. Path mode requires `target`; other modes reject it. The CLI exposes
the same graph modes as commands, with `--path` and `--line` for disambiguation.
See [graph queries](docs/contracts/graph-queries.md) for result semantics and limits.

`code_search_status` takes no arguments and reconciles
before reporting index counts. The root is fixed by the host; tools cannot
switch to another directory. MCP transport framing adds bytes beyond the result
budget. A result is a source snapshot, not proof that a file cannot change again.

`serve` watches files until stdin closes. One process owns the watcher per
database; other sessions take over when that owner exits. `serve --no-watch`
disables background work but still reconciles before queries. Watcher failures
go to stderr; explicit reconciliation remains available.

Prometeu will supply this configuration automatically when integrating the
engine. Its app-side lifecycle and installer wiring are separate from this
repository; see [the bundling contract](docs/contracts/integration.md).

## Index location and exclusions

By default the database is under the OS cache directory at
`prometeu-search/<hash-of-canonical-root>/index.sqlite`. The project is not
modified. `--database /path/to/index.sqlite` lets an embedding host own storage.
Each database records its root and refuses a different worktree. Unsupported
schema versions and unrelated SQLite databases are rejected without adopting
them. Stop all clients before removing a derived index to rebuild it.
Schema version 4 requires a fresh index when upgrading from versions 1, 2, or 3.
Unchanged files must be extracted again to populate scopes, imports, receivers,
and inheritance metadata, including corrected loop binding scopes. Old databases
are rejected without modification.

The scanner honors `.gitignore`, `.ignore`, and `.prometeu-searchignore`, including
nested rules. It also excludes `.git`, dependency/build/cache directories,
symlinks, non-UTF-8 files, unsupported languages, and files larger than 1 MiB.
Source reads reject symlinks at every path component through opened directory
handles, including when paths change after enumeration.
The hard directory exclusions are listed in `src/index.rs`. Generated code in
other locations must be excluded by an ignore file.

Source snippets are stored locally in the cache. The engine makes no network
requests. An agent host may send returned source to its model provider under
that host's settings. The final cache directory created by the engine uses mode
`0700` on Unix; its database and lock files use mode `0600`.

## Development

Start at the [documentation map](docs/README.md) for contracts and decisions.
Documentation-only changes can run `cargo test --locked --test docs`.

```sh
cargo fmt --all --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
cargo build --release --locked
```

Tests exercise parser offsets, incremental graph changes, ignore rules, worktree
isolation, concurrent clients, byte budgets, native watcher handoff, and real
stdio MCP subprocesses. CI runs on Linux and macOS for pull requests and pushes
to `main`. Formatting is checked once on Linux; Clippy and tests run on both
platforms. Binary builds run only on manual dispatch.

Current limits: TypeScript/JavaScript/Rust/Ruby only; heuristic static resolution,
no compiler authority, cross-language bridges, framework routing, embeddings, or
graph UI. Watcher updates retain changed paths and skip unrelated subtrees. Query
refresh still enumerates file metadata; unchanged source is not read or hashed.
The first refresh per connection, explicit `index`, and recovery checks at least
every 60 seconds hash all eligible files. Only content changes are parsed again.
See [architecture](ARCHITECTURE.md) and [third-party notices](THIRD_PARTY_NOTICES.md).

## Native extraction library

The workspace includes `prometeu-extract`, a reusable Rust library for native
language extraction without a Node runtime. Its Tree-sitter walkers return
canonical records and run against independent reference fixtures; the current
SQLite engine keeps its existing language selection and query contracts.

## Canonical graph algorithms for Rust hosts

`graph_query::GraphSnapshot` accepts host-owned canonical nodes and edges and
provides traversal, paths, impact, call sites, dependencies, and symbol context.
The `hierarchy` module computes declared ancestry, subtype fans, and member
overrides from the same record types. These pure APIs do not open an index or
read source files; their independent oracle tests run offline.
