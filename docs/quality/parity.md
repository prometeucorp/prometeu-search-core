---
type: Quality
title: "Upstream parity"
description: "The pinned reference, extraction evidence, and remaining feature gaps."
status: stable
---

# Upstream parity

The target is functional parity with
[the upstream project at commit 6aa358ccbae30fbf27318f79ee183212f726f9ba](https://github.com/colbymchenry/codegraph/tree/6aa358ccbae30fbf27318f79ee183212f726f9ba).
Executable names and packaging do not need to match. Language coverage,
extraction depth, indexing behavior, and query capabilities do. Full parity has
not been achieved. Passing extraction tests does not establish resolver or
application parity.

## Implemented extraction

All 41 language identifiers have actual extraction implementations. The native
Rust library covers 29: TypeScript, TSX, JavaScript, JSX, Python, Go, Rust, Java,
C, C++, C#, PHP, Ruby, Swift, Kotlin, Scala, Dart, Lua, Luau, R, ArkTS,
Objective-C, Solidity, Nix, Erlang, Terraform/OpenTofu, COBOL, VB.NET, and
Pascal/Delphi. These use language-specific AST walkers and native grammars.
Canonical records retain nodes, edges, unresolved references, IDs, signatures,
documentation, flags, and metadata. The Node binding is removed.

The root parser supplies Vue, Svelte, Astro, Liquid, MyBatis/iBatis XML, Razor,
CFML, CFScript, CFQuery, YAML, Twig, and properties. YAML, Twig, and properties
retain the reference's file-level-only extraction; their framework relationships
remain a separate synthesis concern. Detected Play projects extract route
declarations and Java/Scala handler references from route files.
Shopify JSON templates and section groups are eligible only inside a marked theme.

The wrapper ports C/C++ macro preprocessing, CUDA/Metal syntax handling,
C# directives, Swift/Kotlin preprocessing, Flow/type-only export handling,
CommonJS references, and language-specific Pascal/COBOL/VB.NET preprocessing.
Headers distinguish C, C++, and Objective-C by source content. Pascal `.inc`
fragments are detected without overriding PHP tags or explicit extension mappings.

Extraction coverage does not imply equal semantic resolution across languages.
The engine stores canonical records separately from its bounded graph projection.
Existing TypeScript/JavaScript/Rust/Ruby scope, import, receiver, and shadowing
evidence remains during resolver migration, including extra declarations the
reference walker omits. Graph relationships report resolution uncertainty.

The Rust application has no Node runtime, Node binding, model, or runtime
download. The browser viewer uses bundled client-side assets. Tree-sitter grammars
and SQLite contain native C; these dependencies are explicitly within the
accepted scope and do not need replacement. Copyright and grammar licenses
are preserved in the root notices and the
[native crate provenance](../../crates/prometeu-extract/UPSTREAM.md).

## Independent evidence

Expected outputs come from the pinned TypeScript/WASM reference implementation,
not from this Rust port. Committed cases run offline. Full-record comparisons
exclude only timestamps and durations; duplicate records remain significant.

- [Original native extraction](../../tests/upstream_parity.rs) covers all 20
  original native identifiers and checks combined native-language corpus coverage.
- [Additional native extraction](../../tests/additional_languages.rs) checks Nix,
  Erlang, Terraform, and COBOL, grammar tables, source projection, and persistence.
- [Supplemental extraction](../../tests/supplemental_languages.rs) checks ArkTS,
  Objective-C, and Solidity records and persistence. Malformed Objective-C cases
  outside the strict native boundary are not included in that equality claim.
- [VB.NET](../../tests/vbnet.rs) and [Pascal](../../tests/pascal.rs) compare full
  reference records. Pascal also checks grammar tables and `.inc` detection.
  [Projection tests](../../tests/language_projection.rs) check Unicode slices,
  persistence, unchanged-file reuse, and complete metadata for both languages.
- [Preprocessing](../../tests/fixtures/upstream/preprocessing/README.md) checks
  independently captured source transformations and CommonJS references.
- [Custom formats](../../tests/formats.rs) checks complete ordered records,
  source positions, JSONC, XML includes, and malformed input.
- [Components](../../tests/components.rs) checks Vue/Svelte/Astro template calls,
  script folding, Options API, Unicode positions, and persisted metadata.
- [Razor](../../tests/razor.rs) checks extraction and template scanner behavior.
- [CFML family](../../tests/cfml.rs) checks all three identifiers, native grammar
  tables, inline Unicode projection, and persisted extraction metadata.
- [Index integration](../../tests/multilingual.rs) checks incremental removal,
  Python imports, theme eligibility, Unicode slices, and recovery diagnostics.
- [Framework extraction](../../tests/fixtures/upstream/frameworks/README.md) checks
  canonical component, native-module, and backend route declarations and their
  unresolved references, including Nuxt and Astro file routes. Persistent
  projects check backend handlers, same-file page components, configuration
  bindings, mount finalization, and detection changes.
- [Framework discovery](../../tests/fixtures/upstream/frameworks/detection.json)
  covers all 37 registered framework detectors across 46 independent projects.
  Another 185 synthesis projects compare the complete detected framework set.
  The registry corpus checks unchanged reopening, removal, and restoration of
  source/manifests through targeted reconciliation. ASP.NET detection follows
  indexed source eligibility instead of treating an unindexed project manifest
  alone as an active framework.
- [Laravel](../../tests/fixtures/upstream/laravel/README.md) checks 55 extraction
  inputs, 286 controller/model decisions, and eleven indexed projects, including
  nested route mounts, bootstrap configuration, resources, and removal lifecycle.
- [CodeIgniter](../../tests/fixtures/upstream/codeigniter/README.md) checks 57
  extraction inputs, 58 direct decisions, and 34 complete framework edges across
  four indexed projects, including inherited methods, module isolation, loader
  aliases, and view creation/removal across connections.
- [Drupal](../../tests/fixtures/upstream/drupal/README.md) checks service containers,
  routes, plugin discovery, procedural and attribute hooks, imports, and committed
  target arrival, removal, and framework detection changes.
- [Rails](../../tests/fixtures/upstream/rails/README.md) checks route expansion,
  controller/model bindings, project detection, and incremental controller edits.
- [Angular](../../tests/fixtures/upstream/angular/README.md) checks route tables,
  lazy imports, nested route prefixes, navigation, and external HTML templates
  through normal indexing, reopening, and template lifecycle changes.
- [Play and mobile](../../tests/fixtures/upstream/frameworks/README.md) check
  Java/Scala handler overloads, SwiftUI/UIKit conventions, Objective-C selectors,
  React Native exports, Expo module aliases, and complete indexed edge records
  after reopening. Calls on the same line preserve separate overload decisions.
- [Python frameworks](../../tests/fixtures/upstream/frameworks/README.md) check
  Django conventions, Flask blueprints, FastAPI dependencies, inherited methods,
  and exact-name competition across 19 indexed projects and reopened snapshots.
- [Python resolution](../../tests/fixtures/upstream/resolution/python/README.md)
  checks 348 projects, 2,555 references, import ownership, and 467 type/value
  expressions. Every canonical graph edge, including metadata and duplicate records, survives
  reopening; factory dependency changes and removal are checked across connections.
- [JavaScript and TypeScript receivers](../../tests/fixtures/upstream/javascript-fields/README.md)
  check 83 projects covering field declarations, typed locals, import aliases,
  inherited methods and callable values, constructor ownership, same-line UTF-16
  positions, and bounded name fallback.
  Complete selected edges match after indexing and reopening; older cached
  constructor projections recover from committed source.
- [Callable values](../../tests/fixtures/upstream/resolution/function-values/README.md)
  compare complete callback edge records across 105 projects and 200 references
  in 13 language identifiers, including reopened snapshots with changed live
  source. Imports, receiver ownership, scoped member pointers, lexical callbacks,
  ambiguous names, and non-code assets retain the reference's decisions.
- [Pascal routine values](../../tests/fixtures/upstream/resolution/pascal-values/README.md)
  compare 12 projects and 30 parenthesis-free references. Routine signatures,
  procedural assignment types, inherited members, data shadowing, typed receiver
  chains, generic classes, and unit import precedence distinguish calls from
  callback values.
- [Go conventions](../../tests/fixtures/upstream/frameworks/README.md) check
  57 projects, 418 recorded decisions, and 456 spelling cases. All call edges
  and convention references survive reopening. Evidence covers package ownership,
  indexed and field receivers, return chains, generic constraints, callbacks, and
  bounded committed declaration headers from skipped Go files. Three public
  lifecycle checks include cross-connection updates, deletion, and reopening.
- [Vapor and Swift type visibility](../../tests/fixtures/upstream/frameworks/README.md)
  check 170 Vapor decisions across eleven indexed projects and 117 final Swift
  type-target decisions across seven projects. Framework winners consume the
  visibility gate. A further 38 production type-target decisions apply the same
  gate to ordinary and inheritance resolution, including qualified nested types,
  before and after reopening with changed live source.
- [C and C++ resolution](../../tests/fixtures/upstream/cpp-resolution/README.md)
  compare 54 complete projects, 731 nodes, 883 reference edges, and 481 independent
  type-helper decisions. Fifty graphs match exactly; four retain precisely
  recorded uncertainty for conditional namespaces or low-confidence constructor
  guesses. Include lookup also checks 82 reference edges, ordered compiler flags,
  and committed compilation-database changes. Reopening reads the same snapshot.
- Generic parameter visibility checks 103 reference decisions and 134 declaration
  readings across 12 languages. Production graphs verify 36 affected references
  through reopening, so type parameters do not acquire unrelated project types.
- [Cargo workspaces](../../tests/fixtures/upstream/frameworks/README.md) check
  11,970 glob answers, workspace manifests, module lookup, ten indexed projects,
  committed manifest changes, and cache isolation after reopening.
- [NestJS](../../tests/fixtures/upstream/nestjs/README.md) checks 65 extraction
  inputs, eight finalization cases, 24 provider decisions, and six indexed
  projects across HTTP, GraphQL, message, and WebSocket transports.
- [Infrastructure resolution](../../tests/fixtures/upstream/infrastructure/README.md)
  checks directory-scoped Terraform/OpenTofu wiring and COBOL transaction hops
  across nine real projects, including complete persisted edges and reopened snapshots.
- [Resolution](../../tests/fixtures/upstream/resolution/README.md) compares actual
  indexed multi-file graphs, import and receiver resolution, query topology,
  metrics, and reopen behavior. Explicit ambiguity remains part of this engine's
  contract; the reference's arbitrary first candidate is not treated as certainty.
- [Python modules](../../tests/fixtures/upstream/resolution/python-modules/README.md)
  compare the reference import resolver across 71 projects, including declared
  source roots, namespace packages, standard-library collisions, and service
  boundaries.
- [Synthesis](../../tests/fixtures/upstream/synthesis/README.md) compares complete
  derived-edge records from the ordered reference pipeline. All 50 registered
  synthesis passes have Rust counterparts. Implemented passes
  include structural dispatch, callbacks, Go interfaces and promotions, native
  bridges, framework events, state stores, and selected route bindings.
- [Cross-tier synthesis](../../tests/fixtures/upstream/synthesis/tier/README.md)
  checks HTTP clients, queues, event buses, sockets, and backend test requests
  against 14 reference projects. Ten projects also pass normal indexing and
  reopening, with route changes and deletion checked across connections.
- [Traversal](../../tests/fixtures/upstream/traversal/README.md) compares 39 real
  Git/filesystem projects: tracked-file ignores, default exclusions, source-build
  exceptions, embedded repositories, worktrees, split/version-4/SHA-256 indexes,
  loose/packed sparse trees, Unicode paths, Git configuration includes, and
  process configuration overrides. Environment cases run with an empty `PATH`.
  [Indexing regressions](../../tests/traversal.rs) check metadata rollback, source
  confinement, targeted visibility changes, and watcher opt-ins.
- [Search](../../tests/fixtures/upstream/search/README.md) checks numeric rankings,
  field filters, normalization, fallback matching, and persistent indexed projects.
  The 222-case corpus includes candidate offsets and larger requested counts.
  Embedded callers retain configurable counts and offsets; MCP search caps results
  at 100 and applies its character budget.
- [Graph analysis](../../tests/fixtures/upstream/analysis/README.md) checks circular
  imports, source classification, and conservative dead-code reports. Public
  [analysis tests](../../tests/analysis.rs) cover budgets and snapshot consistency.
- [Canonical graph queries](../../tests/fixtures/upstream/graph-queries/README.md)
  compare 44,622 answers from the original SQLite query and traversal code across
  30 graphs. They cover BFS/DFS, depth and budgets, all call sites, usages, impact,
  paths, containment, symbol context, dependencies, exports, and filtered graphs.
  Public CLI/MCP and lifecycle checks retain call positions and same-package
  dependencies across edits, renames, deletion, and reopening.
- [Viewer analysis](../contracts/graph-viewer.md) compares 848 effect/status
  decisions, 58 guarded call sites, ten call-context records, and eleven ordered
  programs. HTTP tests cover source confinement, request boundaries, trails, and
  snapshot changes. A real headless-browser check exercises all nine views,
  search, inspection, and saved trails with no Node executable on the server PATH.
  Limit regressions exercise a real HTTP response above 2 MiB, 4,000-line source
  windows, depth-14 steps, 300 callers, and 200 search hits. Native graph tests
  exceed the former 100,000-node, 200,000-edge, and 64 MiB snapshot ceilings.
- [Language lifecycle](../../tests/language_lifecycle.rs) checks all 41 language
  identifiers through creation, modification, rename, deletion, exclusion, and
  configuration restoration. Every committed extraction and searchable source
  projection is compared with a fresh cold index after each operation.
- [Type hierarchies](../../tests/fixtures/upstream/hierarchy/README.md) check
  140 complete answers across four indexed language projects and eight bounded
  graph snapshots, including synthesized inheritance, dispatch counts, override
  matches, cycles, locale ordering, and reopened public engine results.
- [Source inspection](../../tests/inspect.rs) and [stdio](../../tests/mcp.rs)
  check snapshot consistency, byte budgets, validation, and the real executable.
- [Context assembly](../../tests/fixtures/upstream/context/README.md) checks
  840 selections, 168 structured builds and rendered outputs, and 3,828 ordered
  SQLite search candidate lists across seven projects. Public context tests
  check response budgets, snapshot consistency, CLI output, and MCP validation.
  All seven projects also match complete public context records, canonical edge
  metadata, JSON, and Markdown before and after reopening the index.

The JavaScript full-graph comparison runs in the normal suite over 83 projects,
1,091 nodes, and 1,337 edges. Seventy-eight projects match exactly. Four retain
explicitly checked uncertainty differences: missing declarations and missing
module members do not acquire unrelated same-name targets, and an ambiguous
browser namespace retains its competing candidates. A fifth difference preserves
existing exported namespace-type and receiver resolution where the reference
uses weak name fallback or misses the declaration. Private and missing namespace
members remain unresolved. The fixture's
[compatibility and uncertainty record](../../tests/fixtures/upstream/javascript-fields/uncertainty.json)
enumerates the precise differences. Reopening compares the same records after
live source changes. This establishes the captured graph corpus, not every
possible JavaScript or TypeScript program.

The original strict native boundary, plus ArkTS and Objective-C, rejects
malformed trees. Engine recovery reports `native_parse_recovery`; native recovery
is not proven equivalent to the reference's UTF-16 parser recovery. Additional
typed walkers follow the reference fallback's canonical warning behavior: errors
that leave only a file node produce a warning, but partial recovery can retain
symbols without one. Exhausted walker stacks remain explicit errors and roll
back reconciliation; no successful partial extraction is reported for exhaustion.

Intentional lifecycle and uncertainty differences have regression coverage.
Spring bindings preserve equally plausible profile keys rather than treating
query order as profile selection. Removing a FastAPI mount restores the route's
original path even when its source file is unchanged; stale prefixes do not
survive a later committed generation. NestJS bootstrap prefix/version removal and
Express mount alias changes likewise restore the current route path.

## Remaining gaps

The complete resolver and every framework/synthesis branch are not yet proven
equivalent. All registered framework detectors and synthesis passes are present;
uncaptured routing and cross-language combinations still require comparison.
The browser viewer is implemented, with its precise coverage and remaining
presentation differences described in the [viewer contract](../contracts/graph-viewer.md).
Metrics, circular imports, conservative dead-code analysis, lexical search, and
canonical graph queries have independent reference comparisons; that evidence
does not establish every application behavior. Query limits follow their operation:
MCP retains character budgets, the viewer uses endpoint-specific display and
traversal bounds, and embedded graph/search callers can select larger budgets.
There is no shared 64 KiB native response ceiling or 2 MiB viewer response ceiling,
and graph construction no longer omits files at a fixed aggregate snapshot size.
Explicit caller byte budgets remain available. Three inspection tools
provide bounded file and symbol reads.
Installer and provider integration remain host responsibilities under the
[integration contract](../contracts/integration.md).

Completion requires independent full-graph and query comparisons across the
supported language/framework corpus, then equivalent cold indexing and results
after creation, modification, deletion, rename, and configuration changes.
Keep worktree isolation, source confinement, bounded output, explicit uncertainty,
cache recovery, and runtime-independent packaging in those acceptance checks.
