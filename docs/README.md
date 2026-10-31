# Prometeu Search documentation

`docs/` is the repository's OKF knowledge bundle. Standing instructions remain in
[`AGENTS.md`](../AGENTS.md); the [product](../README.md) and
[architecture](../ARCHITECTURE.md) maps remain at the root.

When the location is unknown, choose an area, read its index, then open the
relevant concept and follow its dependencies. Open known paths directly.

- [Contracts](contracts/README.md) — Interfaces and responsibilities between the engine and its host.
- [Decisions](decisions/README.md) — Architectural choices, rationale, and documentation conventions.

## Maintenance

Update the relevant concept when behavior, a contract, or a decision changes.
Each concept has YAML `type`, `title`, `description`, and `status`, plus an entry
in its folder's `README.md`. Folder indexes are navigation, not concepts.
Only create areas when they have content; see the
[format and lifecycle](decisions/README.md).

Keep draft proposals separate from current guidance. Completed plans become
contracts or decisions, or leave the tree; Git retains their history.
Run `cargo test --locked --test docs` to check metadata, local links, index
coverage, and the short agent map.
