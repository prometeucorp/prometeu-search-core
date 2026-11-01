---
type: Contract
title: "Graph queries"
description: "Static resolution, bounded exploration, graph analysis, and result uncertainty."
status: stable
---

# Graph queries

The engine combines lexical retrieval with static source relationships. It runs
locally without a model, compiler service, network connection, or provider
integration. The [integration contract](integration.md) defines process and cache
ownership; [architecture](../../ARCHITECTURE.md) describes extraction and recovery.

## Entry points

`Engine::graph_query(&GraphQueryOptions)`, `graph <operation>` on the CLI,
and the `code_graph` MCP tool expose canonical graph operations. The operations
are `bfs`, `dfs`, `callers`, `callees`, `call_sites`, `call_graph`, `type_graph`,
`usages`, `ancestors`, `children`, `impact`, `path`, `symbol_context`,
`dependencies`, `dependents`, `exports`, `modules`, `qualified`, and `filtered`.
Symbol operations take a numeric search-result `id`; `path` also takes `target`.
File operations take a relative `path`; `qualified` takes a glob `pattern` with
`*` and `?`. `modules` and `filtered` need no symbol or path. Incompatible selectors
and unknown fields are rejected.

The `traversal` object controls `max_depth`, positive `limit`, `direction`
(`outgoing`, `incoming`, or `both`), `edge_kinds`, `node_kinds`, and `include_start`.
BFS and DFS default to 1,000 nodes with no depth ceiling. Callers and callees
default to depth 1; call graphs default to depth 2. Impact defaults to depth 3,
10,000 nodes, and 50,000 edges. These defaults are caller-overridable without
additional hard maxima. Other operations have no implicit collection limit.
An explicitly supplied `traversal` object overrides only its named fields.
`call_sites` uses `incoming` for callers and `outgoing` for callees. `impact`
also accepts `max_edges`, including zero. `filtered` uses `node_kinds`
and `include_edges`; `qualified` restricts results to the reference's searchable
declaration kinds. CLI flags use hyphens, for example:

```sh
prometeu-search graph call_sites --id 42 --direction incoming
prometeu-search graph bfs --id 42 --depth 4 --edge-kinds calls,references
prometeu-search graph dependencies --path src/service.go
prometeu-search graph qualified --pattern '*::save'
```

Results include `operation`, `generation`, `data`, `ambiguous`, and `truncated`.
`data` contains complete canonical nodes and edges, ordered neighbor rows,
grouped call sites, a path, symbol context, or file paths according to the
operation. Canonical records use string IDs; input selectors use the numeric
search IDs. `symbol_context` contains the focal node, containment ancestry,
children, incoming/outgoing references, types, and relevant imports. Every
distinct call position survives in `call_sites`, including recursive calls.
BFS preserves parallel edges and prioritizes containment before calls. DFS uses
an explicit stack. Caller/callee and impact walks re-expand nodes reached at a
shorter depth. Impact follows every incoming relationship except containment
and expands nested members of containers without consuming a depth level.

Rust callers use `GraphQueryOptions::for_operation(operation)` to select these
defaults, then override individual fields. `Default::default()` selects BFS;
changing only its `operation` field retains the existing traversal options.
`usize::MAX` represents an unrestricted depth, collection size, or byte budget.

The library and CLI have no implicit serialized-byte ceiling. Optional
`max_bytes` bounds complete responses, with a minimum of 1,024 bytes and no
additional hard maximum. The MCP transport applies its separate response budget.
Clipping removes complete records and marks truncation. A path that cannot
fit is `null`, not a partial successful path. Graph clipping removes edges to
omitted nodes. An explicit module limit bounds both directory entries and file collections;
directory names are opaque keys, including `nodes`, `edges`, `roots`, and `focal`. If even a single record cannot fit, `data` can be `null`.
The uncertainty flag conservatively includes uncertain relationships in the
committed graph. Pure Rust hosts can also construct
`graph_query::GraphSnapshot` over their own canonical records and use its typed
query methods without opening SQLite. Snapshots have no fixed node or edge
ceiling. Traversals terminate through visited-node tracking and the requested
operation budgets. These APIs read no source or network data.

`code_search` retains its `search`, `callers`, and `callees` modes. Relationships
now use import, scope, module, and receiver evidence before name fallback. Existing
relation fields remain present; locations add `id` and `qualified_name`, while
relationships add `kind`, `confidence`, and `ambiguous`.

`code_explore` accepts these arguments:

