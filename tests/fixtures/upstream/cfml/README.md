# CFML family reference records

The 189 full-record cases in `cases.json` were captured independently from the
pinned upstream TypeScript CFML wrapper and WebAssembly CFScript/CFQuery
walkers. Cases cover tag and script components, interfaces, inherited types,
visibility, properties, closures, scoped calls, component construction, template
includes, embedded scripts, SQL hash expressions, malformed blocks, BOMs,
Unicode, and line endings. File-based cases also pass through persistent indexing.

All three grammars use the exact `tree-sitter-cfml` 0.26.30 release at commit
`776f83d82440aeb05d163bc454e88890a8a99870`. Its three checked-in WebAssembly
artifacts are byte-identical to those bundled in the reference checkout. The
published Rust crate's generated parsers, scanners, and common scanner headers
were verified byte-identical to that tag. Tests additionally compare every node
kind and field name, ABI, and parse-state count with the WebAssembly grammars.

Run `node tests/fixtures/upstream/cfml/regenerate.mjs /path/to/prepared/upstream`
to recapture the pinned reference. Node and WebAssembly are development-time
oracle dependencies; the Rust implementation uses native grammars.

See the enclosing [license](../LICENSE) and
[provenance](../../../../crates/prometeu-extract/UPSTREAM.md).
