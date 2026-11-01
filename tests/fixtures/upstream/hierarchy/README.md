# Type hierarchy reference fixtures

The generator runs the pinned implementation at revision
`6aa358ccbae30fbf27318f79ee183212f726f9ba`. It indexes four real TypeScript, Go,
Rust, and Java projects, then captures its actual hierarchy queries and complete
records. Eight additional graph snapshots exercise cycles, missing nodes,
duplicate relationships, locale ordering, ancestor and descendant depth limits,
wide subtype fans, hidden children, and nearest-member override matches.

The corpus contains 140 hierarchy answers, including disabled override lookup,
plus eligibility and direct subtype counts. Pure Rust tests compare complete
records. Public engine tests independently index all four projects and compare
complete hierarchies before and after reopening, with modified live source to
prove snapshot isolation. Separate checks cover committed inheritance changes,
response budgets, CLI output, and MCP argument validation.

Timestamps are replaced with zero. SQL null optional fields and omitted Rust
optional fields deserialize to the same value. No hierarchy edges, order, counts,
confidence, provenance, or override records are removed from comparisons.

Regenerate with `node --no-wasm-tier-up regenerate.mjs <pinned-checkout>` after
preparing the reference's development dependencies. Normal Rust tests use the
committed JSON and need no JavaScript runtime. The derived fixtures retain the
reference [MIT license](LICENSE).