| Argument | Meaning |
| --- | --- |
| `query` | Required nonempty symbol, file, or lexical query, up to 10,000 UTF-16 units. |
| `mode` | `explore` by default; also `impact`, `path`, `implementations`, or `affected`. |
| `path` | Optional exact file relative to the fixed worktree. Absolute paths and parent traversal are rejected. |
| `line` | Optional positive source line, requiring `path`, for symbol disambiguation. |
| `target` | Required destination symbol for `path` mode; rejected in other modes. |
| `depth` | Positive traversal depth, default 2 in MCP. MCP impact clamps to 1–10; CLI impact defaults to 3. Other native queries have no fixed depth ceiling. |
| `limit` | Positive returned source-symbol, root, impact-entry, or affected-test limit per collection; defaults to 200 for exploration and 10,000 for impact in CLI/MCP. |
| `max_bytes` | Optional complete-result byte budget, at least 1024, with no fixed upper ceiling. MCP additionally applies character budgets; framing is additional. |

The CLI provides `explore`, `impact`, `path`, `implementations`, and `affected`
commands. `path` accepts the source query and destination as positional arguments.
Graph commands accept `--path`, `--line`, `--depth`, `--limit`, and `--max-bytes`.
For example:

```sh
prometeu-search explore 'session launch' --limit 8
prometeu-search explore save --path src/store.ts --line 42
prometeu-search impact Store --depth 3
prometeu-search path start save --depth 4
prometeu-search implementations Repository --depth 3
prometeu-search affected src/store.ts --depth 5
```

Rust hosts call `Engine::explore(query, &GraphOptions)` or
`Engine::analyze(query, GraphMode, &GraphOptions)`. These read an indexed snapshot;
call `refresh` or `sync` first when freshness is required. CLI and MCP entry
points refresh automatically.

## Resolution

Qualified names use `::` between enclosing scopes. A file path plus qualified
name disambiguates symbols; IDs are internal to a committed generation and can
change after indexing. JavaScript/TypeScript imports retain named aliases,
default imports, namespace imports, and reexports. Rust retains module paths and
`use` aliases. Ruby retains `require_relative` dependencies and enclosing scopes.
Simple receiver annotations and construction expressions provide type evidence.
TypeScript interface methods and types imported through a namespace participate
in receiver resolution, subject to the same scope and export boundaries.
Inheritance and implementation declarations create candidate type relationships.
Canonical JavaScript/TypeScript type references join the graph when local scope
or import evidence identifies the declared type. Rust calls exclude module and
namespace declarations from callable candidates.
Local bindings, including destructuring and catch parameters, hide imported
values within their scopes. Calls through unknown bindings that hide imports
remain unresolved; known receiver types retain their static evidence.
JavaScript/TypeScript `let` and `const` bindings declared in a loop header hide
imports only within that loop; `var` bindings retain function scope.

Resolution methods describe evidence, not proof:

- `syntax` describes direct structural containment.
- `scope_match`, `import_match`, `module_match`, and `receiver_match` describe
  static candidates supported by the corresponding source evidence.
- `typed_receiver_match` follows JavaScript/TypeScript or Python receiver evidence
  to its method. Canonical metadata retains `instance-method`, its numeric
  confidence, and Python type provenance when present. JavaScript/TypeScript
  field decisions use 0.85; annotated or constructed locals use 0.9. Field
  initializers and nested arrow functions retain their canonical source owner
  and call-site columns. Unknown claimed field types remain unresolved.
- `function_ref_match` resolves callable values through imports, lexical scope,
  qualified member names, and unique cross-file declarations. Same-file callbacks
  take precedence; ambiguous cross-file names stay unresolved. JavaScript/TypeScript
  `this.member` values resolve to their owning callable or an inherited callable.
  Canonical edges use `references` and `fnRef: true`. The `function-ref` strategy
  retains confidence 0.95 for a unique same-file value, 0.9 for qualified or
  overloaded values, and 0.8 for unique cross-file values. Inherited `this.member`
  values use 0.85. Language-specific receiver and import strategies retain their
  own metadata. Pascal parenthesis-free reads distinguish calls from callbacks
  using routine signatures and assignment types.
- `framework_match` describes framework declarations, component bindings,
  configuration keys, and navigation conventions. Canonical metadata retains
  the framework name and its numeric confidence when available.

