# Go conformance reference records

The corpus executes the pinned reference's native extraction, SQLite resolver,
and ordered synthesis pipeline on eight projects. Literal file maps come from
its Go arity, type, defined-type, embedding, unexported-method, interface-embedding,
and standard-library regression suites. Dynamic fixture-generation expressions
are omitted; two additional projects check the 40-implementer cap and Unicode
exported method names. These are complete projects as recorded in each case,
not a claim that every dynamic case in those suites has been ported.

Records retain canonical nodes, resolved base edges, unresolved embeddings,
complete expected conformance edges, all synthesized edges, and independently
evaluated signature arities. The Rust comparison includes every edge field.
Only node timestamps are normalized. The standard-library method table in
`src/synthesis/go_stdlib.json` is extracted from the pinned reference's static
AST literal by the same script; it is not derived from the host's Go installation.

Run `node tests/fixtures/upstream/synthesis/go/regenerate.mjs /path/to/prepared/upstream`.
The checkout must match revision `6aa358ccbae30fbf27318f79ee183212f726f9ba`.
Tests consume committed records offline. The reference [license](LICENSE) is retained.
