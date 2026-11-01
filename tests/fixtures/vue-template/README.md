# Vue template scanner reference

The expected calls come from the pure TypeScript scanner at
[upstream commit 6aa358cc](https://github.com/colbymchenry/codegraph/tree/6aa358ccbae30fbf27318f79ee183212f726f9ba).
The generator executes only the `vueTemplateCalls` unit-test group to capture its
input strings, then adds focused Unicode, scope, comments, interpolation, and
template-boundary cases. It does not execute the separate indexing suites.
The upstream [MIT license](LICENSE) is retained.

Expected values contain each call name and its UTF-8 byte offset in the source.
The reference uses UTF-16 offsets; the generator converts them with the byte
length of the original source prefix. Order and duplicate calls are retained.

Prepare the pinned checkout as described in [the native oracle](../upstream/README.md),
then regenerate with:

```sh
node tests/fixtures/vue-template/regenerate.mjs /absolute/path/to/reference-checkout
```

Rust tests consume the checked-in results without a JavaScript runtime.
