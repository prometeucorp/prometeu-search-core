# Python name-resolution oracle

`cases.json` contains 348 independently indexed or synchronized Python projects
and 2,555 extracted references from the pinned reference regression suites at
revision `6aa358ccbae30fbf27318f79ee183212f726f9ba`. It records the actual canonical
nodes and edges, full resolver answers, import mappings and targets, lexical
ownership, and typed attribute/local decisions. Another 467 expressions record
the original annotation and value readers directly.

`regenerate.mjs <prepared-checkout>` checks the revision, executes the original
project indexing APIs, and captures the resulting graph. Its development-only
module wrapper exposes private reference helpers without rewriting their logic.
Expected values never come from the Rust implementation. Offline Rust tests
need neither Node nor network access. The [reference license](LICENSE) covers
the reference-derived project inputs and snapshots.

The production gate compares every canonical edge, preserving duplicate records,
confidence, ownership, and metadata, before and after reopening with changed live
sources. Direct helper tests compare scopes, imports, annotations, factory returns,
attribute ownership, inherited methods, and annotated local receivers. The original
function-value suite adds class registries, imported callbacks, typed member values,
module instances, cross-file writes, test patches, and namespace-dictionary writes.
Exact-name helpers are compared for all 2,555 references. Public lifecycle tests
change a factory's return type through another connection, verify that uncommitted
edits cannot change answers, and remove the target. This corpus establishes these
cases; it is not an exhaustive proof for arbitrary Python programs.

Run `cargo test --locked --lib python_` for these comparisons and the related
Python framework and package-resolution tests.
