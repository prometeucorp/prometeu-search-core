# Architecture

Prometeu Search is a local search engine, independent of any desktop shell,
agent provider, account, or model API.

The [knowledge bundle](docs/README.md) indexes detailed guidance; the
[integration contract](docs/contracts/integration.md) defines executable, MCP,
distribution, and host lifecycle responsibilities.

`crates/prometeu-extract` contains native Tree-sitter walkers for 29 language
identifiers and a typed Rust decoder. The root parser supplies the remaining
12 component and custom-format extractors. It has no Node binding or external runtime. The parser's
preprocessing, component, and custom-format modules port the surrounding source
transformations and template extraction. Canonical records preserve identifiers,
positions, signatures, documentation, metadata, typed edges, and unresolved
references. Independent oracle fixtures exercise these extraction boundaries;
the committed fixture descriptions define their scope.

`src/parser/native.rs` projects those records into the bounded search engine.
UTF-16 source positions are mapped to UTF-8 byte boundaries before slicing.
Where an upstream walker extends an ending line without extending its column,
the source projection uses the ending line; canonical metadata remains intact.
Component and CFML script columns map back to their host source. Free-format
COBOL removes preprocessing indentation from projected columns; VB.NET source
slices exclude the parser's appended newline. These corrections never rewrite
canonical source positions.
Existing TypeScript/JavaScript/Rust/Ruby semantic extraction still supplies
scope, import, receiver, and shadowing evidence during the resolver migration.
Declarations that the native walker omits remain additional projection records.
Canonical references are retained separately instead of reintroducing excluded
local bindings into the query graph. Every indexed file retains a full-source
projection, including file-level formats with no canonical symbols.

`src/project_config.rs` validates shared extension and selection settings.
Raw content and effective settings fingerprints commit with each configuration
change; even formatting-only changes force full eligibility reconciliation and
re-extraction. Go module text and project manifests are captured with the same committed generation.

The original native extraction boundary, plus ArkTS and Objective-C, rejects
malformed trees in strict mode. The engine retries native Tree-sitter recovery
and records a `native_parse_recovery` diagnostic and the syntax-error flag.
The additional typed walkers follow the reference fallback's recovery contract:
a malformed tree emits a canonical warning when it produces no declarations.
Partial recovery in those walkers does not necessarily produce a warning. Exhausted walker stacks remain errors, not successful partial
extractions. This recovery is not claimed equivalent to the reference parser's
UTF-16 recovery. The parser does not run a compiler.

`src/index.rs` owns one canonical project root and the SQLite schema.
`src/search.rs` queries canonical names, qualified names, documentation, and
signatures through FTS5, followed by substring and bounded typo fallbacks.
Field filters and ranking operate on the same committed snapshot. The separate
source-text FTS index remains available to exploration. `src/sync.rs` reconciles source atomically. Full recovery enumerates and
hashes eligible files; `sync_paths` prunes traversal to changed paths and their
ancestors. Both parse only content changes. Foreign keys and FTS triggers remove
stale extraction and search records together. Source access stays under the
pinned project directory.

`src/graph.rs` resolves the stored extraction metadata and provides exploration,
impact, paths, implementation hierarchies, and affected test candidates. Import,
scope, and receiver evidence precedes uncertain name matching. Relationship
responses retain resolution method, confidence, and ambiguity. Query operations
retain their own traversal and display limits; they do not cap the indexed graph
at a fixed node count, edge count, or aggregate source size. Graph metadata is cached per connection and
index generation; source is read from the same SQLite snapshot as the graph.
A generation change invalidates the cached graph, including when another client
commits changes. The graph remains heuristic, not compiler authority.
Reference lookup preserves canonical UTF-16 columns as well as line numbers.
Multiple calls to the same name on one line retain distinct overload decisions.
Older projected records recover these columns from their committed canonical
extraction, so this additive field does not require replacing the cache.
JavaScript/TypeScript field receiver decisions use canonical source owners and
positions, including property initializers and nested arrow functions that the
legacy projection attributes to an enclosing method. Their source, imports,
declared types, and inheritance come from the same committed generation.
Constructor and direct `this` calls use the same canonical ownership. Older
projections recover missing call positions from committed source without changing
the stored extraction or reading live files. Local receiver inference retains
the reference's line-based declaration scan; it is heuristic, including when
several functions share a line. Name-based fallback candidates remain explicitly
low confidence in public responses.
Declared inheritance resolves before framework bindings that follow inherited
controller or model methods. Framework lookup tables use canonical records and
committed source once per graph generation. Import and ordinary reference edges
then consume those cached decisions. Recognized unresolved container and loader
calls retain uncertainty instead of selecting unrelated same-name declarations.

