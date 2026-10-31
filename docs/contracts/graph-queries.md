---
type: Contract
title: "Graph queries"
description: "Static resolution, bounded exploration, graph analysis, and result uncertainty."
status: stable
---

# Graph queries

The engine combines lexical retrieval with static source relationships. It runs
locally without a model, compiler service, network connection, or provider
integration. The [integration contract](integration.md) defines process and cache
ownership; [architecture](../../ARCHITECTURE.md) describes extraction and recovery.

## Entry points

`code_search` retains its `search`, `callers`, and `callees` modes. Relationships
now use import, scope, module, and receiver evidence before name fallback. Existing
relation fields remain present; locations add `id` and `qualified_name`, while
relationships add `kind`, `confidence`, and `ambiguous`.

`code_explore` accepts these arguments:

| Argument | Meaning |
| --- | --- |
| `query` | Required nonempty symbol, file, or lexical query, up to 1024 UTF-8 bytes. |
| `mode` | `explore` by default; also `impact`, `path`, `implementations`, or `affected`. |
| `path` | Optional exact file relative to the fixed worktree. Absolute paths and parent traversal are rejected. |
| `line` | Optional positive source line, requiring `path`, for symbol disambiguation. |
| `target` | Required destination symbol for `path` mode; rejected in other modes. |
| `depth` | Maximum traversal depth, 1–8, default 2. |
| `limit` | Maximum returned source symbols, roots, impact entries, or affected test files per collection, 1–50, default 10. |
| `max_bytes` | Maximum complete serialized result, 1024–65536 bytes, default 12000. MCP framing is additional. |

The CLI provides `explore`, `impact`, `path`, `implementations`, and `affected`
commands. `path` accepts the source query and destination as positional arguments.
Graph commands accept `--path`, `--line`, `--depth`, `--limit`, and `--max-bytes`.
For example:

```sh
prometeu-search explore 'session launch' --limit 8
prometeu-search explore save --path src/store.ts --line 42
prometeu-search impact Store --depth 3
prometeu-search path start save --depth 4
prometeu-search implementations Repository --depth 3
prometeu-search affected src/store.ts --depth 5
```

Rust hosts call `Engine::explore(query, &GraphOptions)` or
`Engine::analyze(query, GraphMode, &GraphOptions)`. These read an indexed snapshot;
call `refresh` or `sync` first when freshness is required. CLI and MCP entry
points refresh automatically.

Rust hosts can also call `Engine::overview()` for a file-level map without a
query. It returns `generation`, `files` (`path`, `language`, indexed `symbols`
count), directed `edges` (`from` and `to` file paths, `count`, `uncertain`), and
`truncated`. Cross-file resolved symbol relationships are aggregated by ordered
file pair; same-file and unresolved relationships are excluded. `count` counts
static relationships, not runtime calls. `uncertain` is true if any contributing
relationship is ambiguous or has low/unknown confidence. Medium-confidence
relationships remain static candidates even when this flag is false.

Overview uses the same committed snapshot and generation cache as exploration.
It returns files and edges in lexical path order, bounded to 1,000 files,
4,000 edges, and 512 KiB of serialized JSON. Truncation at these limits or during
graph construction is explicit; omitted files or relationships are not absent
code. Every returned edge refers to a returned file. `Engine::status()` supplies
the complete indexed file/symbol counts; its separately read generation can
differ if another client commits between calls. Hosts call `refresh` before a
fresh overview or `sync` for an explicit full-content reconciliation. Overview
does not change CLI/MCP tools or the cache schema. `tests/overview.rs` covers
aggregation, evidence, concurrent updates, and response bounds.

## Resolution

Qualified names use `::` between enclosing scopes. A file path plus qualified
name disambiguates symbols; IDs are internal to a committed generation and can
change after indexing. JavaScript/TypeScript imports retain named aliases,
default imports, namespace imports, and reexports. Rust retains module paths and
`use` aliases. Ruby retains `require_relative` dependencies and enclosing scopes.
Simple receiver annotations and construction expressions provide type evidence.
TypeScript interface methods and types imported through a namespace participate
in receiver resolution, subject to the same scope and export boundaries.
Inheritance and implementation declarations create candidate type relationships.
Local bindings, including destructuring and catch parameters, hide imported
values within their scopes. Calls through unknown bindings that hide imports
remain unresolved; known receiver types retain their static evidence.
JavaScript/TypeScript `let` and `const` bindings declared in a loop header hide
imports only within that loop; `var` bindings retain function scope.

Resolution methods describe evidence, not proof:

- `syntax` describes direct structural containment.
- `scope_match`, `import_match`, `module_match`, and `receiver_match` describe
  static candidates supported by the corresponding source evidence.
- `name_match` is a low-confidence fallback that may connect unrelated symbols.
- `unresolved` means no indexed candidate was established.

Confidence is `high` for direct containment, `medium` for unambiguous static
resolution, `low` for name fallback or multiple candidates, and `unknown` for
unresolved references. Each relationship reports its own ambiguity. Known import
or receiver evidence is not replaced with an unrelated same-name target merely
because the expected target is missing.

Resolution is heuristic. Package configuration aliases, external dependencies,
macros, metaprogramming, runtime dispatch, and complete compiler visibility/type
rules are not implemented. Unsupported patterns remain unresolved or low-confidence
candidates. Results from files with syntax errors retain `syntax_errors`.

## Analysis semantics

`explore` selects exact names, qualified names, or files first, then uses ranked
lexical search when no exact symbol matches. It expands a bounded neighborhood
and assembles source grouped by file, relationships, and reverse impact. Large
source snippets can begin near a matching line; `snippet_start_line` locates the
returned text while `start_line` and `end_line` retain the complete symbol range.
`snippet_truncated` identifies incomplete source.

`impact` follows incoming calls and type relationships transitively. Selecting a
class also considers its contained members. `implementations` follows incoming
`extends` and `implements` relationships. `path` finds a shortest directed call
path between selected source and destination candidates. Cycles are visited once.
All these operations inherit the uncertainty of their relationships.

`affected` follows file imports in reverse and returns candidate test paths.
Test detection recognizes test/spec directories and common filename patterns,
including `.test.`, `.spec.`, `_test.`, `_spec.`, and `test_`. It does not execute
tests or claim complete test coverage. An empty impact, path, or affected-test
result is not proof that a change is safe.

## Results and bounds

Graph responses contain `generation`, `roots`, `files`, `edges`, `path`, `impact`,
`affected_tests`, `ambiguous`, and `truncated`. `path` holds symbol IDs in traversal
order. Edges identify source and optional destination IDs; a missing destination
retains the unresolved target spelling. Consumers must tolerate partial context
when `truncated` is true and must not interpret omitted data as absent code.

Every response reads one committed SQLite snapshot. Graph metadata is cached by
generation, including invalidation after another client commits an update. Source
snippets come from that same indexed snapshot, never from live files at old
byte offsets. Read live source before making edits that depend on exact content.

Graph construction bounds serialized extraction metadata at 64 MiB, symbols at
100,000, edges at 200,000, and resolution candidates at 50. Traversals visit at
most 1,000 symbols and collect at most 4,000 traversal edges. Depth, collection,
graph construction, and serialized-byte limits can all truncate results.
The serialized budget includes JSON escaping and metadata. Source and collections
are shortened to fit; every omitted result is signaled with `truncated`.

The graph cache is rebuilt after a changed generation. Targeted source updates
avoid hashing unrelated files, but rebuilding graph metadata still depends on
indexed graph size. This is separate from source synchronization costs.
