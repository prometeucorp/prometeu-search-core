---
type: Contract
title: "Source inspection"
description: "Bounded file listing, symbol metadata, and source reads from committed snapshots."
status: stable
---

# Source inspection

The library, CLI, and MCP expose indexed files and source without opening live
project files during a read. CLI/MCP entry points refresh first. Library callers
choose when to refresh. Every response identifies the committed generation.

| Capability | CLI | MCP | Library |
| --- | --- | --- | --- |
| List files | `files [query]` | `code_files` | `Engine::files` |
| Read a symbol | `node <id>` | `code_node` | `Engine::node` |
| Read file lines | `file <path>` | `code_file` | `Engine::file` |

File listing accepts an optional case-insensitive path substring, a positive
`limit`, and optional `max_bytes`. CLI/MCP default to all matching files before
transport budgets apply. Results include path, extraction language,
symbol count excluding the file projection, and syntax-error status. Results are
ordered by path. `truncated` indicates omitted files.

A node read requires a positive integer search-result ID. IDs belong to the
current cache and can change after reindexing. The result includes source and,
when available, the canonical extraction metadata: signature, documentation,
visibility, type information, and other upstream fields. Supplemental projection
symbols can lack canonical metadata. Missing or stale IDs return an error.

A file read requires a worktree-relative path and accepts `start_line` (default
1) and optional `end_line`. Lines are positive and inclusive. A range starting
beyond the indexed source returns an error. An ending line beyond the source is
clamped to the source length. MCP defaults an omitted ending line to a 2,000-line
window; callers can request another range. The library and CLI honor the requested
range without an implicit line ceiling. The result reports total lines, the returned range,
language, and syntax-error status. A trailing newline introduces a final empty
line. Missing files return an error. Absolute paths and parent traversal are rejected.

All three operations accept explicit `max_bytes` of at least 1024 with no fixed
upper ceiling. CLI has no implicit byte budget. MCP applies 15,000 UTF-16 units
to file listings and symbol reads, and 38,000 to file windows, while retaining
complete JSON results. An explicit byte budget applies in addition. The
budget covers the complete serialized result, including escaped source and
metadata; MCP framing adds bytes. Source is clipped only at UTF-8 boundaries.
`source_truncated` identifies clipped source. `truncated` also reports omitted
metadata or a result that could not fit. If identifying fields alone exceed the
budget, `node` or `file` is null and `truncated` is true.

Source is untrusted project content. A result is an indexed snapshot, not a
filesystem lock; verify live source before editing. See the
[integration contract](integration.md) for worktree ownership and lifecycle.
