# Prometeu Search

Local code search for coding agents. Run it as a standalone command or MCP server,
or bundle the same executable with Prometeu. No Node runtime, model, API key,
embedding service, or separately installed SQLite is required.

## What works

- Tree-sitter parsing of TypeScript, TSX, JavaScript/JSX, Rust, and Ruby.
- SQLite FTS5 search over names, source, and paths, with source snippets and locations.
- Candidate callers and callees, including unresolved and ambiguous names.
- A separate persistent index for each canonical worktree directory.
- Content-hash reconciliation: only changed files are parsed again; deleted and renamed files disappear from the graph.
- Automatic background updates while the MCP server runs, including watcher handoff between sessions.
- Automatic reconciliation before CLI searches and MCP tool queries.
- Bounded JSON results and a two-tool MCP surface.
- A Rust library for hosts that prefer linking instead of running a subprocess.

Call relationships use name matching, not compiler type resolution. Results can
contain multiple candidates. Search is lexical, not semantic embedding search.
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

Prebuilt binaries are produced as artifacts by the **Binaries** GitHub Actions
workflow for macOS Apple Silicon/Intel, Linux x86_64/ARM64, and Windows x86_64.
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
`limit`, and `max_bytes`. `code_search_status` takes no arguments and reconciles
before reporting index counts. The root is fixed by the host; tools cannot
switch to another directory. MCP transport framing adds bytes beyond the result
budget. A result is a source snapshot, not proof that a file cannot change again.

`serve` watches files until stdin closes. One process owns the watcher per
database; other sessions take over when that owner exits. `serve --no-watch`
disables background work but still reconciles before queries. Watcher failures
go to stderr; explicit reconciliation remains available.

Prometeu will supply this configuration automatically when integrating the
engine. Its app-side lifecycle and installer wiring are separate from this
repository; see [the bundling contract](docs/integration.md).

## Index location and exclusions

By default the database is under the OS cache directory at
`prometeu-search/<hash-of-canonical-root>/index.sqlite`. The project is not
modified. `--database /path/to/index.sqlite` lets an embedding host own storage.
Each database records its root and refuses a different worktree. Unsupported
schema versions and unrelated SQLite databases are rejected without adopting
them. Stop all clients before removing a derived index to rebuild it.

The scanner honors `.gitignore`, `.ignore`, and `.prometeu-searchignore`, including
nested rules. It also excludes `.git`, dependency/build/cache directories,
symlinks, non-UTF-8 files, unsupported languages, and files larger than 1 MiB.
The hard directory exclusions are listed in `src/index.rs`. Generated code in
other locations must be excluded by an ignore file.

Source snippets are stored locally in the cache. The engine makes no network
requests. An agent host may send returned source to its model provider under
that host's settings. The final cache directory created by the engine uses mode
`0700` on Unix; its database and lock files use mode `0600`.

## Development

```sh
cargo fmt --all --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
cargo build --release --locked
```

Tests exercise parser offsets, incremental graph changes, ignore rules, worktree
isolation, concurrent clients, byte budgets, native watcher handoff, and real
stdio MCP subprocesses. CI runs on Linux, macOS, and Windows.

Current limits: TypeScript/JavaScript/Rust/Ruby only; no import/type resolution,
cross-language bridges, framework routing, embeddings, or graph UI. Each
reconciliation enumerates and hashes eligible files, while parsing only changes.
Event-targeted reconciliation is the next performance step if profiling warrants
it. See [architecture](ARCHITECTURE.md) and [third-party notices](THIRD_PARTY_NOTICES.md).
