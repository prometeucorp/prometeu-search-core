# Prometeu Search

Local code search for coding agents. Run it as a standalone command or MCP server,
or bundle the same executable with Prometeu. No Node runtime, model, API key,
embedding service, or separately installed SQLite is required.

## What works

- Native extraction of TypeScript/TSX, JavaScript/JSX, Python, Go, Rust, Java,
  C/C++, C#, PHP, Ruby, Swift, Kotlin, Scala, Dart, Lua/Luau, R, ArkTS,
  Objective-C, Solidity, Nix, Erlang, Terraform/OpenTofu, COBOL, VB.NET, and Pascal/Delphi.
- Vue, Svelte, Astro, Liquid, MyBatis/iBatis XML, Razor, CFML, CFScript, and
  CFQuery extraction. YAML, Twig, and properties retain file-level extraction.
  Play route files additionally declare routes and handler references in detected
  Play projects. All 41 upstream language identifiers have extractors.
- SQLite FTS5 search over canonical names, qualified names, documentation, and signatures, with field filters, typo fallback, source snippets, and locations.
- File entries for imports and top-level source outside named declarations.
- Import-, scope-, and receiver-aware candidate relationships, with explicit uncertainty.
- Bounded exploration and ranked task context with source blocks and Markdown output.
- Multi-hop impact, symbol paths, implementation hierarchies, and affected test candidates.
- Resolved-edge metrics, circular import groups, and conservative unreferenced-symbol reports.
- A separate persistent index for each canonical worktree directory.
- Content-hash recovery and targeted file updates; deleted and renamed files disappear from the graph.
- Automatic background updates while the MCP server runs, including watcher handoff between sessions.
- Reconciliation before CLI searches and MCP tool queries; repeated queries skip hashing unchanged file metadata.
- Bounded JSON results and nine MCP tools, including canonical graph queries, task context, type hierarchies, and source/metadata reads.
- An embedded browser viewer with graph exploration, source inspection, routes, and saved trails.
- A Rust library for hosts that prefer linking instead of running a subprocess.

Relationships use static import, scope, and receiver evidence, with name matching
as an uncertain fallback. They are not compiler proof and can contain multiple
candidates. Search is lexical, not semantic embedding search. See the
[search contract](docs/contracts/lexical-search.md) for filters and ranking.
No token savings are claimed without comparative agent evaluations.

The upstream extraction port is verified against independent reference outputs.
All registered language extractors, framework detectors, synthesis passes, and
canonical graph operations are implemented. Complete behavioral equivalence
across arbitrary projects is not yet established.
See the [parity evidence and remaining work](docs/quality/parity.md).

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
prometeu-search --root /path/to/project search 'kind:function lang:rust path:src session'
prometeu-search --root /path/to/project callers save
prometeu-search --root /path/to/project callees start
prometeu-search --root /path/to/project explore 'session launch'
prometeu-search --root /path/to/project context 'session launch' --format markdown
prometeu-search --root /path/to/project impact save --depth 3
prometeu-search --root /path/to/project path start save --depth 4
prometeu-search --root /path/to/project implementations Store
prometeu-search --root /path/to/project affected src/store.ts --depth 5
prometeu-search --root /path/to/project files '.py'
prometeu-search --root /path/to/project file src/store.ts --start-line 10 --end-line 30
prometeu-search --root /path/to/project node 42
prometeu-search --root /path/to/project metrics 42
prometeu-search --root /path/to/project hierarchy 42
prometeu-search --root /path/to/project cycles
prometeu-search --root /path/to/project deadcode
prometeu-search --root /path/to/project status
```

The first search creates the index automatically. `index` explicitly reconciles
it, and `status` reads its last committed state. Commands write compact JSON to
stdout and diagnostics to stderr. `context --format markdown` selects Markdown
instead of JSON for assembled task context.

Use `--limit 5 --max-bytes 4096` to reduce returned context. `max_bytes` bounds the
complete serialized result, including escaped source and metadata. CLI search
defaults to 10 results and supports `--offset`; counts are configurable and byte
budgets are optional, with a minimum of 1024 bytes and no fixed upper ceiling. MCP
search caps each response at 100 results and applies its own character budget. A result's
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
`path`, positive `line` (requires `path`), positive `depth` (default 2), `limit`, and
`max_bytes`. Path mode requires `target`; other modes reject it. The CLI exposes
the same graph modes as commands, with `--path` and `--line` for disambiguation.
See [graph queries](docs/contracts/graph-queries.md) for result semantics and limits.

`code_hierarchy` returns ancestry, subtype fans, and name-based member override
matches for a search-result ID. See the [type hierarchy contract](docs/contracts/type-hierarchy.md).

`code_graph` exposes canonical BFS/DFS traversal, all call sites, usages, impact,
paths, symbol context, file dependencies, exports, and module structure. Rust
hosts can use `Engine::graph_query` or the pure `graph_query::GraphSnapshot`.
The CLI provides `graph <operation>` with the same selectors and bounds.

Run `prometeu-search --root /path/to/project ui --port 3333` for the bundled
[graph viewer](docs/contracts/graph-viewer.md). Its Rust server listens only on
loopback; browser assets ship inside the binary and require no Node runtime.

`code_context` ranks task-relevant entry points and expands their relationships,
source blocks, and related files. It accepts `query`, `max_bytes`, and an optional
`options` object. See [task context](docs/contracts/task-context.md) for defaults,
snapshot behavior, formatting, and limits.

`code_search_status` takes no arguments and reconciles
before reporting index counts. The root is fixed by the host; tools cannot
switch to another directory. MCP transport framing adds bytes beyond the result
budget. A result is a source snapshot, not proof that a file cannot change again.

`code_files` lists indexed files with an optional path substring. `code_node`
reads a search-result ID with its source and extraction metadata. `code_file`
reads inclusive lines from a worktree-relative path. These tools use the same
byte budgets and indexed snapshots; see [source inspection](docs/contracts/source-inspection.md).

`serve` watches files until stdin closes. One process owns the watcher per
database; other sessions take over when that owner exits. `serve --no-watch`
disables background work but still reconciles before queries. Watcher failures
go to stderr; explicit reconciliation remains available.

Prometeu will supply this configuration automatically when integrating the
engine. Its app-side lifecycle and installer wiring are separate from this
repository; see [the bundling contract](docs/contracts/integration.md).

## Embed in another Rust project

Add the engine as a path dependency; keep its `crates/prometeu-extract` directory
beside the root manifest:

```toml
[dependencies]
prometeu-search = { path = "../prometeu-search-core" }
```

The host chooses the worktree and cache locations, then calls the same engine
used by the CLI and MCP server:

```rust
use prometeu_search::{Engine, GraphOperation, GraphQueryOptions};
use std::path::Path;

