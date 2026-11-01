# Erlang grammar provenance

Source: [WhatsApp/tree-sitter-erlang tag 0.19](https://github.com/WhatsApp/tree-sitter-erlang/tree/836aa2b6c3af2c7cef3f84049b0ed6d44485a870),
commit `836aa2b6c3af2c7cef3f84049b0ed6d44485a870`, [Apache-2.0](LICENSE).
The scanner retains its copyright notice: Meta Platforms, Inc. and affiliates.
Sources come directly from the GitHub repository; the unrelated npm package is
not used. The checked-in parser is ABI 14. No regeneration or local modifications.
Copy `src/parser.c`, `src/scanner.c`, `src/tree_sitter/*.h`, and `src/node-types.json`.

SHA-256:

- `parser.c`: `c8c83443d536875c2f28f12ae4f0242f477cac82115073f4d9506c2bb188dbcc`
- `scanner.c`: `bcb05457c981783245db637ec98a0b47f18e0acfe939efdd69a9da3e26f1b8a6`

The additional-language oracle checks complete native/WASM grammar tables and
independently captured full extraction records, including arities and OTP dispatch.
