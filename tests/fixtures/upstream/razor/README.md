# Razor reference records

The 56 full extraction cases and 31 script-scanner cases in `cases.json` were
captured independently from the pinned upstream TypeScript implementation and
its WebAssembly C#/JavaScript/TypeScript walkers. They cover directives, Blazor
components, generic arguments, embedded C#, scripts, Razor control statements,
emitted script, comments, and Unicode offsets.

Run `node tests/fixtures/upstream/razor/regenerate.mjs /path/to/prepared/upstream`
to recapture the pinned revision. The Rust tests compare complete record
multisets without timestamps and also index all fixtures. Node and WebAssembly
are required only for this development-time capture.

See the enclosing [license](../LICENSE) and
[provenance](../../../../crates/prometeu-extract/UPSTREAM.md).
