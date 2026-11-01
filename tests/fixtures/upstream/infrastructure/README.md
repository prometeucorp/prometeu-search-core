# Infrastructure reference records

The pinned revision and MIT provenance are recorded in the [parity contract](../../../../docs/quality/parity.md) and [license](LICENSE).
`regenerate.mjs` indexes nine real projects and captures 980 direct resolver decisions plus 96 complete framework edges from the reference implementation.

Terraform/OpenTofu cases cover directory scope, ancestor variables and providers, local module files and input/output wiring, registry boundaries, remote-state component defaults, and ambiguous components. COBOL cases cover CICS transaction ownership, case normalization, unresolved transactions, and declaration ordering. The same complete records are checked after replacing live files and reopening the committed Rust index. Bounds failures remain explicit graph truncation rather than cross-module guesses.

```sh
node --no-wasm-tier-up tests/fixtures/upstream/infrastructure/regenerate.mjs /absolute/path/to/reference-checkout
cargo test --locked --lib resolution::infrastructure
```