`src/frameworks.rs` detects framework declarations from committed inputs.
Detection changes re-extract affected language files within the reconciliation
transaction. Pure extraction APIs can supply framework identifiers explicitly.
Framework declarations and references join canonical extraction before projection.
Cross-file route finalization uses the same committed source and manifest inputs.
When a mount or endpoint group changes, finalization renames affected routes
without replacing their numeric or canonical IDs. It updates extraction records,
symbol names, and both search indexes in the same transaction. Cross-file React
route tables store their desired additional declarations and references separately.
Changes re-project only affected files through the normal parser path; removed
consumers remove obsolete declarations. Reopening reads the same committed additions.
SQLite update
triggers keep FTS rows synchronized, including after reopening an existing cache.

`src/synthesis.rs` evaluates ordered structural and callback passes after
resolution, using canonical records and stored source. Synthesized edges carry
provenance and metadata and share the generation cache. Original extraction
records remain unchanged. All 50 registered synthesis passes have Rust
counterparts. The parity record distinguishes their captured evidence from
complete equivalence of every resolution and synthesis branch.

`src/analysis.rs` measures resolved graph edges and computes circular import
groups with an iterative traversal. Its conservative dead-code report removes
candidates when exports, declarations, framework evidence, unresolved names,
source mentions, or missing reachability cast doubt on a negative claim. It
refuses incomplete graph construction and reads only committed source.

`src/hierarchy.rs` derives type ancestry, subtype fans, and member override
matches from canonical records. `src/hierarchy_adapter.rs` applies public byte
budgets over one committed graph generation, retaining direct fan counts and
explicit truncation when detailed entries do not fit.

`src/context.rs` combines ranked search channels with bounded graph expansion,
source blocks, and structured or Markdown context. The pure builder accepts
host-owned canonical records and query callbacks. `src/context_adapter.rs` binds
it to one committed graph generation and SQLite search, applies response byte
limits, and removes dangling relationships when trimming the selected context.

Search reads source stored with the indexed symbol. It never slices a changed
file using old byte offsets. Each response reads one database snapshot and
includes its generation. Files can still change during or after reconciliation;
the index is not a filesystem snapshot or a compiler authority. Callers must read
live source before applying edits that depend on exact content.

`src/source.rs` pins the project directory for each reconciliation and opens each
source path component relative to directory handles, rejecting symlinks throughout
the path. File type and size checks use the opened handle. A replaced source or
ancestor cannot redirect reads outside the selected tree; a failed open rolls back
reconciliation.

`src/watch.rs` uses native notifications with a 250 ms quiet period and a 2 s
maximum burst delay. It retains changed paths; overflow, watcher errors,
directory topology, and ignore-rule changes require full reconciliation.
A filesystem lock elects one watcher per database. Followers retry acquisition
and reconcile after takeover. Updates serialize with SQLite transactions, a
busy timeout, and WAL.

CLI searches and MCP queries call `refresh` independently of watcher delivery.
The first refresh on each connection hashes all files. Later refreshes enumerate
metadata and hash only changed signatures, with full content recovery at least
every 60 seconds while queries or watching remain active. Signatures include
size and modification time, plus device, inode, and change time on Unix.
A file changed during a read gets an invalid signature so the next refresh reads
it again. Metadata-preserving changes can wait until full recovery. Explicit
`sync` always hashes every eligible file. Failed updates roll back atomically.

`src/main.rs` provides the CLI. `src/mcp.rs` provides newline-delimited JSON-RPC
over stdio, with a fixed worktree root, bounded requests and strict tool inputs.
MCP stdout carries only protocol messages. Closing stdin stops the process and
joins its watcher. The library exposes `Engine` for direct Rust embedding.
`src/inspect.rs` supplies bounded file listing, symbol metadata, and file-line
reads from committed SQLite snapshots; it never reads live source for a query.

There is no daemon discovery, remote listener, authentication system, async runtime,
plugin framework, or provider-specific adapter. The embedding host owns process
lifecycle. Separate agent processes can share the same worktree database and
watcher election without sharing writable indexes across worktrees.

Schema version 6 stores root ownership, generation, reconciliation time, files,
file signatures, serialized extraction metadata, symbol ordinals and source,
raw references, canonical extraction records, and separate canonical and source FTS5 indexes. The format is derived cache data, not a public
interchange format. Increment the version when extraction or storage changes
require rebuilding unchanged files. Unknown versions are rejected instead of
silently destroying or migrating another engine's data. Versions 1 through 5 are
rejected without modification because unchanged files need the new extraction
records, canonical search fields, and full-file source. Stop clients and use a fresh cache or
deliberately remove the derived index before retrying.

Native Tree-sitter grammars and bundled SQLite keep the executable independent
of external runtimes.

Pure graph traversal and hierarchy functions accept host-owned canonical records.
Engine adapters expose graph traversal, hierarchy, task context, and analysis over one committed generation.
The viewer library binds only loopback and serves embedded browser assets through a host-owned server lifecycle.
