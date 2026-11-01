# Traversal reference data

`cases.json` records 39 projects evaluated by the pinned reference scanner at
revision `6aa358ccbae30fbf27318f79ee183212f726f9ba`. It stores source/config text,
real Git index bytes, and the loose/packed objects required by sparse indexes.
Expected file lists come only from the reference implementation.

The projects cover root versus nested tracked-file ignores, negation, the full
default directory catalog, JVM/source-build exceptions, configured includes and
excludes, embedded repositories, ignored repository opt-ins, submodules, linked
worktrees, project subdirectories, version-4 and split indexes, sparse flags,
SHA-256, loose/packed sparse trees, Unicode paths, and Git configuration includes
and conditions. Additional Rust regressions check rollback on corrupt metadata,
source confinement, targeted visibility reconciliation, and watcher exceptions.
Seven projects exercise process configuration: counted overrides, override
precedence, both quoted parameter formats, quoted paths, environment includes,
and remote-URL conditions supplied by the environment. Those cases run the Rust
executable with an empty `PATH`, proving that production traversal does not
invoke Git or Node. Process environments stay isolated from parallel tests.

Regenerate with:

```sh
node --liftoff-only tests/fixtures/upstream/traversal/regenerate.mjs /path/to/prepared/upstream
```

Generation requires Git and the prepared reference development checkout. The
normal Rust tests use the committed data and need neither Git nor JavaScript.
The generator checks the revision before execution. Temporary absolute paths
are represented by `<WORKSPACE>` and substituted on fixture installation.
The default exclusion catalog derives from the same MIT-licensed reference;
its retained license is [LICENSE](../LICENSE). Gitoxide dependencies retain
their own licenses in the distributed third-party notices.
