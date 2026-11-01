# Configuration oracle

`cases.json` captures project-config validation and path matching from the
[MIT-licensed reference](https://github.com/colbymchenry/codegraph/tree/6aa358ccbae30fbf27318f79ee183212f726f9ba),
pinned at `6aa358ccbae30fbf27318f79ee183212f726f9ba`. Copyright 2026 Colby
Mchenry; the parent fixture directory contains the upstream [license](../LICENSE).

Run `node regenerate.mjs /path/to/reference-checkout` after compiling the pinned
TypeScript checkout and installing its development dependencies. The script uses
the reference's exported configuration loaders and its installed `ignore`
matcher. It writes only temporary configuration files and the resulting fixture;
the Rust implementation never generates expected output. No Node runtime is
needed to build, run, or test the engine.

The corpus covers extension normalization, colliding and duplicate JSON keys,
ECMAScript property order and whitespace, invalid fields, malformed JSON,
case-insensitive matching, anchored and recursive patterns, and parent-directory
negation. It does not claim filesystem discovery or full search-ranking parity.
