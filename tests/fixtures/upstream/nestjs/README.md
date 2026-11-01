# NestJS reference records

The pinned revision and MIT provenance are recorded in the [parity contract](../../../../docs/quality/parity.md) and [license](LICENSE).
`regenerate.mjs` invokes the original extraction and finalization tests against the reference implementation, then captures six actual indexed projects.

The corpus checks 65 complete extractions, eight finalization inputs, and 24 provider lookups. Projects cover HTTP, GraphQL, microservices, WebSockets, nested module prefixes, global prefixes, URI versions, exclusions, and separate applications. Persistent tests compare route records and handler targets after reopening without reading changed live source. Removing bootstrap configuration restores the original route name while retaining its identity; this intentionally fixes a stale-name behavior in the reference's incremental finalizer.

```sh
node --no-wasm-tier-up tests/fixtures/upstream/nestjs/regenerate.mjs /absolute/path/to/reference-checkout
cargo test --locked --lib frameworks::nestjs
cargo test --locked --test nestjs
```
