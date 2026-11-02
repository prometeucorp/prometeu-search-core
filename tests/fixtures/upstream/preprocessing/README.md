# Upstream preprocessing reference cases

`cases.json` records outputs from upstream commit
`6aa358ccbae30fbf27318f79ee183212f726f9ba`, under its MIT license. The root
`THIRD_PARTY_NOTICES.md` records upstream attribution. Each case includes its
upstream test path and source line, or a descriptive name for an additional
Unicode/dialect case.

Expected `prepared` values were produced by
`EXTRACTORS[language].preParse(source, path)` from
`src/extraction/languages/index.ts`, using the original TypeScript implementation.
Expected `commonjs` values were produced by `commonJsRequireRefs(path, source,
language)` from `src/extraction/commonjs-requires.ts`. These are reference outputs,
not outputs captured from this Rust port.

The capture includes multiline string literals from the upstream macro, raw
string, Flow, type-only re-export, and CommonJS tests, plus original kernel
fixtures. C/C++ samples are checked under both dialects. Additional cases cover
CUDA launch syntax, Metal attributes, Objective-C annotations and Foundation
enums, UTF-16 columns, CRLF, and non-ASCII source.

Run the offline comparison with:

```sh
cargo test --locked --lib parser::preprocess
```

Updating expected values requires running the pinned upstream implementation.
Do not regenerate them from the Rust implementation under test.
