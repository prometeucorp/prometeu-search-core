# Angular reference fixtures

Pinned revision: `6aa358ccbae30fbf27318f79ee183212f726f9ba`.

Both generators take a prepared reference checkout as their only argument. They
invoke the original extraction, resolution, template, and project APIs; Rust
results never generate expected output.

`regenerate.mjs` captures 45 complete source extraction records and component
declarations, 14 static string values, 22 navigation command arrays, 35 template
member-use lists, and seven microsyntax results. Cases cover original tests,
Unicode offsets, template locals, control flow, comments, and string holes.

`regenerate-projects.mjs` captures eight projects, including four original test
projects. Records include mounted route names, constant lookups, redirect
tables, every raw reference's framework resolution, and complete synthesized
template edges. Additional projects exercise dependency gates, duplicate
selectors and application roots, the 40-child limit, and the 24-link limit.

`tests/angular.rs` compares indexed route records and canonical edges after
reopening. Its external-template lifecycle test proves that live HTML changes
remain invisible until reconciliation, edits and removals advance the committed
generation, and oversized or symlink targets roll back atomically.

Route finalization recomputes names from original qualified paths on each
reconciliation. Source and auxiliary template reads use committed snapshots;
querying the graph never rereads the live worktree.
