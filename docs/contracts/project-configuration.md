---
type: Contract
title: "Project configuration"
description: "Shared extension mappings, index selection, ranking preferences, and atomic configuration updates."
status: stable
---

# Project configuration

Commit `prometeu-search.json` at the indexed worktree root to share project
settings. Configuration belongs to that root and never affects another engine.

```json
{
  "extensions": { ".dota_lua": "lua", ".tpl": "php" },
  "include": ["Tools/"],
  "includeIgnored": ["repos/"],
  "exclude": ["Tools/generated/"],
  "deprioritize": ["examples/"]
}
```

`extensions` maps the final filename extension to an identifier in
`parser::LANGUAGES`. Keys are trimmed, lowercased, and given a leading dot when
absent. Multipart extensions and path separators are invalid. Valid mappings
override the built-in table. Invalid entries produce a diagnostic and are skipped.
Language names are case-sensitive. Special Play and Erlang filenames retain
their language, and Shopify theme JSON retains Liquid extraction. Flow detection
and C-header detection still apply; an explicit `.inc` mapping suppresses the
Pascal include heuristic.

The four path lists use case-insensitive gitignore syntax against root-relative
paths. Whitespace is trimmed and invalid list entries are warned and skipped.
Pattern order and negation matter: a negated child cannot restore an excluded
parent directory unless the parent is also restored.

- `include` restores matching source despite ignore files. Its literal directory
  prefixes limit the extra traversal. Explicit `exclude` and built-in dependency,
  output, Git, and engine-cache exclusions still win.
- `includeIgnored` discovers embedded repositories below opted-in prefixes; it
  does not restore ordinary ignored files. Each restored repository uses its own
  ignore rules. Git worktree pointers are skipped, while submodule pointers are
  accepted. Discovery examines at most 2,000 directories and four levels below
  each ignored directory. This option applies when the indexed root belongs to a
  Git repository; filesystem fallback does not revive ignored repositories.
  Source and repository traversal never follow symlinks.
- `exclude` removes matching paths, including paths selected by either inclusion
  rule. It applies to directories before their contents are visited.
- `deprioritize` keeps matching files indexed and searchable. It applies the
  reference score penalty of 15 points once and multiplies the name-match bonus
  by 0.75 before rounding. This shares the penalty used for inferred test,
  example, and fixture files. An explicit test/spec query can waive the inferred
  test penalty, but never the configured preference.

A missing configuration uses defaults. Malformed JSON emits a diagnostic and
uses defaults. An unreadable configuration, a symlink, a non-regular file, or a
file larger than 1 MiB fails reconciliation without changing the committed
snapshot. Source reads retain the same confinement and rollback guarantees.

Every reconciliation checks configuration contents, including targeted updates
and metadata refreshes. Any content change, removal, or creation forces a full
reconciliation and reparses eligible source even if its contents are unchanged.
The effective settings fingerprint and content fingerprint commit with the new
index generation. Formatting-only edits change the content fingerprint while
preserving the effective fingerprint. Watchers observe configuration changes and
edits to custom extensions.

Go module configuration is also part of the committed resolver snapshot.
Eligible `go.mod` files are retained as root-relative source text, with a 1 MiB
per-file limit and an 8 MiB aggregate limit. Changes or removals invalidate the
graph generation independently of source changes. Queries never read live module
configuration.

Project manifests and workspace directory existence also commit with the index.
Package metadata (`package.json`, `pnpm-workspace.yaml`, and `oh-package.json5`)
and framework manifests use secure bounded discovery: dependency roots to depth
six and 8,000 directories, workspace glob roots to depth eight and 10,000
directories, and literal workspace targets. Captured text has a 1 MiB per-file
and 8 MiB aggregate limit. Directory existence includes empty dependency targets.
Manifest or captured directory changes advance the graph generation; queries use
only the committed snapshot.