let mut engine = Engine::open(
    Path::new("/path/to/project"),
    Some(Path::new("/path/to/cache/index.sqlite")),
)?;
engine.sync()?;
let search = engine.search("session launch", 10, 12_000)?;
let modules = engine.graph_query(&GraphQueryOptions::for_operation(GraphOperation::Modules))?;
```

No subprocess or HTTP listener is needed. SQLite is bundled. Watching and the
browser viewer are optional APIs controlled by the embedding host.

## Index location and exclusions

By default the database is under the OS cache directory at
`prometeu-search/<hash-of-canonical-root>/index.sqlite`. The project is not
modified. `--database /path/to/index.sqlite` lets an embedding host own storage.
Each database records its root and refuses a different worktree. Unsupported
schema versions and unrelated SQLite databases are rejected without adopting
them. Stop all clients before removing a derived index to rebuild it.
Schema version 6 requires a fresh index when upgrading from versions 1 through 5.
Unchanged files must be extracted again to populate canonical search records
and full-file source. Old databases are rejected without modification.

Project settings in `prometeu-search.json` support extension mappings, explicit
inclusion/exclusion, embedded repository discovery, and ranking preferences.
Configuration changes atomically reconcile eligibility and reparse unchanged
files. See [project configuration](docs/contracts/project-configuration.md).

The scanner reads Git visibility natively, including split and sparse indexes,
submodules, and embedded repositories; Git is not required at runtime. Root
ignore rules and default exclusions apply to tracked files, while nested ignore
rules can leave tracked files visible. `.ignore` and `.prometeu-searchignore`
remain supported. The catalog in `src/traversal/defaults.rs` excludes dependency,
output, and tool-cache directories, with the reference's source-build exceptions
and root ignore negations.

Symlinks, non-UTF-8 files, unsupported languages, and files larger than 1 MiB stay
out of the source index. Shopify JSON templates and section groups are indexed
as Liquid only under a theme marked by `layout/theme.liquid` or
`config/settings_schema.json`. Adding or removing a theme marker triggers full
eligibility reconciliation. Source reads reject symlinks at every path component
through opened directory handles, including when paths change after enumeration.
Generated code outside default exclusions can be excluded by an ignore file.

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

Current limits: heuristic static resolution, no compiler authority or embeddings,
and no exhaustive proof of cross-language/framework behavior. Extraction covers all 41
language identifiers; the [parity record](docs/quality/parity.md) distinguishes
that evidence from complete feature parity. Native recovery of malformed source
is not claimed equivalent to the reference parser's UTF-16 recovery. Watcher updates retain changed
paths and skip unrelated subtrees. Query
refresh still enumerates file metadata; unchanged source is not read or hashed.
The first refresh per connection, explicit `index`, and recovery checks at least
every 60 seconds hash all eligible files. Only content changes are parsed again.
See [architecture](ARCHITECTURE.md) and [third-party notices](THIRD_PARTY_NOTICES.md).
