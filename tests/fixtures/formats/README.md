# Custom-format reference results

The expected results in `cases.json` come from the TypeScript implementation of
[CodeGraph at 6aa358cc](https://github.com/colbymchenry/codegraph/tree/6aa358ccbae30fbf27318f79ee183212f726f9ba),
with native extraction disabled. The fixtures cover Liquid tags and declarations,
Shopify JSONC section references, MyBatis/iBatis XML statements, and upstream's
zero-node YAML, Twig, and Properties tracking behavior. XML robustness cases
follow upstream `__tests__/mybatis-extractor-robustness.test.ts`; Liquid cases
exercise the contracts in `src/extraction/liquid-extractor.ts` and
`__tests__/extraction.test.ts`. The upstream [MIT license](LICENSE) is retained.

All fields except node timestamps and extraction duration are preserved, including
duplicate nodes, UTF-16 columns, and optional metadata. These snapshots establish
extraction behavior only; they do not prove framework resolution or file scanning.

Prepare the pinned upstream checkout as described in
[the native oracle](../upstream/README.md), then regenerate independently:

```sh
node tests/fixtures/formats/regenerate.mjs /absolute/path/to/reference-checkout
```

Normal Rust tests read the snapshots without Node or network access. Never derive
the expected results from the Rust implementation being tested.
