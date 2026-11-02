# Canonical graph query reference cases

`cases.json` records the pinned reference's SQLite graph queries and traversal
algorithms over twenty independently indexed multilingual projects, eight
hierarchy snapshots, and two graphs with parallel edges, recursion, shorter
routes, and nested containers. Expectations come from the original query code,
not the Rust implementation. Canonical input records preserve complete metadata;
answer records refer to those nodes and edges by ID or array position to avoid
repeating large records for every traversal combination.

The corpus covers BFS, DFS, caller/callee depth, all call sites, call graphs,
type graphs, usages, containment ancestry, children, shortest paths, impact
budgets, symbol context, file dependencies/dependents, exports, qualified-name
patterns, and filtered graphs. Order, parallel call positions, cycles, missing
nodes, node/edge filters, and inclusion of the starting node are significant.
The Rust API additionally reports truncation for bounded traversals.

Run `cargo test --locked --test graph_queries`. Regenerate with
`node --no-wasm-tier-up tests/fixtures/upstream/graph-queries/regenerate.mjs /path/to/prepared/upstream`.
The script checks revision `6aa358ccbae30fbf27318f79ee183212f726f9ba` before capture.
Normal tests run offline without Node or Git.

See the retained [license](LICENSE) and
[provenance](../../../../crates/prometeu-extract/UPSTREAM.md).
