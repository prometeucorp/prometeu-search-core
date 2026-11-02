# Laravel reference records

The pinned revision and MIT provenance are recorded in the [parity contract](../../../../docs/quality/parity.md) and [license](LICENSE).
`regenerate.mjs` captures the reference extractor and resolver directly, then indexes eleven real projects through its public project API.

The corpus checks 55 complete extractions, 286 controller/model lookup decisions, route-file prefixes, nested includes, resource controllers, namespace disambiguation, and Laravel 11 bootstrap configuration. Persistent tests compare route records and handler targets after replacing live files and reopening the committed index. Mount removal and framework detection changes exercise generation invalidation, stable route identities, and both search indexes.

```sh
node --no-wasm-tier-up tests/fixtures/upstream/laravel/regenerate.mjs /absolute/path/to/reference-checkout
cargo test --locked --lib frameworks::laravel
cargo test --locked --test laravel
```
