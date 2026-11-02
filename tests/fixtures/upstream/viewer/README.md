# Viewer evidence

The committed fixtures were captured from the pinned reference implementation,
independently of the Rust port. They execute offline during normal Rust tests.

- `branches.json`: 58 branch/loop sites in 10 sources, including early returns,
  nested arms, switches, ternaries, short-circuit operators, callbacks, and catches.
  The corpus covers JavaScript/TypeScript, Python, Go, Java, C#, C, Swift, and Kotlin.
- `calls.json`: 10 complete call-site records with injected receiver types,
  nested parallel calls, object response statuses, and same-block status lookback.
- `effects.json`: 848 classifier, explicit response-status, and implicit response-status
  answers across language families, project-type gates, database model/access
  extraction, and receiver types.
- `programs.json`: 11 complete ordered programs, including decision arms, loops,
  reply termination, repeated helper reads, deferred work, and parallel work.

`src/viewer/branch-rules.json` stores the reference's function-boundary and block
sets. `src/viewer/effect-rules.json` stores the ordered effect classifiers and
status names. These are ordinary embedded data; production uses native Rust
syntax trees and regex evaluation.

Regenerate using a built reference checkout:

```sh
node --no-wasm-tier-up tests/fixtures/upstream/viewer/regenerate.mjs /path/to/reference
```

Node is required only to regenerate independent expected results, not to build,
run, embed, or distribute the Rust engine or viewer. The browser assets ship in
the executable and require no package manager or build step.

Public integration tests cover graph bounds, source snapshots, reopen and edits,
branch/effect/program integration, alternative call paths, actual HTTP assets,
Host/Origin checks, path rejection, read-only writes, persistent trail resolution,
and generation events. These selected comparisons do not establish complete
viewer equivalence. See the [viewer contract](../../../../docs/contracts/graph-viewer.md)
and [provenance](../../../../crates/prometeu-extract/UPSTREAM.md).
