---
type: Contract
title: "Local graph viewer"
description: "Embedded browser assets, loopback serving, graph inspection, and saved reading trails."
status: stable
---

# Local graph viewer

Run `prometeu-search --root <project> --database <cache> ui --port 0` to serve the
viewer on an operating-system-selected loopback port. The process prints the URL
to stderr. `--read-only` disables trail writes. The default CLI port is documented by `ui --help`. All browser assets
are embedded in the executable. No Node runtime, package manager, asset build,
CDN, or internet connection is needed at deployment or launch.

The browser provides ranked symbol search, a directed symbol graph, module maps
and dependency cycles, file outlines and source, route tables, screen navigation,
step graphs, directed call flows, conservative dead-code candidates, type
hierarchies, caller/callee lists, affected test candidates, and saved reading
trails. Graphs support keyboard selection, pan, zoom, filters, focus, and SVG
export. Uncertain edges remain visibly distinct. Empty results do not establish
that no dynamic relationships exist.

## Embedding and snapshots

`Engine::viewer_graph(&ViewerOptions)` returns a bounded graph with stable
canonical node IDs where available, cache-local `indexId` values, and resolution
metadata. The caller may filter by directory, kind, language, name, and graph
neighborhood. `Engine::viewer_query(target)` provides the read endpoints without
opening a socket. Both read committed snapshots and do not implicitly refresh.

`viewer::ViewerServer::bind(port, read_only)` binds only `127.0.0.1`.
`address()` returns the selected address. `serve_one(&mut engine)` handles one
request. A host can call `set_nonblocking(true)` and drive requests from its own
loop; an idle call then returns `false`. This does not introduce an async runtime
or background service. `viewer::serve(&mut engine, port)` owns the blocking CLI
loop and attempts to start the existing native watcher. A watcher failure leaves
query-time refresh available. HTTP queries refresh before reading.

Source responses use the same committed source as graph relationships. They say
`sourceState: "indexed"`; they do not mix live files with stored line positions.
Generation events use server-sent events with a two-second reconnect interval.
A changed generation causes the browser to refresh its view.

## Endpoints and limits

The browser reads these JSON endpoints:

- `/api/stats`, `/api/search?q=`, `/api/graph`, and `/api/nodes?id=`.
- `/api/node/<id>`, `/api/source?file=&from=&to=`, `/api/file/<path>`, and
  `/api/filecode/<path>`.
- `/api/map?root=&depth=`, `/api/routes`, `/api/entrypoints`, `/api/screens`, and
  `/api/steps?anchor=&depth=`.
- `/api/flow?from=&to=` and `/api/deadcode`.
- `/api/trails` and `/api/events`.

Node IDs and file paths must be URL encoded. Explicit directory selections reject
absolute paths, parent components, backslashes, and NUL bytes. Sources are read
from SQLite, never through an HTTP-controlled filesystem path.

Native grammars supply syntax spans for source windows. Sources larger than
256 KiB and formats without a native grammar remain plain text.

Limits belong to individual endpoints; there is no shared response-byte ceiling.
Direct graph export defaults to 500 nodes and depth 2. Hosts can increase either
positive value without a fixed maximum. `ViewerOptions.max_bytes` defaults to
`usize::MAX`; hosts can supply an explicit budget of at least 1 KiB. The browser
initially selects 200 graph nodes. Graph export retains all selected edges unless
an explicit byte budget requires truncation.

- Search defaults to 60 results and permits 1 through 200. Ranking reads up to
  400 candidates from lexical search and supplements unqualified name matches.
- Routes default to 200 rows and permit 3 through 500.
- Entry points default to 12 rows and permit 1 through 50. Their separate
  `routes` selection defaults to 60 and permits 3 through 300.
- Steps default to 120 steps and depth 8. They permit 20 through 400 steps and
  depth 1 through 14. Each function has a fan-out cap of 80, and folded chains
  retain at most 300 visited symbols and seven intermediate hops per step.
