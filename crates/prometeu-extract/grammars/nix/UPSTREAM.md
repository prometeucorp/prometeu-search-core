# Nix grammar provenance

Source: [nix-community/tree-sitter-nix](https://github.com/nix-community/tree-sitter-nix/tree/3d0173d903e630b6e14d17f1cf79488791379ded),
commit `3d0173d903e630b6e14d17f1cf79488791379ded`, [MIT](LICENSE).
Run official tree-sitter CLI 0.25.10 `generate` in that checkout, matching the
pinned reference WASM's ABI-15 regeneration. Copy `src/parser.c`, `src/scanner.c`,
`src/tree_sitter/*.h`, and `src/node-types.json`. No local grammar changes.

SHA-256:

- `parser.c`: `0f185844da0e8e033127ebaa625554994eab6ef5d1a9629dd41b579fb41a8792`
- `scanner.c`: `45e12521e8be62ea47525417c64d9372fa34e9e12bff643c430b8b3607be9d56`

The additional-language oracle checks the complete node-kind and field tables
against the independently loaded reference WASM, alongside full extraction records.
