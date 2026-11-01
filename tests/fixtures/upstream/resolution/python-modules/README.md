# Python module resolution oracle

`cases.json` records 2,662 module-resolution queries across 71 independently
captured projects. Expected paths come from the pinned reference resolver at
revision `6aa358ccbae30fbf27318f79ee183212f726f9ba`, never from the Rust port.

The generator reads literal project inputs from the reference Python resolution
tests and adds packaging configurations for setuptools, Poetry, Hatch, and
`setup.cfg`. It captures canonical declarations and import mappings alongside
resolver outputs. Cases cover package/module precedence, namespace segments,
relative imports, nested service roots, `src` layouts, ambiguous roots, tests
and vendor scopes, script imports, and standard-library collisions.

`regenerate.mjs <prepared-checkout>` verifies the revision and exposes the
original private resolver through a development-only module wrapper. It does
not rewrite resolver logic. Committed Rust tests need neither Node nor network
access. The adjacent [license](LICENSE) covers the reference-derived fixtures.

Run `cargo test --locked --lib python_modules` for the offline comparison.
