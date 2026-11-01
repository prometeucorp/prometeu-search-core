# Search reference corpus

Pinned upstream revision: `6aa358ccbae30fbf27318f79ee183212f726f9ba`.

`regenerate.mjs <prepared-checkout>` executes the original query parser,
normalization helpers, path recognizer, and real SQLite `QueryBuilder`.
`cases.json` contains 108 normalization inputs, 954 name scores, 198 path scores,
162 bounded edit distances, 156 path-recognition cases, and 222 ranked queries
over 217 canonical nodes. The ranking comparisons include the numeric BM25
scores, exact-name supplementation beyond the FTS candidate window, selective
filters, Unicode, wildcard substring fallback, typo fallback, project names,
and deprioritized paths.

`regenerate-projects.mjs <prepared-checkout>` runs the original project indexer
for three projects spanning TypeScript, Python, Rust, Go, Vue, and Svelte. Its
99 query results are compared after native extraction and persistent indexing.
The tests overwrite live files and reopen the database before repeating the
comparisons; returned snippets must remain part of the committed source.

The ranking corpus includes candidate offsets, counts above fifty, Unicode field
filters, exact-name supplementation, and offset-independent fuzzy fallback.
Empty raw queries retain the reference's full five-times-overfetched candidate
page. Direct library search has no fixed count or byte ceiling; MCP applies its
own count and character budgets. These fixtures establish lexical search and
pure helper parity, not complete exploration, graph ranking, or prompt-hook
behavior.
