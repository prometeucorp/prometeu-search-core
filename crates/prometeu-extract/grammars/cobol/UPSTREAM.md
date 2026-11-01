# COBOL grammar provenance

Source: [yutaro-sakamoto/tree-sitter-cobol](https://github.com/yutaro-sakamoto/tree-sitter-cobol/tree/e99dbdc3d800d5fa2796476efd60af91f6b43d93),
commit `e99dbdc3d800d5fa2796476efd60af91f6b43d93`, [MIT](LICENSE).
Apply the [pinned reference patch](https://github.com/colbymchenry/codegraph/blob/6aa358ccbae30fbf27318f79ee183212f726f9ba/docs/grammars/tree-sitter-cobol.patch)
and generate with official tree-sitter CLI 0.24.5, following its
[rebuild instructions](https://github.com/colbymchenry/codegraph/blob/6aa358ccbae30fbf27318f79ee183212f726f9ba/docs/grammars/tree-sitter-cobol.md).
The patched grammar and scanner include EXEC blocks, copybooks, wide free-format
mode, and the sequence-area EOF termination fix. The patch retains the reference
project's MIT license in the crate's root. No further local source modifications.

Copy generated `src/parser.c`, patched `src/scanner.c`, `src/tree_sitter/*.h`, and
`src/node-types.json`. Entry point: `tree_sitter_COBOL`; ABI 14.

SHA-256:

- `parser.c`: `cca01ffc4ef5c1d0a9d393d89a4dc42f4e9ee8621e89d6a551c32f4c85ddd041`
- `scanner.c`: `04a7feb8eaee7381f14dde24fd45ee798ad79f2ecc0807328897bc17d7df311b`

The additional-language oracle verifies complete native/WASM grammar tables and
full records, including fixed/free format, Unicode and the former EOF hang.