- Dead-code candidates default to 300 rows and permit 1 through 300.
- Node inspection retains up to 300 caller groups, 200 callee groups, and
  40 edges per group. File outlines retain 3,000 symbols; import lists retain
  300 files per direction. Node-reference batches accept at most 60 IDs.
- Maps accept depth 1 through 4, defaulting to 1. Modules list up to 40 files;
  cycle summaries retain 40 cycles and 12 members per cycle. Module counts and
  links have no additional global item ceiling.

Truncated lists retain totals and explicitly report truncation. Source excerpts
contain up to 4,000 lines from indexed files no larger than 8 MiB, without a
separate text-byte cap. `from` and `to` select subsequent windows. Other viewer
lists do not expose cursor pagination; callers can narrow the query or anchor.

## Local boundary and trails

HTTP requests require the exact listener's numeric loopback host or `localhost`
and port. Cross-origin `Origin` and `Sec-Fetch-Site` values are rejected. The
server sends no CORS permission, disables framing, uses a restrictive content
security policy, and renders source as text. Request headers and bodies are each
limited to 64 KiB, with finite socket read/write timeouts. Only embedded assets
are served; arbitrary filesystem paths are not static asset paths.

`POST /api/trails` accepts `{ "name": "Reading path", "note": "...", "hops":
["<node-id>"] }`. `DELETE /api/trails/<id>` deletes a saved trail. Writes require
`Content-Type: application/json` and `X-Viewer-Write: 1`. A read-only server rejects
both operations. Saving a trail with the same name replaces that trail.

Trails use a separate SQLite sidecar beside the host-selected index, with the
extension `.viewer.sqlite`. The sidecar records project ownership and rejects
reuse for another root. It does not alter project sources or the index schema.
The limit is 200 trails, 64 hops per trail, 120 name characters, and 600 note
characters. Hops persist file, qualified name, kind, and line, then resolve again
when read. Missing and ambiguous symbols remain explicit after source changes.
Hosts should preserve the sidecar when replacing a derived index if saved trails
must survive cache replacement.

## Equivalence boundary

This viewer reads the engine's actual graph and analysis APIs. It does not claim
pixel-for-pixel or endpoint-schema compatibility with another UI. Branch guards,
early exits, decision arms, and loops are read from native syntax trees for
JavaScript/TypeScript, Python, Go, Java, Kotlin, Swift, C#, C, C++, and Objective-C. Unsupported
languages yield no condition claim. Sites retain whether conditions were
available. The ordered program groups decisions, loops, deferred callbacks,
parallel call groups, and repeated function reads without inventing control flow.

The effect classifier distinguishes network, storage, device, telemetry, database,
response, queue, email, payments, cache, authentication, and process effects.
Database effects retain inferred model and read/write access. Response sites retain
known literal or named status codes, including object properties and preceding
status assignments in the same block. Injected constructor properties and fields
in TypeScript, Java, C#, and Kotlin supply receiver-type evidence. Independent
fixtures compare 848 effect/status decisions, 58 guarded sites, 10 call-site
records, and 11 complete ordered programs.

Flow accepts directed `from`/`to`, a `symbols` list, or repeated `hop` parameters.
A hop starts with `s`, `d`, or `u`, then a canonical node ID. Directed traversal
allows 12 hops; named traversal allows seven with at most one intermediate unnamed
symbol between named symbols. At most four alternative strips are returned.
Actual unresolved dispatch calls can produce a partial strip with uncertain
continuations and unreached named symbols. These are candidates, not proof of a
runtime path. Work caps remain explicit.

Complete equivalence of screen attribution, step folding, symbol ranking,
receiver-type effect inference, and dynamic-dispatch continuation heuristics is
not yet independently established. The viewer uses committed source rather than
live-file drift overlays. Store actions are identified by the reference's file
conventions. Ordinary helpers fold between meaningful steps; the source-order
reading retains their bodies and marks repeated reads.

See [graph queries](graph-queries.md), [source inspection](source-inspection.md),
and [integration](integration.md) for the underlying engine contracts.
