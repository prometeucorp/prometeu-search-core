# C and C++ resolution evidence

These records were generated from pinned reference revision
`6aa358ccbae30fbf27318f79ee183212f726f9ba`. The adjacent MIT license covers
copied regression project inputs. Expected decisions and graphs come from the
reference implementation, independently of the Rust implementation.

The corpus contains 54 indexed projects from namespace qualification, declared
receiver aliases, global alias targets, inherited nested aliases, macros, and
constructor suites: 731 canonical nodes and 883 reference edges. It includes
481 recorded type lookup decisions, with the
actual declaration visibility sets passed by the calling resolver. The direct
helper gate compares every recorded alias, class, enum, written declaration,
and template parameter decision.

Production resolution reads committed sources and uses native syntax trees for
receiver declarations. It follows local and class aliases, resolves declared
receivers through nested and inherited types, distinguishes global qualification,
resolves explicit constructor sites by overload arity, and walks macro includes
with three-valued build conditions. Alias chains, syntax traversal, source scans,
include recursion, and directive evaluation have explicit budgets.

The normal complete graph test compares every node and edge record, including
duplicate multiplicity, before and after reopening an index whose live sources
have changed. Fifty projects match exactly. Four projects have only the specific
edge differences recorded in `uncertainty.json`: one competing conditional
namespace and three copies of a bare constructor call with competing overloads.
Rust leaves the conditional targets unresolved and retains both constructor
candidates with low confidence and explicit ambiguity. No project is exempted
from the complete graph comparison, and reference records remain unchanged.

Regeneration requires the pinned checkout's installed development dependencies
and compiled `dist` directory. Node is only a development fixture generator;
Rust tests and distributed binaries do not invoke it.

```sh
node --no-wasm-tier-up tests/fixtures/upstream/cpp-resolution/regenerate.mjs /path/to/pinned/checkout
cargo test --locked --lib cpp_alias_helpers
cargo test --locked --lib cpp_complete_graph_audit
```
