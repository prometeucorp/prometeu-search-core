# Generic parameter and Scala scope evidence

These records were generated from the pinned reference revision
`6aa358ccbae30fbf27318f79ee183212f726f9ba`. The adjacent MIT license covers
copied project regression inputs. Expected decisions come from the reference,
independently of the Rust implementation.

The corpus contains 12 indexed projects across 12 languages, 103 recorded final
gate decisions, and 134 declaration-head readings. It covers type parameter
shadowing, Java and Kotlin parameters before declaration names, Go method
receiver parameters, C++ template defaults, Scala curried value parameters,
Scala nested member visibility, inheritance, and imported members.

Direct tests compare every recorded decision and declaration reading. Production
tests check the references the gate changes before and after reopening an index
whose live source has changed. These gates establish the recorded rules, not
complete resolution equivalence for every language.

The production gate applies during both inheritance and ordinary resolution.
The override synthesizer reuses the same declaration reader.

Regeneration requires the reference checkout's installed development dependencies
and compiled `dist` directory. Node is only a development fixture generator;
Rust tests and distributed binaries do not invoke it.

```sh
node --no-wasm-tier-up tests/fixtures/upstream/type-parameters/regenerate.mjs /path/to/pinned/checkout
cargo test --locked --lib type_parameters
cargo test --locked --lib generic_parameter_decisions
```
