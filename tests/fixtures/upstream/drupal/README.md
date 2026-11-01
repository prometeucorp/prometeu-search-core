# Drupal reference fixtures

Pinned revision: `6aa358ccbae30fbf27318f79ee183212f726f9ba`.

`regenerate.mjs <prepared-checkout>` invokes the original resolver, initializes
the original grammars for direct extraction, and indexes temporary projects
through the reference API. Rust results never generate expected output.

The corpus contains 233 complete extraction records, 40 detection decisions,
14 reference-shape claims, and four indexed projects with 91 direct resolver
decisions. Inputs include
the original route, service, plugin, and hook tests, plus Unicode, line ending,
quoted service ID, duplicate annotation, and incomplete attribute cases.

Projects cover service classes by namespace, inherited container methods,
attribute and annotation plugins, procedural and object-oriented hooks, route
controller and form references, composer-less detection, and inactive detection.
The snapshot includes full canonical nodes and edges, along with the exact
supertype lookup answers used by direct resolution.

`tests/drupal.rs` checks canonical framework records and edge metadata after
indexing and reopening. It also compares all resolved call/reference targets
for service, route, plugin, and container-call sources, including failed service
lookups. A lifecycle case adds and removes a service's class, changes framework
detection, and verifies committed graph state across reopen.

Binding indices and source line arrays are borrowed from one committed graph
snapshot. Container method inheritance retains the original six-level search;
no query reads live PHP, YAML, or Composer files.
