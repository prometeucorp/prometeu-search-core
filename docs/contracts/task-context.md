---
type: Contract
title: "Task context"
description: "Ranked task context, canonical graph records, source blocks, formatting, and response bounds."
status: stable
---

# Task context

`Engine::context` accepts a `TaskInput` and `BuildContextOptions`, and reads one
committed graph and source snapshot. A task can be a query string or a title with
an optional description. The method does not refresh implicitly or read live
files. CLI `context` and MCP `code_context` refresh before querying.

Selection combines exact names, lexical search, identifier segments, substrings,
file paths, source classification, and graph relationships. Entry-point ranking,
type hierarchy expansion, per-file limits, and nonproduction limits keep results
focused on the query. Configured deprioritization adjusts ranking without hiding
files. Retrieval confidence describes selection relevance; it does not turn a
static graph edge into compiler proof.

Explicit `node_kinds` and `edge_kinds` restrict the complete selected subgraph,
including search seeds, hierarchy expansion, and the final edge merge. Empty
kind lists impose no additional restriction. Hierarchy expansion does not cross
excluded nodes or relationship kinds.

The response contains `generation`, `truncated`, and `context`. The context has
`query`, `subgraph`, `entryPoints`, `codeBlocks`, `relatedFiles`, `summary`, and
`stats`. Its nodes and edges use canonical string IDs, distinct from numeric IDs
used by search and source inspection. Source blocks use committed file lines.
Configuration-key declarations return the key or signature instead of including
configuration values. Canonical graph metadata retains resolution evidence.

Defaults are 20 nodes, five code blocks, 1,500 UTF-16 units per block, three search
entry points, depth one, and minimum score 0.3. These counts and traversal depth
are configurable without fixed application ceilings; counts must fit the ranking
and SQLite arithmetic. Minimum score must be finite and between zero and one.
Queries contain 1–10,000 UTF-16 units. Zero code blocks, zero block size, and depth zero are
supported. Code blocks use an explicit truncation marker when their source limit
is reached.

An explicit `max_bytes` must be at least 1,024 UTF-8 bytes, without a fixed upper
ceiling. CLI has no implicit byte budget. MCP additionally bounds the complete
JSON content to 25,000 UTF-16 units. When the
structured response exceeds that budget, complete source blocks and then lower
priority nodes are removed. Removed nodes also remove their edges, roots, entry
points, and source blocks; statistics and the summary are recomputed. Original
root order is preserved. `truncated` also reports an incomplete underlying graph.
Node-count selection also removes roots whose nodes are not retained.
A budget too small for mandatory response metadata returns an error.

`ContextResult::markdown()` formats the same retained snapshot with source blocks,
relationship descriptions, short call paths, and an explicit low-confidence
handoff when appropriate. Markdown has the same byte ceiling and an explicit
marker if formatting reaches it. CLI `context --format markdown` prints this
text; the default format is JSON. MCP returns structured JSON in its text block.
Transport framing is outside the result byte budget.

```sh
prometeu-search --root /absolute/worktree context "validateToken and login" --max-bytes 12000
prometeu-search --root /absolute/worktree context "validateToken" --format markdown --no-code
```

MCP `code_context` accepts `query`, `max_bytes`, and an optional `options` object.
The option keys are `maxNodes`, `maxCodeBlocks`, `maxCodeBlockSize`, `includeCode`,
`searchLimit`, `traversalDepth`, and `minScore`. Unknown fields are rejected.

The public `context::ContextBuilder` also accepts host-owned canonical nodes,
edges, sources, search callbacks, and classification predicates for direct Rust
embedding. Its `find`, `build`, and formatting helpers do not open files or
contact services. [Parity evidence](../quality/parity.md) distinguishes the
independently compared context algorithm from remaining general resolver gaps.
