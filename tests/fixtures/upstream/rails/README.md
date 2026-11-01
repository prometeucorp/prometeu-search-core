# Rails framework oracle

`cases.json` retains 68 complete extraction cases, 87 convention-resolution and
ownership queries, and three indexed projects from the pinned
[reference revision](https://github.com/colbymchenry/codegraph/tree/6aa358ccbae30fbf27318f79ee183212f726f9ba).
It includes the original scoped Rails and only/except regression projects plus
focused flat routing, singular resource, nested scope, Unicode, malformed input,
and controller/model/helper/service lookup cases.

The development generator invokes the actual reference framework extractor,
indexes real temporary projects, and queries its resolver. Expectations retain
all extraction fields except timestamps and all outgoing route-edge metadata.
Run `node --liftoff-only tests/fixtures/upstream/rails/regenerate.mjs <prepared-checkout>`
to regenerate. Rust tests use the checked-in JSON without JavaScript.

Persistent tests compare complete canonical route metadata (with documented SQL
readback defaults) and outgoing edges, including after reopening while source
files change outside the index. A separate lifecycle test activates and removes
Rails through the Gemfile and changes a controller action through another
connection, verifying committed source and graph-cache invalidation.
The original MIT notice is retained in [LICENSE](LICENSE).
