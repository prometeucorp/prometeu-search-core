# Cross-tier and test-request synthesis oracle

The pinned reference revision is `6aa358ccbae30fbf27318f79ee183212f726f9ba`.
Run `regenerate.mjs <prepared-checkout>` to index the reference's complete
cross-tier example, its literal Spring/Laravel test projects, and additional
fanout, import, Unicode, and ambiguous-route cases. The generator then invokes
the original synthesis passes against their actual project resolution context.

`cases.json` contains 14 projects and 43 complete expected edges. It also records
180 independent source-reader and path-matching inputs by exposing the unchanged
reference helpers through a development-only module wrapper. Expected records
are never produced by Rust. Only node timestamps are omitted.

Coverage includes HTTP verbs and client factories, imported client instances,
base URLs, template holes, parameter/catch-all scoring, queue producers and
consumers, event globs, socket direction, fanout limits, MockMvc/WebTestClient/
RestTemplate/RestAssured requests, Laravel helper chains, Pest calls, and
interpolated or concatenated request paths.

`cargo test --locked --lib synthesis::tier` compares complete pass outputs and
reader values offline. `cargo test --locked --lib tier_tests` additionally checks
six projects through normal indexing and reopening, then changes and removes
HTTP routes through a second connection to verify cache invalidation. The
remaining projects validate the synthesis boundary independently of framework
extractors still being ported. [License](LICENSE).
