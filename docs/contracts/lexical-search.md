---
type: Contract
title: "Lexical search"
description: "Canonical search fields, field filters, ranking, and bounded fallbacks."
status: stable
---

# Lexical search

`Engine::search`, the `search` command, and MCP `code_search` in search mode
search canonical extraction nodes. The searchable fields are the name, qualified
name, documentation, and signature. Returned snippets are stored source from the
same committed generation. Source body text alone does not make a symbol match.
File nodes remain searchable with an explicit file or path filter.

Recognized filters are `kind:`, `lang:` (or `language:`), `path:`, and `name:`.
A quoted value can contain spaces: `path:"src/app views"`. Repeated values of one
field are alternatives; different fields are combined. Path and name filters
match case-insensitive substrings. Recognized kinds match canonical node kinds;
languages accept the parser's language identifiers and `unknown`. Unknown fields
or invalid kind/language values remain ordinary query text.

For example, `kind:function lang:rust path:src session` searches Rust functions
under paths containing `src`, with `session` as search text. A filter-only query,
such as `kind:file path:src/routes`, returns matching nodes in name order.

Search tries FTS5 prefix terms over the canonical fields, then substring matching
when no FTS matches remain, then bounded edit-distance matching. The FTS weights
are 20 for names, 5 for qualified names, 1 for documentation, and 2 for signatures.
Ranking additionally considers name matches, declaration kind, query words,
project-name words, test/generated/example paths, and configured deprioritization.
The numeric `score` describes that lexical ranking; it is not a probability or
semantic confidence. Search normalization includes identifier segments, Unicode
word handling, and the reference's conservative suffix variants.

`Engine::search_with_options` accepts `search::SearchOptions`: `limit` defaults to
100, `offset` to zero, and `max_bytes` to `None`. The library imposes no fixed
result-count or response-byte ceiling. Counts and offsets must fit JavaScript's
safe-integer range; `limit` must be positive. The existing `search(query, limit,
max_bytes)` method retains its explicit host-selected byte budget and zero offset.
An explicit byte budget must be at least 1024 bytes and has no fixed upper bound.

Offsets apply to each SQL candidate source before final ranking, matching the
reference query behavior. FTS and filter-only queries overfetch candidates;
exact-name supplementation can reintroduce a symbol on later pages. Fuzzy
fallback is independent of the offset. Consequently these are candidate pages,
not a promise of disjoint pages through one globally ranked list. An empty raw
query returns its five-times-overfetched candidate page; nonempty filter-only
queries retain the requested count.

CLI search defaults to ten results, accepts `--offset`, and has no implicit
serialized-byte budget. MCP search defaults to ten results and callers/callees
to twenty, with their requested counts clamped to 1–100. MCP free-form queries
allow up to 10,000 UTF-16 code units. Ordinary MCP tool content has a 15,000-code-unit
budget; the transport retains complete JSON records and reports truncation.
An explicit `max_bytes` can impose a smaller additional response budget.
Source snippets may be clipped to fit; `snippet_truncated` and `truncated` report
omitted content. Fuzzy matching retains its edit-distance and candidate-followup
limits but does not reject a repository merely for having more than 200,000 names
or 64 MiB of name text.

Search uses the indexed snapshot without refreshing when called through the Rust
API. CLI and MCP entry points refresh first. File IDs may change after reindexing;
use paths and line ranges when presenting stable human references. See the
[bundling contract](integration.md) and [source inspection](source-inspection.md).

Independent reference comparisons cover normalization, numeric ranking, field
filters, typo fallback, incremental updates, and persisted projects. Exploration
continues to use its existing graph and source-text seeding; this contract does
not claim identical exploration ranking. See [parity evidence](../quality/parity.md).
