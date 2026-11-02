# Frontend declaration oracle

`cases.json` contains 598 per-file extraction cases produced by the pinned
[reference revision](https://github.com/colbymchenry/codegraph/tree/6aa358ccbae30fbf27318f79ee183212f726f9ba).
The generator invokes the actual Next.js, Expo Router, and Svelte extractors.
Cases combine route-path conventions with named, anonymous, wrapped, aliased,
and endpoint exports. Expectations retain every node and unresolved-reference
field except node timestamps.

Run `node tests/fixtures/upstream/synthesis/frontend/regenerate.mjs <prepared-checkout>`
to regenerate. Runtime Rust tests use the checked-in JSON and need no JavaScript.
Full graph and navigation resolver cases live in the [parent corpus](../README.md).
The original MIT notice is retained in [LICENSE](LICENSE).

React fixtures add 348 complete per-file records and 174 structural route scans,
1,212 static loader helper inputs, 58 lazy-loader calls, 771 export reader inputs,
and 97 lexical binding cases. Original test literals supply the inputs; helper
expectations execute the pinned TypeScript functions directly, including private
functions exposed only by the development generator. Source branding in fixture
inputs is replaced before executing the reference. Unicode positions are converted
to UTF-8 only for scanners whose Rust API returns byte offsets. Canonical node and
reference positions retain the reference's UTF-16 units.
