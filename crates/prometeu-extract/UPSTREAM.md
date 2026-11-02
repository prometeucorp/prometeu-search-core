# Native extraction provenance

This directory derives from
[colbymchenry/codegraph](https://github.com/colbymchenry/codegraph/tree/6aa358ccbae30fbf27318f79ee183212f726f9ba/codegraph-kernel)
at commit `6aa358ccbae30fbf27318f79ee183212f726f9ba` (MIT).
The upstream copyright and license are retained in `LICENSE`; vendored grammars
retain their own license files. Grammar revisions and local scanner patches are
recorded in `Cargo.toml` and `build.rs`.

The native language walkers and their buffer layout are retained from upstream,
apart from an explicit native error-recovery mode and warning cleanup.
The JavaScript N-API binding is replaced by ordinary Rust functions and a typed
decoder equivalent to upstream `src/extraction/kernel/decode.ts`. `extract`
returns serializable nodes, edges, unresolved references, and extraction errors;
`extract_file` exposes the original flat tables for differential validation of
the original twenty native languages and the ArkTS/Objective-C extensions.
Additional typed walkers are available through `extract`; the raw-buffer API
does not encode their results.
No JavaScript runtime, Node binding, or runtime download is required.

This is the extraction kernel, not the complete upstream application. Upstream
preprocessing, framework hooks, resolution, synthesis, and query behavior live
outside this crate. The original native boundary, plus ArkTS and Objective-C, returns an explicit
`defer:` error for malformed parse trees or exhausted walker stacks, as upstream
does. Additional typed walkers preserve the fallback extractor's canonical
warning behavior for partial trees; they do not share this strict-tree guarantee.
`extract_recovering` allows native Tree-sitter recovery on incomplete trees and
includes a `native_parse_recovery` diagnostic. The scoped recovery mode is local
to the calling thread and never changes subsequent strict calls. Exhausted
walker stacks still fail; partial walks never become successful files. Native
malformed-tree recovery is not claimed equivalent to the upstream WebAssembly
fallback.

The Kotlin grammar license missing from the upstream kernel directory is copied
from [fwcd/tree-sitter-kotlin tag 0.3.8](https://github.com/fwcd/tree-sitter-kotlin/blob/0.3.8/LICENSE).
The build applies the MSVC UTF-8 flag only on MSVC, avoiding an unrelated Clang
linker-option warning. Rust formatting and unnecessary `mut` bindings are also
normalized without changing extraction behavior.

The TypeScript/JavaScript decorator scan walks preceding named siblings directly
instead of rescanning every child of the parent for each declaration. The parent
relationship and decorator ordering are unchanged; the native extraction corpus
checks this path against the pinned reference.

Native grammars and Tree-sitter contain C code. The application API is Rust, but
this dependency is not a Rust-only implementation of every transitive library.

## Additional native grammar pins

The ArkTS, Objective-C, Solidity, and VB.NET ports use native grammar sources
matching the pinned application's WebAssembly artifacts. The differential test
in `tests/supplemental.rs` compares every node-kind and field-name table entry,
alongside the grammar ABI. All four grammar directories retain their MIT license.

| Grammar | Source | ABI / node kinds / fields |
| --- | --- | --- |
| ArkTS | [harmony-contrib/tree-sitter-arkts](https://github.com/harmony-contrib/tree-sitter-arkts/tree/56f7fc288715befe92c734d603f9e6bc3c65d2a8), npm `tree-sitter-arkts@0.2.0` | 14 / 443 / 42 |
| Objective-C | [amaanq/tree-sitter-objc](https://github.com/amaanq/tree-sitter-objc/tree/dea2b2d6a1253c2f0f58b543c10d62edd2f860f2), npm `tree-sitter-objc@2.1.0` | 14 / 570 / 46 |
| Solidity | [JoranHonig/tree-sitter-solidity](https://github.com/JoranHonig/tree-sitter-solidity/tree/b239a95f94cfcc6e7b3e961bc73a28d55e214f02) | 14 / 512 / 33 |
| VB.NET | [govindbanura/tree-sitter-vbnet](https://github.com/govindbanura/tree-sitter-vbnet/tree/538b7087bf80e86004531b392fe1186379c0a2b5), plus the application's [pinned grammar/scanner patch](https://github.com/colbymchenry/codegraph/blob/6aa358ccbae30fbf27318f79ee183212f726f9ba/docs/grammars/tree-sitter-vbnet.patch) | 15 / 498 / 52 |

ArkTS uses the npm package's checked-in parser and scanner, including its shared
`common/scanner.h`. The scanner include changes from `../common/scanner.h` to
`common/scanner.h` to fit this vendored directory layout; grammar behavior is
unchanged. Objective-C and Solidity use their checked-in generated parsers.
VB.NET has no generated parser at the pinned revision: apply the linked patch,
provide the `tree-sitter.json` from its upstream rebuild instructions, and run
`tree-sitter-cli@0.25.10 generate` to produce ABI 15.

SHA-256 of the compiled source files:

| File | SHA-256 |
| --- | --- |
| `arkts/parser.c` | `bbee17f38c8cc0a953ccb0d7c9d98e92b3557296efbd45dfbf748024ce81b621` |
| `arkts/scanner.c` (include adjusted) | `83e1fbc0293d470bc3c34e693f3477ab2df50fc155d0a2a305f4c4764cc1b92f` |
| `objc/parser.c` | `5e49451fb32c3b8668750762ce9a2b5ccc2c09115ebb0a1b1872ab3922a8dfb7` |
| `solidity/parser.c` | `f247baf0dd98d45efe90220a6d49a0c6b2dcedbf2d642bb4169d44bf91e8ef03` |
| `vbnet/parser.c` | `d0c29e9a77416fb24e60c8e7f832d60b0cbbf7f40e375f179b119790b0ff0d78` |
| `vbnet/scanner.c` | `51a81d6616e6499bdce1ee58aacd7798b34623a098a3d7d5a89bd1d61c7842a5` |

ArkTS extends the existing TypeScript walker only where the reference adapter
shares behavior; its structs, decorator metadata, ArkUI attribute chains, and
reference exclusions have dedicated branches. Objective-C extends the C walker
with class implementations, protocols, selectors, properties, message sends,
and its declaration differences. Solidity and VB.NET use typed AST walkers and
the shared node/scope emitter. Oracle records come from executing the pinned
TypeScript/WebAssembly implementation, not from the Rust port; see
`../../tests/fixtures/upstream/supplemental/README.md`.


## Pascal and CFML grammar pins

Pascal uses `tree-sitter-pascal = 0.10.2`, whose published crate records
commit `2f28b717be47cf592241e1b7bec3b2b906f59148` of
[Isopod/tree-sitter-pascal](https://github.com/Isopod/tree-sitter-pascal).
Its parser is byte-identical to revision
`042119eca2e18a60e56317fb06ee3ba5c32cb447`; the parser SHA-256 is
`d7a5f5c04880fac79e0eb1841cbbc00d7e305863efe56d83c9754fba70385329`.
Every native node-kind and field-name entry matches the reference WASM grammar.
The Pascal port includes preprocessing, AST extraction, typed local and routine
reference recovery, and the DFM/FMX form extractor. Its
[reference corpus](../../tests/fixtures/upstream/pascal/README.md) independently
checks complete output records, prepared source, and include-file detection.

The root parser uses `tree-sitter-cfml = 0.26.30` for CFML, CFScript, and CFQuery,
at commit `776f83d82440aeb05d163bc454e88890a8a99870`. All three checked-in WASM
artifacts match the reference checkout byte for byte. Native parsers, scanners,
and shared headers match that tag. The
[CFML corpus](../../tests/fixtures/upstream/cfml/README.md) compares grammar
tables and complete extraction records. Both grammar projects use the MIT
license, retained in the root third-party notices.
