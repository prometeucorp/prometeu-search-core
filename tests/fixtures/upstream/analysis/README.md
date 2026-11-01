# Graph analysis reference data

`cycles.json` contains 105 import graphs evaluated by the pinned reference's
iterative strongly connected component implementation. It covers self edges,
disconnected groups, duplicate edges, chains, and deterministic generated graphs.
The Rust test also exercises a 30,001-file graph without recursive stack growth.

`classification.json` captures the reference's test-path, non-production-path,
generated-path, and generated-content decisions. Inputs include literal cases
from its generated-file and conservative dead-code suites, plus UTF-16 header
boundaries, comment markers, source strings, and minification cases.

Regenerate with `node tests/fixtures/upstream/analysis/regenerate.mjs /path/to/prepared/upstream`.
The generator checks revision `6aa358ccbae30fbf27318f79ee183212f726f9ba` and uses
its compiled JavaScript; Rust never supplies expected results. Normal tests use
the committed JSON and require no JavaScript runtime. The reference license is
retained in [LICENSE](LICENSE).
