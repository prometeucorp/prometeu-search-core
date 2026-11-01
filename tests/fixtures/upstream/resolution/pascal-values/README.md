# Pascal routine values

Independent source, graph, and resolver snapshots from the pinned reference parenthesis-free routine regression suite. Run the generator with the prepared reference checkout; normal Rust checks read JSON without a JavaScript runtime.

The 12 projects contain 30 extracted parenthesis-free references. The gate compares all resolved mode-reference edges, including call-versus-value kind and numeric confidence. Run `cargo test --locked --lib pascal_parenthesis_free_values_match_reference_graphs`.
