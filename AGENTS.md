# Prometeu Search

Read `ARCHITECTURE.md` before changing boundaries and `docs/integration.md`
before changing the executable or MCP contract.

Keep this engine standalone and independent of Tauri, Prometeu state, model
providers, and network services. Preserve worktree isolation, bounded responses,
explicit resolution uncertainty, and cache recovery. Use English for code and
documentation. Preserve unrelated local changes.

Run the closest tests, then `cargo fmt --all --check`,
`cargo clippy --all-targets --locked -- -D warnings`, and `cargo test --locked`.
Update contract documentation for externally visible changes. Commits follow
Conventional Commits. Do not create tags or publish releases without a request.
