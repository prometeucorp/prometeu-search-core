# Express reference fixtures

Pinned revision: `6aa358ccbae30fbf27318f79ee183212f726f9ba`.

`regenerate.mjs <prepared-checkout>` invokes the original resolver and project
API. Rust results never generate expected output. The corpus includes:

- 84 complete extraction records, with node timestamps removed. Inputs include
  original test literals, inline and wrapped handlers, chained registrations,
  regex/division/JSX boundaries, trailing options, handler arrays, Unicode,
  comments, and supported filename suffixes.
- 30 direct middleware/controller/service resolver decisions, including the
  original same-file restriction and numeric confidence.
- Ten projects indexed through the reference API, with canonical route records
  and outgoing edges. Router mounts cover ESM, CommonJS, composed and ambiguous
  prefixes, options, workspace packages, and inherited TypeScript aliases.
  The invalid workspace export case also records a deliberate difference: the
  reference falls back to an unrelated global name, while this engine retains
  unresolved evidence after the package target is rejected.

`tests/express.rs` checks the indexed records and edge endpoints after reopening
with modified live source. A separate lifecycle case changes an inherited alias
file through `sync_paths` and checks route renaming and search updates.

Finalization recomputes Express names from their original qualified paths so a
changed or removed mount restores previous names. This freshness adaptation
retains canonical IDs and all other fields; both FTS indexes update atomically.
