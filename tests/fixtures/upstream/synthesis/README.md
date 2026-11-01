# Canonical synthesis oracle

`cases.json` comes from the pinned reference revision
[`6aa358ccbae30fbf27318f79ee183212f726f9ba`](https://github.com/colbymchenry/codegraph/tree/6aa358ccbae30fbf27318f79ee183212f726f9ba).
The generator invokes its actual source extraction, SQLite persistence, reference
resolver, and ordered synthesis pipeline. It stores all canonical nodes, resolved
base edges, failed supertype references, imports, committed manifest inputs,
detected frameworks, and final edges. The resolver is initialized before its
batched resolution pass. Expectations never come
from the Rust implementation. Timestamps alone are removed from nodes.

Run `node tests/fixtures/upstream/synthesis/regenerate.mjs <prepared-checkout>`
to regenerate. Node and the reference checkout are only required to regenerate
the corpus; ordinary Rust tests consume the checked-in JSON.

Fixtures include the original Nix option-path test sources and the complete
multi-language `FILES` map from `override-overload-pairing.test.ts`. Additional
projects exercise cross-file Go containment, gRPC generated stubs, Kotlin
expect/actual, MyBatis XML, UI state updates, static overrides, C++
specializations, Swift protocol extensions, native component bridges,
framework dispatch and registrations, frontend route navigation, and Next
proxy/middleware matchers. The separate [Go corpus](go/README.md) retains
conformance and promoted-method cases.
The original MIT notice is retained in [LICENSE](LICENSE).

The unit test compares every derived-edge field, including provenance, source
positions, and synthesis metadata. Framework fixtures include nodes produced
by the reference's framework extraction stage. The integration suite selects
projects with ordinary indexing support, writes their source and manifest files,
indexes and reopens the database, and compares the resulting canonical edges.
This establishes those cases; it does not establish full resolver or framework
parity beyond the recorded inputs.

`navigation` records every input reference and the actual Next, Expo Router,
or SvelteKit resolver's primary and secondary targets, including empty results.
The frontend tests compare these records separately from the final graph.
`next-proxy.json` captures calls from the original pure matcher tests, including
rejected patterns, malformed configurations, and bounded adversarial patterns.
`comments.json` and `strings.json` retain the original helper results alongside
their UTF-8 byte-preserving projection used by native source scanners.

[Frontend declaration fixtures](frontend/README.md) are regenerated separately.

`const-alias.json` records 50 initializer interpretations and 26 complete projects
from the original alias-flow tests. Its generator records the actual upstream
import resolver's answers, including its explicit default-export proof, and all
edge fields from the alias pass. `tests/const_alias.rs` independently indexes all
26 source projects and verifies the projected links after reopening the database.

`go-func-field.json` records 13 projects, including the original provider callback
fixture, plus 1,282 actual local-declaration answers, four Go import-mapping cases,
and 484 English numeric collation comparisons. It retains every callback edge
field and its wiring location. Twelve projects run through ordinary indexing;
the remaining project exercises the pass's configurable cap directly. Both suites
also check removal of wiring through a second database connection and graph cache
invalidation. Their `regenerate-const-alias.mjs` and
`regenerate-go-func-field.mjs` scripts require the same prepared reference checkout.

`c-fnptr.json` comes from actual reference project indexing and its C/C++
function-pointer pass: 20 original projects plus a 305-handler fan-out case,
macro expansion limits, and a two-level quoted include environment. Complete
edge order is checked, including the first 300 handlers retained by the cap.
Twenty-two projects run through persistent indexing. The additional lifecycle
check retains unindexed include snapshots after live edits and reopening, then
verifies targeted reconciliation and rollback on oversized or symlinked inputs.
Regenerate with `regenerate-c-fnptr.mjs` and the prepared reference checkout.

`tauri.json` retains full command and event records from 32 original desktop IPC
projects and three additional projects for Unicode positions, the seven-variant
command ambiguity cap, and the sixteen-emitter event boundary. All 35 projects
also run through ordinary indexing and database reopening. A separate lifecycle
check removes and restores a command registration across connections and verifies
that queries use committed source. This is static source analysis with no desktop
runtime dependency. Regenerate with `regenerate-tauri.mjs` and the prepared
reference checkout.

The main corpus includes 36 original JSX render projects and 45 React route-table
projects, including a Unicode comment/scope regression. Their setup writes are
captured from original tests; extraction, finalization, resolution, and synthesis
expectations are then computed by the actual reference pipeline. Module-resolution
and explicit import-binding answers are recorded for isolated Rust tests.
React finalization retains complete generated nodes/references plus name updates;
its persistence test compares generated records and update identity/name/signature
fields. SQL readback fills omitted boolean flags with false, whereas native raw
records preserve their original absence. The indexing lifecycle check edits route
tables, removes/restores consumers, deletes files, and reopens the cache.

`react-bindings.json` independently runs the React resolver against 1,473
reference inputs from 81 indexed projects. Successful reference inputs are
reconstructed from the reference database's recorded name, kind, source and
position; unresolved inputs use its retained rows. All 212 resolver answers and
122 persisted framework edges are checked, including null answers, confidence,
layout metadata and columns. Its project sources come from the original JSX and
React Router suites captured in `cases.json`; `regenerate-react-bindings.mjs`
runs the actual reference resolver afresh. The persisted comparison decodes
nullable optional fields through the shared edge model. A separate lazy-export
lifecycle test checks cross-connection invalidation and committed source reads.
