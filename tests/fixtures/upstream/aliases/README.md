# Configuration alias reference fixtures

Pinned revision: `6aa358ccbae30fbf27318f79ee183212f726f9ba`.

`regenerate.mjs <prepared-checkout>` executes the original configuration loader
and module resolver for 19 independent projects. Expected maps and resolved paths
come entirely from that implementation. Cases cover root and nearest configs,
base URLs, declared packages and declaration shims, relative/package/array
inheritance, fallback order, empty paths, wildcard specificity, JSONC, cycles,
invalid first candidates, worktree boundaries, workspace precedence, and configs
deeper than workspace discovery's default depth.

The unit test compares pure loader results and module decisions, then writes the
same inputs to disk, captures confined indexing metadata, reloads it, and repeats
the comparisons. Express integration also exercises inherited alias edits across
persistent index generations.
