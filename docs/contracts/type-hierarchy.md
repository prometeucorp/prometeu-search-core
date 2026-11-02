---
type: Contract
title: "Type hierarchy"
description: "Bounded ancestry, subtype fans, and member override matches from committed graph records."
status: stable
---

# Type hierarchy

`Engine::hierarchy(id, max_bytes)`, `prometeu-search hierarchy <id>`, and the MCP
tool `code_hierarchy` read the hierarchy of a positive search-result ID. The CLI
accepts `--max-bytes`; MCP accepts `id` and optional `max_bytes`. The byte budget
is optional in CLI/MCP and accepts at least 1024 bytes without a fixed upper
ceiling. MCP also applies its 15,000 UTF-16-unit response budget. CLI and MCP refresh first; the library
reads the current committed snapshot without reading live files.

The response contains `generation`, `hierarchy`, `ambiguous`, and `truncated`.
Unknown IDs fail explicitly. `hierarchy` is null for a non-type or a type without
inheritance relationships. A response reduced to null by the byte budget has
`truncated: true`.

The hierarchy includes its canonical `focus`, ancestors, descendants, direct
subtype and implementation counts, a `polymorphic` flag, and member `overrides`.
Each entry retains its canonical node and edge, depth, parent ID, relation,
synthesized status, and hidden subtype count. Only `extends` and `implements`
edges participate. Synthesized inheritance remains labeled with its provenance.
Override matches compare member names along established ancestry. They do not
assert signature compatibility or compiler-verified dispatch.

Ancestry walks at most eight levels. Descendants walk breadth first for six
levels and at most 400 entries. `bounded` marks omitted descendants, and direct
counts describe the complete direct fan even when entries are omitted. Eight
direct implementations mark a polymorphic dispatch fan. Override lookup uses
the nearest twelve returned ancestors. Cycles are visited once.

Byte trimming removes override details, then the deepest descendant entries,
then ancestor entries. Direct counts remain intact; retained parents record
additional omitted children. `truncated` also propagates incomplete graph
construction. `ambiguous` marks uncertain inheritance in the computed hierarchy.
All returned source metadata belongs to the same generation.

For host-owned canonical records, `hierarchy::build_type_hierarchy` provides the
pure derivation, and `can_have_hierarchy` and `count_implementers` expose its
eligibility and counting rules. The pure builder accepts the complete snapshot;
ancestry and descendant limits apply to the selected hierarchy, not the input
graph size. The [graph contract](graph-queries.md) describes resolution
uncertainty and the [integration contract](integration.md) defines lifecycle.
