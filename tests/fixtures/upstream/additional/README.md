# Additional native language oracles

The 80 cases cover Nix (8), Erlang (36), Terraform/OpenTofu (26), and COBOL (10).
Expected full records were captured independently from the pinned TypeScript/WASM
implementation at commit `6aa358ccbae30fbf27318f79ee183212f726f9ba`.
The generator runs only the four pure extraction groups in upstream
`__tests__/extraction.test.ts`, with inert assertions and a direct reference
walker, then adds Unicode LF/CRLF and COBOL end-of-input regressions. It never
runs upstream filesystem/indexing suites. Unique fixture paths isolate upstream
file-keyed memoization between independent cases.

Native Rust results do not generate expected records. All fields except node
timestamps and extraction duration are compared; duplicates remain significant.
The grammar node-kind and field tables are independently captured from the WASM
artifacts and compared with the compiled native grammars. Terraform's native
parser has ABI 15 while the same-revision upstream WASM has ABI 14; the complete
node-kind and field tables match.

The Rust tests also index every case, reopen the persistent database, and verify
the original source and all stored canonical node metadata through inspection.
Extraction and persistence evidence does not establish full resolver parity.

Prepare the pinned checkout as described in the enclosing
[reference instructions](../README.md), then run:

```sh
node --liftoff-only tests/fixtures/upstream/additional/regenerate.mjs /path/to/reference
cargo test --locked --test additional_languages --test upstream_parity
```

`--liftoff-only` is the upstream workaround for V8 optimizing-compiler exhaustion
on the large COBOL WASM. Node is required only for reference regeneration, never
for Rust execution or tests. Upstream fixtures retain their enclosing [MIT
license](../LICENSE); native grammar licenses and pins live beside their sources.