Cargo workspace discovery additionally follows root `[workspace] members`,
including literal targets beyond the ordinary manifest scan and glob expansion
through six directory levels below each static prefix. Package names and their
hyphen-to-underscore aliases map to indexed `src/lib.rs` or `src/main.rs` files.
Glob matching follows the pinned reference, including brace, character-class,
extglob, and UTF-16 behavior; patterns are limited to 4,096 UTF-16 units and 64
nested groups. This discovery shares the manifest limits above and a 50,000
listing budget. Member manifest edits participate in the same atomic generation
change. Framework module resolution does not replace general Rust name and scope
resolution.

C/C++ include search reads the first available `compile_commands.json` from the
project root, `build/`, `cmake-build-debug/`, `cmake-build-release/`, or `out/`.
These files share the committed manifest snapshot and its size and confinement
limits, including when build directories are excluded from source indexing.
`arguments` arrays or shell-quoted `command` strings supply ordered `-Ipath`,
`-I path`, and `-isystem path` roots. Commands are parsed as text and never run.
Include roots resolve against each compilation directory; paths outside the
project are excluded. A valid database with no include flags keeps an empty
search list. Missing or malformed databases use convention roots (`include`,
`src`, `lib`, `api`, `inc`) and top-level directories containing headers.

Exact sibling headers take precedence, followed by root-relative and include-root
searches using the source language's extension order. Unresolved paths may retain
an explicitly lower-confidence filename candidate, with `resolvedBy: "file-path"`
and confidence 0.7 for a unique basename alone. Such a candidate does not prove
that the compiler can resolve that include. Configuration changes and removal
advance the committed generation; graph queries never read a live compilation
database.

Quoted local C/C++ includes used by function-pointer tables and explicit Angular
component `templateUrl` files also commit with the index, including otherwise unindexed `.def`, `.inc`, and `.tbl` files. Discovery
follows two include levels and explicitly declared template paths under the pinned
source root, with shared limits of 1 MiB per file, 8 MiB total, 4,096 auxiliary files, and 100,000 auxiliary source probes. These inputs remain separate from indexed graph files. Include and template edits and removals advance the generation during reconciliation. Failed reads, symlinks, and
exceeded budgets roll back the transaction; queries retain the prior snapshot.

Git visibility is read natively from version 2, 3, and 4 indexes, including split
indexes, SHA-1/SHA-256 repositories, sparse indexes, submodules, and embedded
repositories. Sparse directories are expanded from loose or packed tree objects
only when materialized on disk. Root ignore rules and the default catalog apply
to tracked files; a nested ignore rule can leave an already tracked file visible.
Git metadata and scope fingerprints are checked before targeted reconciliation,
so index changes cannot leave previously visible files behind. No Git executable
is required at runtime.

The default catalog covers dependency, build, tool-cache, and Android resource
directories. A root ignore negation can opt a default directory back in. JVM
packages named `build` beneath source roots and supported handwritten source
inside `src/**/build` retain the reference exceptions; generated declaration
files and non-source build outputs stay excluded unless explicitly restored by
root ignore rules. `include` still cannot override defaults by itself.

Git configuration uses a native parser with bounded global, repository, and
worktree configuration snapshots, ordered includes, and `gitdir`, `onbranch`,
and remote-URL conditions. Exclude files retain their Git precedence. Git
environment overrides support `GIT_CONFIG_COUNT` with indexed keys and values,
and both quoted `GIT_CONFIG_PARAMETERS` formats. Environment include directives
and remote URLs participate in conditional configuration. Their bounded text and
included contents join the visibility fingerprint; malformed parameters abort
reconciliation. Parameter strings are decoded without evaluating a shell. Git
metadata pointers may refer to worktree/submodule metadata outside the source
root; indexed source always remains inside the pinned source handle. Invalid
indexes, failed metadata reads, and exceeded snapshot budgets abort reconciliation
without replacing the committed index. Index reads are capped at 64 MiB; sparse
trees at 64 MiB each and 128 MiB total, with bounded path storage and nesting.

The independent traversal corpus records the pinned reference's actual file
lists and Git index/object fixtures. Configuration matching and persistent
index/watch tests cover the atomic behavior above; see the
committed parity fixtures for remaining application migration work.
