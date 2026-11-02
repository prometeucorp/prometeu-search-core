# Python framework resolution fixtures

Pinned revision: `6aa358ccbae30fbf27318f79ee183212f726f9ba`.

`regenerate.mjs <prepared-checkout>` runs the original Django, Flask, and
FastAPI resolvers, import resolver, exact-name matcher, and complete resolver.
It initializes the original grammars explicitly and indexes each temporary
project through the reference API. Rust results never generate expected output.

The corpus contains 19 projects and 948 raw or synthetic reference queries.
Each query captures the independent framework choices, import and name-match
choices, and final selected target, confidence, and strategy. Project snapshots
also retain complete canonical nodes and edges, module-path answers, and
supertype lookup answers.

Cases cover conventional model, view, form, router, blueprint, and dependency
directories; local declarations and imports; nested classes and functions;
parameters and local variables; test suites versus test support; manual mocks;
iterable methods; inherited methods; competing frameworks; and framework
selection of a base class before inherited member resolution. Conflicting
imports are checked against instance, class, super, and multiline receiver
calls.

The resolver unit test checks each helper and selected answer. The canonical
graph child test compares complete stored edges for the handled names, including
ordinary import and exact-name winners. `tests/python_frameworks.rs` independently
checks the public graph projection. Both indexing tests reopen the database
after replacing live source files, proving that queries use the committed
snapshot.

The binding and source indices are built once per graph snapshot. Scope and
candidate work has an explicit cumulative bound, and source reads remain
confined to committed files. The Python-specific tests establish this strategy
boundary; they do not claim complete parity for the general multilingual name
matcher, foreign-language visibility rules, or every fallback strategy.
