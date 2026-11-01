# Callable values

Independent snapshots from the pinned reference regression suites cover callable values, scoped member pointers, project imports, package imports, and receiver ownership. The generator runs the reference implementation and retains source, canonical nodes, edges, and resolution decisions. Rust tests consume the JSON directly; Node is required only to regenerate fixtures.

Run `node --liftoff-only tests/fixtures/upstream/resolution/function-values/regenerate.mjs /path/to/reference` after building the pinned reference.

The normal gate compares every callback edge across 105 projects and 200 references in 13 language identifiers. It preserves duplicate edges, call positions, strategy, and numeric confidence, then repeats after reopening with changed live files. Run `cargo test --locked --lib callable_values_match_reference_projects_and_reopened_snapshots`.
