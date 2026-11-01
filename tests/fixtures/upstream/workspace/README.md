# Workspace and import fixtures

These records were captured independently from the prepared reference checkout at
`6aa358ccbae30fbf27318f79ee183212f726f9ba`. The regeneration scripts execute its
original package loader, import scanner, resolver, and full indexing pipeline.
Rust output is never used to produce expected records.

- `cases.json`: 20 package discovery/entry cases, including scoped path dependencies,
  nested roots, pnpm globs, OHPM, browser conditions, exports, depth limits, and
  19 tolerant JSONC recovery cases.
- `projects.json`: eight fully indexed projects, with complete canonical nodes and
  edges. Rust tests compare all non-containment edge endpoints and kinds, then
  reopen the persisted index after overwriting the live files.
- `imports.json`: 23 JS/TS binding scans, 76 manifest ownership decisions, and the
  reference runtime's 72 builtin module names. Scans cover Unicode, string-named
  imports, comments, JSDoc, CommonJS, and component-hosted imports.
- `defaults.json`: 28 stated-default export cases and 29 nested template-position
  decisions. Includes named ESM/CommonJS, declaration files, and component files.
- `modules.json`: 112 relative and conventional module lookup decisions across
  eight script/component languages, including extension priority and remapping.

Regenerate with Node and the prepared checkout path:

```sh
node tests/fixtures/upstream/workspace/regenerate.mjs /path/to/reference
node tests/fixtures/upstream/workspace/regenerate-projects.mjs /path/to/reference
node tests/fixtures/upstream/workspace/regenerate-imports.mjs /path/to/reference
node tests/fixtures/upstream/workspace/regenerate-defaults.mjs /path/to/reference
node tests/fixtures/upstream/workspace/regenerate-modules.mjs /path/to/reference
```

The Rust index captures confined manifest and directory snapshots during sync;
resolution never consults subsequently changed source files or manifests. Host
read and graph budgets remain explicit. These fixtures establish the listed
boundaries rather than equivalence for every possible module-resolution setup.
