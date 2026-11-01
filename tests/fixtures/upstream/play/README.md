# Play reference fixtures

Pinned reference: [source revision](https://github.com/colbymchenry/codegraph/tree/6aa358ccbae30fbf27318f79ee183212f726f9ba).

`cases.json` records 69 extraction inputs and six projects indexed by the original
implementation. The projects contain 412 direct resolver answers, 182 overload
retargeting answers, 125 argument type answers, and 33 complete persisted Play
framework edges. Helper coverage adds 160 signatures, nine argument lists, and
20 UTF-16 call positions. Expectations come from the original implementation,
including its own Java type and visibility readers, not from the Rust port.

Regenerate with `node --liftoff-only regenerate.mjs <pinned-checkout>` after building
the reference's development dependencies. Runtime Rust tests do not use Node.
The source inputs and expected records retain upstream semantics; timestamps are
removed. The included MIT license covers the derived fixtures.
