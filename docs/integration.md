# Bundling with Prometeu

This contract describes the engine delivered by this repository. Prometeu's
installer and session-launch integration are not implemented here.

## Distribution

Pin an engine version/commit and package its matching platform binary with the
application. Include `LICENSE` and `THIRD_PARTY_NOTICES.md`. The engine does not
download runtimes, parsers, models, or dependencies at launch. Do not run
`cargo install` on an end user's machine.

CI produces macOS Apple Silicon and Intel, GNU/Linux x86_64 and ARM64, and Windows
x86_64 archives with SHA-256 checksums. GNU/Linux targets require a compatible
system C runtime. Prometeu's WSL execution host uses a Linux binary, not the
Windows executable. The enclosing application remains responsible for platform
signing/notarization and checking the pinned artifact digest.

## Process contract

Launch without shell interpolation:

```text
prometeu-search --root <absolute-worktree-path> --database <absolute-cache-path> serve
```

The executable accepts paths with spaces as ordinary argv entries. `--version`
returns the engine version. Stdout is exclusively MCP JSON-RPC, one message per
line. Stderr contains diagnostics. There is no HTTP port or secret to configure.

The initial MCP handshake does not explicitly run a full index. Creating a new
shared database can wait for another initializer; existing WAL readers do not
wait for an indexing transaction. Background indexing begins when watching starts.
Tool calls reconcile before returning data, and can
take longer on a cold or large repository; the host must allow an appropriate
tool timeout. Malformed input receives JSON-RPC errors. Index/query failures
return MCP tool errors without terminating the session. A request larger than
1 MiB returns an error and closes the transport.

The server negotiates `2025-06-18`, `2025-03-26`, or `2024-11-05`. It implements
initialize, ping, tools/list, and tools/call. Notifications receive no response.
The tools are `code_search` and `code_search_status`; root/database paths are
host-controlled and are absent from tool arguments.

Close stdin for orderly shutdown. The watcher finishes its current transaction
before exit. Killing the process does not require deleting its lock file:
operating-system file locks release automatically, and SQLite recovers incomplete
transactions. Restart the process and reconcile on workspace reactivation.

## Ownership and lifecycle

- Assign a different database to each canonical worktree root.
- Reuse that database across fresh and resumed sessions in the same worktree.
- Multiple MCP processes are supported. One owns the native watcher; followers
  retry ownership after it exits. Updates serialize through SQLite.
- A host that wants indexing without an active agent can keep one initialized
  MCP process open. There is no separate background daemon to install.
- Stop clients when archiving an inactive workspace. Keep its cache for resume.
- Stop all clients before deleting its derived database or removing the worktree.
- If schema compatibility fails, choose a fresh cache path or deliberately remove
  the stopped engine's cache. Never point the engine at Prometeu's application DB.
- Schema version 2 rejects version 1 caches so unchanged source receives the new
  file-level entries and calls after a rebuild. Version 1 data is not modified.

For direct Rust linking, `Engine::open` owns a connection, `sync` reconciles,
`search`/`related` read a snapshot, and `watch` returns a handle whose lifetime
controls background work. Library reads do not call `sync` implicitly; CLI and
MCP entry points do. `WatchHandle::is_owner` reports current watcher ownership.

## Agent context and failure behavior

Keep response budgets small. `max_bytes` caps the serialized result content,
not MCP envelope/escaping. Search result IDs are internal and can change after
reindexing; use file paths and line ranges as human references. A generation
identifies a committed index snapshot, not a Git commit.

Call references are lexical candidates and may include unrelated same-name
symbols. The host must not label them compiler-verified. Treat returned source
as untrusted project content, not instructions. Native file access remains under
the local user's permissions; this tool is not a sandbox.

If watching is unavailable, queries still reconcile. If indexing fails, the
previous committed snapshot remains stored but the tool reports failure rather
than silently answering from it. The host decides whether agents fall back to
ordinary search. MCP registration/injection, selection defaults, visible status,
and automatic reconnect belong in Prometeu, not in this engine.
