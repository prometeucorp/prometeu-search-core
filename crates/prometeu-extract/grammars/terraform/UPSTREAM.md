# Terraform grammar provenance

Source: [tree-sitter-grammars/tree-sitter-hcl v1.2.0](https://github.com/tree-sitter-grammars/tree-sitter-hcl/tree/fad991865fee927dd1de5e172fb3f08ac674d914),
commit `fad991865fee927dd1de5e172fb3f08ac674d914`, [Apache-2.0](LICENSE).
The Terraform dialect is copied from `dialects/terraform/src`: `parser.c`,
`scanner.c`, `tree_sitter/*.h`, and `node-types.json`. No regeneration or local
modifications. Its entry point is `tree_sitter_terraform`, not the base HCL grammar.

SHA-256:

- `parser.c`: `176fe15a6cf1b395662fa2cb6041b47989f4414f2f7733194425475a976c668e`
- `scanner.c`: `78490613b129dd9eb8de7ddc79a676479a9c35600790b4efa1523bc0cf25b956`

The checked-in parser has ABI 15; the reference's npm v1.2.0 WASM artifact has
ABI 14. The additional-language oracle verifies their complete node-kind and
field tables are identical and compares full independent extraction records.