- `name_match` describes exact-name ranking or a low-confidence fallback that may
  connect unrelated symbols. Canonical metadata retains the numeric score.
  A JavaScript/TypeScript local declaration whose type provides no method may
  fall back to receiver-name ranking. These candidates remain `low` confidence,
  even when the canonical strategy reports `instance-method` with 0.7 or 0.65.
- `unresolved` means no indexed candidate was established.

Edges include a zero-based UTF-16 `column` when the canonical extraction supplies
one. Call-site identity includes that column, so two calls on one line can resolve
to different overloads. Existing caches reconstruct missing projected columns
from their stored canonical records without reading or reindexing live files.
Older JavaScript/TypeScript constructor and direct `this` call projections also
recover canonical ownership from committed source. Ordinary calls and constructor
calls with the same name on one line remain separate. CommonJS export aliases
participate in import lookup without changing canonical declaration metadata.

Confidence is `high` for direct containment, `medium` for unambiguous static
resolution or a validated unique exact-name decision, `low` for uncertain name
fallback or multiple candidates, and `unknown` for
unresolved references. Each relationship reports its own ambiguity. Validated import
or receiver targets are not replaced with an unrelated same-name target merely
because the expected target is missing.

Workspace package resolution reads committed manifests, export conditions,
entry points, local dependencies, and source alternatives. Python module lookup
recognizes concrete and namespace packages, relative imports, declared source
roots, project boundaries, and standard-library shadowing. Ambiguous roots remain
unresolved. External packages are classified without downloading their contents.

File-based pages and registered backend routes retain their canonical identities
when cross-file finalization changes a visible mount prefix. Nuxt and Astro pages
bind to their own file's component. Vue Router component and layout references
use import evidence before an unambiguous component-name fallback. Navigation
and derived relationships carry additional canonical metadata.

Literal HTTP client calls can reach an unambiguous indexed route. Named queue
jobs, event subscriptions, and socket channels produce heuristic calls with
channel metadata and registration locations. Event fanout is capped; computed
or tied HTTP destinations remain unlinked. Spring and Laravel test requests can
produce separate `test-request` edges. These relationships describe static
evidence and do not establish that a request or test ran successfully.

Spring configuration bindings retain equally plausible profile keys as explicit
low-confidence candidates. Runtime profile selection cannot establish a unique
target. This differs from choosing the first matching profile in query order.

Resolution remains heuristic. Compiler visibility, macro expansion,
metaprogramming, runtime dispatch, and all configuration conventions are not
fully implemented. Unsupported patterns remain unresolved or low-confidence
candidates. Results from files with syntax errors retain `syntax_errors`.

## Analysis semantics

`explore` selects exact names, qualified names, or files first, then uses ranked
lexical search when no exact symbol matches. It expands a bounded neighborhood
and assembles source grouped by file, relationships, and reverse impact. Large
source snippets can begin near a matching line; `snippet_start_line` locates the
returned text while `start_line` and `end_line` retain the complete symbol range.
`snippet_truncated` identifies incomplete source.

`impact` follows every incoming relationship except containment. Containers
include nested members without consuming another traversal depth. `implementations` follows incoming
`extends` and `implements` relationships. `path` finds a shortest directed relationship
path across all edge kinds between selected source and destination candidates. Call, reference, import,
construction, and navigation edges can participate in caller/callee traversal. Cycles are visited once.
All these operations inherit the uncertainty of their relationships.

`affected` follows resolved cross-file dependencies in reverse and returns
candidate test paths. Calls, type references, inheritance, imports, and synthesized
relationships participate; containment does not. This includes same-package
calls in languages where files need no explicit import of each other.
Test detection recognizes test/spec directories and common filename patterns,
including `.test.`, `.spec.`, `_test.`, `_spec.`, and `test_`. It does not execute
tests or claim complete test coverage. An empty impact, path, or affected-test
result is not proof that a change is safe.

## Results and bounds

Graph responses contain `generation`, `roots`, `files`, `edges`, `path`, `impact`,
`affected_tests`, `ambiguous`, and `truncated`. `path` holds symbol IDs in traversal
order. Edges identify source and optional destination IDs; a missing destination
retains the unresolved target spelling. Optional `provenance` and `metadata`
retain canonical and synthesized edge evidence when available. Consumers must tolerate partial context
when `truncated` is true and must not interpret omitted data as absent code.

Every response reads one committed SQLite snapshot. Graph metadata is cached by
generation, including invalidation after another client commits an update. Source
snippets come from that same indexed snapshot, never from live files at old
byte offsets. Read live source before making edits that depend on exact content.

