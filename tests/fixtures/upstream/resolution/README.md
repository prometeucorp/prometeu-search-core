# Multi-file graph reference cases

`cases.json` contains 20 projects independently captured by the pinned upstream
project's `init` and `indexAll` APIs. This exercises its extraction, SQLite store,
resolution, synthesis, and deferred resolution lifecycle. Each project records
all canonical nodes and edges, caller/callee queries, and per-node graph metrics.
The separately captured extraction records preserve the raw parser boundary.

The projects cover TypeScript imports and re-exports, Python relative imports and
shadowing, Go modules and package visibility, Java/C# inheritance and typed
receivers, Ruby requires, Rust modules, CFML construction and case folding, PHP
namespaces, Erlang arity-qualified calls, callback value references, external
imports, and unrelated-language name collisions.

The Rust comparison checks complete extraction-node records without timestamps,
resolved edge endpoints and kinds, all caller/callee neighbor sets, public query
results, graph metrics, and cache reopening after live sources have changed.
Resolver confidence labels remain explicit engine evidence; the test does not
claim identical numeric confidence or arbitrary tie-breaking. A declared C#
namespace ambiguity permits additional candidates only when they carry low
confidence and an ambiguity flag. Its metric adjustment counts only those
declared extra import edges. All other edge topology and metrics must agree.

The separate [Python module corpus](python-modules/README.md) checks 2,662 exact
module-path decisions across 71 source and packaging configurations.

Run the offline check with:

```sh
cargo test --locked --lib graph::resolution
```

To recapture the reference, run
`node tests/fixtures/upstream/resolution/regenerate.mjs /path/to/prepared/upstream`.
The script verifies revision `6aa358ccbae30fbf27318f79ee183212f726f9ba` and uses
the original TypeScript/WebAssembly implementation. Node is needed only for
this independent development-time capture.

See the enclosing [license](../LICENSE) and
[provenance](../../../../crates/prometeu-extract/UPSTREAM.md).
