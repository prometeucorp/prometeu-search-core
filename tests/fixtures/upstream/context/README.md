# Context assembly reference fixtures

Pinned reference: [source revision](https://github.com/colbymchenry/codegraph/tree/6aa358ccbae30fbf27318f79ee183212f726f9ba).

The generator indexes seven projects with the original implementation and invokes
its actual context builder, SQLite query methods, graph traversal, and formatters.
Two input projects derive directly from the original context regression tests.
Additional projects exercise hierarchy budgets, compound and Unicode matching,
generated source, COBOL copybooks, file diversity, and dominant-file demotion.

The corpus records 945 find selections, 189 structured context builds with
Markdown/JSON/tree output, and 4038 ordered search candidate lists and scores.
It also records 31 symbol-token inputs, eight byte-formatting inputs, and 59
synthesized-hop descriptions. Node timestamps are omitted. Expected Markdown
substitutes the engine's public tool names and product name only.

The pure tests replay the original candidate answers, then compare selected
records, order, roots, confidence, snippets, statistics, and rendered output.
The SQLite test separately indexes the same sources through Rust and compares
all original candidate answers. The full integration test compares complete
context records and output through the public engine before and after reopening;
live source files are changed before reopening to prove snapshot isolation.
Absent optional Rust visibility/provenance values and SQL null values are
normalized during structural comparisons. JSON formatter output retains the
reference's explicit null visibility.

Regenerate with `node --liftoff-only regenerate.mjs <pinned-checkout>` after
preparing the reference's development dependencies. Rust tests do not run Node.
The included MIT license covers the derived fixtures.
