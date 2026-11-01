# Framework extraction reference corpus

`detection.json` captures every one of the 37 registered framework detectors
across 46 real projects. It includes root and workspace dependencies, source
signatures, empty projects, and malformed package metadata. The production gate
also compares all 185 synthesis projects, then checks removal and restoration of
every detector fixture across targeted reconciliation and cache reopening.
Regenerate this capture with
`node --liftoff-only tests/fixtures/upstream/frameworks/regenerate-detection.mjs /path/to/prepared/upstream`.

Pinned reference revision: `6aa358ccbae30fbf27318f79ee183212f726f9ba`.

`regenerate.mjs <prepared-checkout>` executes the original framework `extract`
methods against literal cases from its tests and additional Unicode, comment,
decorator, imperative-registration, and duplicate-declaration inputs. Expected
nodes and unresolved references retain their order and duplicate records. Only
node timestamps are omitted. No Rust implementation generates expected outputs.

The 134 direct cases cover Fabric/Paper components, Expo Modules declarations,
Django, Flask, FastAPI, Go HTTP registration, GoFrame request metadata,
Rust Actix/Axum/Rocket routes, Spring mappings and configuration keys, Vapor
handlers and closures, and Nuxt/Astro file routes. They validate per-file
extraction separately from framework detection and resolution. Twelve additional
manifest cases check bounded dependency discovery and its UTF-16 ordering.
`tests/frameworks.rs` also checks projection to valid original source and selected
manifest/source detection changes across persistent index generations.

`regenerate-components.mjs <prepared-checkout>` captures 117 exact binding and
ownership decisions over three real indexed projects. They cover Vue compiler
macros, Nuxt auto-import shadowing and virtual modules, Astro globals/components,
Svelte runes/stores/modules, directory preference, ambiguous component names,
and language boundaries. The Rust comparison also reopens the committed cache.

`regenerate-projects.mjs <prepared-checkout>` indexes 16 complete projects through
the reference project API and records canonical routes, configuration bindings,
and outgoing handler targets. Tests compare those records after indexing and
after reopening, with live source deliberately replaced before the second read.
Separate lifecycle tests change and remove FastAPI mount prefixes while
preserving route IDs and refreshing both search indexes.

Spring profile ambiguity is an explicit contract difference: the fixture retains
the reference-selected target and also records all equally plausible canonical
configuration keys. The engine exposes those candidates with low confidence.

`aspnet.json` contains 19 direct ASP.NET extraction cases and eight complete
route-finalization inputs evaluated by the pinned resolver. It covers controller
attributes, minimal APIs, endpoint groups, FastEndpoints literals and constants,
Unicode/comment boundaries, class-prefix overrides, duplicate constant owners,
and the 400-file group-prefix limit. Full node and unresolved-reference records
are compared with only timestamps removed. Finalization lookup ordering comes
from the reference SQLite query builder; canonical input records remain intact.
Regenerate with `regenerate-aspnet.mjs <prepared-checkout>`.

`aspnet-projects.json` records three projects indexed through the real reference
project API, including final route nodes and handler edges. Regenerate with
`regenerate-aspnet-projects.mjs <prepared-checkout>`. `tests/aspnet.rs` verifies
stored metadata, targets, reopen consistency, and cross-file constant/prefix
changes without replacing route IDs.

`mobile.json` records 459 SwiftUI/UIKit, Swift–Objective-C, React Native bridge,
and Expo binding answers, 106 competing Swift type decisions, 11 native export
extractions, and 12 projects indexed by the reference implementation. The
original bridge test contexts run directly; additional fixtures cover visibility,
nested declarations, module aliases, Unicode, duplicate exports, and comment
boundaries. Full edge comparisons include secondary Expo targets and stronger
exact-name winners, preserving their different metadata. The separate bridge
lifecycle test checks edits, another connection's commit, and reopening.
Regenerate with `node --liftoff-only regenerate-mobile.mjs <prepared-checkout>`.
Expected records come exclusively from the pinned TypeScript implementation and
its project API; only node timestamps and the direct answer's repeated input are
omitted. Original source attribution and license are retained in the parent
corpus directory. The Swift type competition helper does not replace the general
matcher for mixed-language or non-type candidates.

`cargo-workspace.json` records 11,970 glob answers from the pinned picomatch 4.0.7
runtime, 12 package-name cases, 22 workspace maps, and ten projects indexed by
the reference API. The corpus exercises brace/range/extglob matching, Unicode,
malformed patterns, depth limits, hidden paths, duplicate aliases, deep literal
members, local module precedence, and complete competing-edge metadata.
Regenerate with `node --liftoff-only regenerate-cargo-workspace.mjs <prepared-checkout>`.
The separate Cargo lifecycle tests check manifest-only commits, another
connection's cache, reopening, oversized input rollback, and symlink confinement.

`vapor.json` records 170 direct handler/naming answers and eleven actual indexed
projects. Original route tests cover nested controllers, dotted extensions,
Request overload preference, lexical handlers, and closure routes. Additional
projects exercise naming competition, private/test/local types, and class versus
struct controllers. The full edge gate preserves all metadata; lifecycle tests
exercise handler replacement and removal across committed cache generations.
Regenerate with `node --liftoff-only regenerate-vapor.mjs <prepared-checkout>`.

`swift-type-gate.json` records 117 final type-target decisions intercepted from
seven original indexed projects and five explicit Unicode/attribute probes.
It checks extension retargeting, SDK-only extension rejection, nested scopes,
inherited type owners, generic qualifiers, and preservation of result metadata.
Regenerate with `node --liftoff-only regenerate-swift-type-gate.mjs <prepared-checkout>`.
The shared framework batch applies this gate after choosing the winning strategy;
applying it to all remaining generic Swift strategies is tracked separately.

`go-bindings.json` records 57 independently indexed projects, 418 direct and final
selection records, and 456 spelling probes. Every call and convention-shaped
reference compares complete canonical edges after indexing and reopening, including explicit
unresolved cases. Coverage includes package ownership, dot imports, qualified
imported types, exported exact-name competition, and indexed slices, arrays, maps,
variadics, local literals, and named containers. Element methods follow imported
types, aliases, and embedded types; primitive, external, and double-pointer
elements retain explicit unresolved outcomes. Public lifecycle tests commit
changes to dot imports, `go.mod`, and imported element methods through another
connection, then reopen the cache. Original receiver suites add function and method
results, tuple results, fields, embedded fields, fluent chains, assertions, generic
constraints, repeated constant types, and external-type exclusions. Oversized Go
files contribute bounded declaration headers to the committed auxiliary snapshot;
their own methods suppress false inherited targets without live reads during queries.
Regenerate with
`node --liftoff-only regenerate-go-bindings.mjs <prepared-checkout>`.
