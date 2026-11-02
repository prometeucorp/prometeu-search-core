# Upstream extraction oracle

These fixtures pin CodeGraph commit
[`6aa358ccbae30fbf27318f79ee183212f726f9ba`](https://github.com/colbymchenry/codegraph/tree/6aa358ccbae30fbf27318f79ee183212f726f9ba).
The source files come from that revision under its [MIT license](LICENSE), with
the upstream product name replaced by `prometeu` in the fixture inputs. The
TypeScript/WASM walker produced the expected results from those sanitized inputs
directly, bypassing native addon routing. The Rust implementation did not generate
them, and the expected identifiers were not edited by hand.

[`cases.json`](cases.json) records each original upstream path, the path passed to
the extractor, input SHA-256 hashes, and expected output. The corpus covers all
20 upstream native language identifiers. It uses the existing extraction torture
fixtures, the upstream kernel loader for TypeScript, and the upstream named-object
literal regression with non-ASCII and astral characters in LF and CRLF forms.

[`upstream_parity.rs`](../../upstream_parity.rs) compares every node, edge, and
unresolved-reference field as multisets. Duplicate records and optional fields
matter. Only node timestamps and extraction duration are ignored. The checked-in
corpus contains 22 cases, 1,400 nodes, 1,445 edges, and 1,773 references.

## Scope

This checks the native extraction boundary. It does not establish parity for
whole-project resolution, framework synthesis, indexing, or queries. C and C++
need upstream preprocessing; their original sources remain in `source/`, while
`prepared/` contains the exact inputs sent to the native kernel. The original
sources produce the expected results through the upstream TypeScript walker.
This distinction keeps missing preprocessing visible rather than treating it as
native extraction behavior.

The corpus uses syntax-valid files. Malformed files and native deferral behavior
need separate recovery tests; a passing corpus is not evidence that a missing
fallback is safe.

## Run

The normal Rust test suite needs no JavaScript runtime or network access:

```sh
cargo test --locked --test upstream_parity
```

## Regenerate

Regeneration is a development-only operation. Use a separate upstream checkout
at the exact revision above. Inspect its package scripts before executing them.
In that checkout, install the locked development dependencies without lifecycle
scripts, transpile the reference implementation, and copy its checked-in grammars:

```sh
npm ci --ignore-scripts --no-audit --no-fund --workspaces=false
./node_modules/.bin/tsc --noCheck --declaration false --declarationMap false
npm run copy-assets
```

From this repository, run:

```sh
node tests/fixtures/upstream/regenerate.mjs /absolute/path/to/reference-checkout
cargo test --locked --test upstream_parity
```

The script rejects a different upstream commit and constructs the TypeScript/WASM
walker directly even if a native addon exists. Review snapshot changes alongside their
upstream source changes; never regenerate expected results from the Rust port.

The configuration, dead-code, and viewer generators also verify the pinned
revision before loading reference code. Their rejection check runs without
reference dependencies:

```sh
node --test tests/fixtures/upstream/regeneration.test.mjs
```
