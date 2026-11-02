The retained compiler in `compiler.rs` derives from the crates.io source of
`picomatch-rs` 0.1.0, published by the contributors at
https://github.com/Maidang1/picomatch-rs. Its MIT license is retained alongside
it. It uses this project's existing serde and regex libraries; the Node binding,
scanner, matcher wrapper, and dependency on a second regex version are absent.
The source uses the same path-separator predicate locally. Its range compiler
and default POSIX-class negation behavior are adapted to the pinned reference.
The parent module supplies UTF-16 matching, malformed-brace behavior, resource
bounds, and independent reference fixtures.

Crates.io package SHA-256: `76a98ffe0fdca4e658785b8f9a5557c71815e57bb4cf3ce36d8a470b1046329a`.
Original compiler SHA-256: `7591cd09098befc81026909a61fb37d8ca399de1001be7c5514d06e1902268c1`.

The range compiler follows the pinned JavaScript implementation: sorted character
classes, including multi-part ranges, rather than numeric enumeration.
