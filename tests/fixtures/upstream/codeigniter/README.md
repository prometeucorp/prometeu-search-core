# CodeIgniter reference records

The pinned revision and MIT provenance are recorded in the [parity contract](../../../../docs/quality/parity.md) and [license](LICENSE).
`regenerate.mjs` captures complete extraction and resolver decisions, then indexes four real projects through the reference public API.

The corpus checks 57 extraction inputs, 58 direct lookup decisions, and 34 complete framework edges. Cases cover controller conventions, explicit routes, loader aliases, HMVC module isolation, inherited controller/model methods, and view file calls. Normal indexing and reopening retain the same canonical edge metadata after live files change. View arrival, deletion, and loader edits invalidate cached answers across connections.

```sh
node --no-wasm-tier-up tests/fixtures/upstream/codeigniter/regenerate.mjs /absolute/path/to/reference-checkout
cargo test --locked --lib frameworks::codeigniter
cargo test --locked --lib resolution::framework_bindings
cargo test --locked --test codeigniter
```
