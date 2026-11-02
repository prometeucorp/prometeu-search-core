# Additional native extraction oracle

`cases.json` records full extraction results from
[the pinned upstream implementation](https://github.com/colbymchenry/codegraph/tree/6aa358ccbae30fbf27318f79ee183212f726f9ba).
Original fixture sources are selected from its dedicated language tests and
`extraction.test.ts`, with focused Solidity and Objective-C syntax cases added.
The expected records are produced by executing the original TypeScript/WASM
extractor. Only timestamps and duration are removed. The copied upstream MIT
license is retained in `LICENSE`.

`source` is the original input. `input` records upstream preprocessing for
Objective-C, so the native walker and full wrapper can each be checked at the
proper boundary. `clean` reports whether the corresponding grammar parses that
input without errors. Native strict tests exclude malformed trees; this does
not assert equality between native and WASM error recovery.

The corpus contains 21 ArkTS, 58 Objective-C (54 clean), 8 Solidity, and 117
VB.NET cases. `grammars` captures every node-kind/field-name entry and ABI from
the loaded upstream grammar; the native table comparison verifies the pins.
Regenerate from the prepared pinned checkout with:

```sh
node tests/fixtures/upstream/supplemental/regenerate.mjs /path/to/upstream
```

The native crate tests all clean ArkTS/Objective-C/Solidity records; root tests
also compare the preprocessing wrapper and persist all 83 fixtures through
SQLite, including original-source reads and unchanged-file recovery. VB.NET's
walker has its own differential suite.
