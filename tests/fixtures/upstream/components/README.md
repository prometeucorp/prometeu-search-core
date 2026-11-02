# Component extraction reference cases

The 101 cases in `cases.json` contain results captured from the TypeScript
single-file component extractors at upstream revision
`6aa358ccbae30fbf27318f79ee183212f726f9ba`. Their script blocks delegate to
the WebAssembly tree-sitter walker. The native Rust extractor does not generate
these expected records.

The cases cover Vue Options API members and template handlers, TSX scripts,
Svelte instance and module scripts, Astro frontmatter, self-closing scripts,
script/style exclusions, aliases, and Unicode with both LF and CRLF. Shared
component samples are exercised across all three formats. Only timestamps and
elapsed durations are omitted; tests compare complete records as multisets.

To regenerate, prepare the pinned upstream checkout with its Node dependencies
and compiled `dist/extraction` modules, then run:

```sh
node tests/fixtures/upstream/components/regenerate.mjs /path/to/upstream
```

The Rust runtime and tests need neither Node nor WebAssembly. See the enclosing
[license](../LICENSE) and [upstream provenance](../../../../crates/prometeu-extract/UPSTREAM.md).