Graph construction retains the complete committed files and edges. The former
64 MiB metadata/source, 100,000-symbol, and 200,000-edge snapshot ceilings are
removed from loading, canonical traversal, hierarchy, context, and synthesis.
Resolution candidates still carry explicit uncertainty. Legacy exploration walks
at least 1,000 symbols, expanding that budget when a larger collection limit is
requested; impact uses 10,000 nodes and 50,000 edges. Canonical graph operations
expose overridable budgets. Path and affected-file searches have no unrelated
1,000-node or 4,000-edge ceiling. Depth, collection, and requested byte limits
can truncate results. MCP exploration content is bounded to 25,000 UTF-16 units;
other graph operations use 15,000, always retaining valid JSON.
The serialized budget includes JSON escaping and metadata. Source and collections
are shortened to fit; every omitted result is signaled with `truncated`.

The graph cache is rebuilt after a changed generation. Targeted source updates
avoid hashing unrelated files, but rebuilding graph metadata still depends on
indexed graph size. This is separate from source synchronization costs.


## Metrics, circular imports, and unreferenced symbols

The CLI also provides `metrics <id>`, `cycles`, and `deadcode`. Rust hosts call
`Engine::metrics(id, max_bytes)`, `Engine::cycles(limit, max_bytes)`, and
`Engine::dead_code(&DeadCodeOptions)`. These read the same indexed snapshots as
other queries; CLI commands refresh first. They are not additional MCP tools.

Metrics count resolved incoming/outgoing edges, calls/callers, direct children,
and containment depth. Unresolved references are not resolved edges and are not
counted. Counts inherit the graph's uncertainty. The result includes generation,
node identity, `metrics`, `ambiguous`, and `truncated`. Node identity can be omitted
when it alone exceeds the byte budget. The ID is the numeric ID returned by search.

`cycles` returns strongly connected groups of files linked by cross-file import
edges, excluding single-file groups. Every file in a group can reach every other
file; the group is not a sequential path. Files are sorted within groups; groups
are sorted by size and first path. Collection and byte limits omit complete
groups rather than presenting fragments as complete components. `total` is the
number of groups before response truncation; a truncated graph can undercount.

`deadcode` starts with callable and type declarations without incoming
non-containment references from another symbol. It then excludes test/generated
files, exports and unknown export coverage, declarations, decorators, overrides,
inferred framework hooks, implicit runtime names, vendored code, test scopes,
markup hosts, unreachable files, unresolved names, and ambiguous names. It checks
remaining identifiers against committed source in their own files and dependent
files. Members are folded under surviving containers, and results rank by source
span, path, and line. These are conservative static candidates, not proof that
code can be deleted.

`--kinds` selects comma-separated declaration kinds. `--include-exported`,
`--include-tests`, and `--include-generated` opt into those otherwise excluded
candidates. The native and CLI default result limit is 200, without an implicit
byte ceiling. The viewer uses its own 300-entry limit. Explicit byte budgets
must be at least 1024 and have no fixed maximum. The flattened report includes `entries`, `total`, `candidates`,
per-rule `excluded` counts, `kinds`, `includeExported`, `bounded`, and
`corroborated`, alongside generation and response truncation. Entry nodes retain
canonical string IDs and complete extraction metadata; those are distinct from
numeric search/inspection IDs. Use their file and source position to locate them.

Candidate scanning stops at 20,000 and sets `bounded`. Source corroboration
examines at most 600 files, at most 2 MB each. Additional work and source-byte
ceilings fail conservatively. Incomplete graph construction is an error for this
report, because missing edges cannot establish that a symbol is unreferenced.
Byte clipping can remove whole entries or member records and sets `truncated`;
`total` remains the count before clipping.

Framework route finalization includes Laravel route-file mounts and NestJS
module/global prefixes, URI versioning, and exclusions. Removing configuration
restores the original route name without changing its canonical or numeric ID.
PHP framework bindings resolve Drupal containers, plugins and hooks, Laravel
controllers and models, and CodeIgniter controllers, loader properties and view
files. Inherited method lookup follows committed inheritance edges. Unresolved
container and loader calls do not acquire unrelated same-name targets. Framework
detection changes and target creation or deletion invalidate the same graph cache.
Terraform/OpenTofu references remain inside their module directory except for
explicit local-module wiring, inherited providers, ancestor variable files, and
unambiguous remote-state component wiring. Registry modules remain unresolved.
COBOL transaction references use committed transaction constants to locate the
owning program. These framework edges retain provenance and confidence.
