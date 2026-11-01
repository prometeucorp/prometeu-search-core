# JavaScript and TypeScript receiver and constructor evidence

These snapshots come from the resolver at
[`colbymchenry/codegraph` revision `6aa358ccbae30fbf27318f79ee183212f726f9ba`](https://github.com/colbymchenry/codegraph/tree/6aa358ccbae30fbf27318f79ee183212f726f9ba).
The adjacent MIT license covers the copied regression inputs.

`regenerate.mjs` indexes the original TypeScript/JavaScript regression projects
with the reference implementation, then calls its field-declaration, class-head,
type-alias, generic return, local receiver, and field receiver readers. Additional
projects exercise aliases, renamed and namespace imports, interfaces, ambient
module merging, name fallback, known absent methods, object-literal ownership,
constructor ownership, and Unicode/CRLF source.
Expected records are produced by the reference implementation, independently of
the Rust port.

The 83 projects contain 105 field-call answers, 48 local receiver answers,
six `this.member` function-value references, 421 declaration answers,
266 class/interface/alias heads, 353 generic-return answers, and 55 recorded
import lookup answers. Eighteen additional inputs capture the original ambient
module token scanner's block positions. Direct tests compare every captured
answer. The production gate compares 154 complete selected edges, including 59
constructor edges and three member-value edges, after normal indexing and database reopening.
The reopening check changes the live source first to verify committed-source
queries. Public lifecycle coverage is in `tests/javascript_fields.rs`.
An older-cache regression removes constructor positions and restores legacy
source owners, then verifies identical graph answers from committed source.

The complete-graph gate runs in the normal test suite. It compares every node and
edge, including duplicate record counts, before and after reopening with changed
live files: 1,091 nodes and 1,337 reference edges. Seventy-eight projects match
exactly. Five projects retain the exact compatibility and uncertainty differences
recorded in `uncertainty.json`: an unrelated method on a class without inheritance,
an unavailable import, an unavailable namespace type, an untyped global receiver
with multiple candidates, and the established namespace receiver behavior below.
These are explicit accepted behavior differences, not missing fixtures or ignored
tests. Changing any accepted record or introducing another difference fails the
gate.

The five-file namespace receiver fixture also runs through the public API in
`tests/receiver_resolution.rs`. It preserves syntax-proven exported class and
interface receiver methods through direct namespace imports and renamed namespace
re-exports, including their declared-type references. The reference's line scanner
misses lowercase namespace annotations, falls back to a method-name guess for the
interface, and guesses an unrelated exported `Hidden` type for a private namespace
member. The engine retains its existing export-aware receiver behavior and leaves
private or absent namespace types unresolved. The oracle retains all original
records, with only the specific differing edges documented separately.

Callable values now check imported aliases, class rejection, lexical reachability,
parameter/local shadowing, and ambiguous cross-file names. Ambient module imports
check direct, aliased, default, namespace, and merged declarations. Module
augmentations do not create packages. Type references respect generic parameter
shadowing and qualified namespace ownership. Public lifecycle tests cover ambient
export creation, alias changes, declaration-file renames, deletion, and reopening.

Coverage includes constructor properties, private fields, inherited fields,
wrapped initializers, dependency injection, imported types, nearest supertype
methods, object-valued fields, explicit unresolved outcomes, and canonical source
owners and UTF-16 columns. Local name fallback checks single candidates, word
overlap, lexical scopes, test paths, framework dispatch methods, prototype
assignments, and React lifecycle methods. Public fallback candidates remain
explicitly low confidence. The reference's line-based receiver scan is retained,
including its limitations when sibling or nested functions share a line.
Member values check own and inherited callables, object-literal scope, data-field
rejection, self references, and committed ancestor replacement/removal.
This fixture does not establish parity for every
JavaScript receiver strategy, compiler inference, or malformed-source recovery.

Regeneration requires the reference checkout's development dependencies and
compiled `dist` files. Node is used only to regenerate development fixtures; Rust
tests and the distributed engine do not invoke it:

```sh
node --liftoff-only tests/fixtures/upstream/javascript-fields/regenerate.mjs /path/to/pinned/checkout
cargo test --locked --lib javascript
cargo test --locked --test javascript_fields
```
