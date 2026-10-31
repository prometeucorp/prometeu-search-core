# Prometeu Search

Read [architecture](ARCHITECTURE.md) before changing boundaries and the
[integration contract](docs/contracts/integration.md) before changing the
executable or MCP contract. When a document's location is unknown, start at
[the bundle map](docs/README.md), then its area index and relevant concept.
Open known paths directly and follow their dependencies.

Keep this engine standalone and independent of Tauri, Prometeu state, model
providers, and network services. Preserve worktree isolation, bounded responses,
explicit resolution uncertainty, and cache recovery. Use English for code and
documentation. Preserve unrelated local changes.

Run the closest tests, then `cargo fmt --all --check`,
`cargo clippy --all-targets --locked -- -D warnings`, and `cargo test --locked`.
Update contract documentation for externally visible changes. Commits follow
Conventional Commits. Do not create tags or publish releases without a request.

`docs/` is the OKF knowledge bundle; this file remains the standing instructions.
New or substantially changed concepts have YAML `type`, `title`, `description`,
and `status`, plus a rendered link in their folder's `README.md`. Folder indexes
and root maps are exempt. Keep draft proposals separate from current guidance;
follow the [format and lifecycle](docs/decisions/README.md).
Run `cargo test --locked --test docs` for documentation checks; the full test
suite includes them. Keep local links valid and update guidance with its code.
