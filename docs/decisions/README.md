# Architecture decisions

Stable decisions describe accepted guidance implemented in this checkout.
Each decision records its context, alternatives, consequences, and evidence.

## Current decisions

- [ADR 0001 — Repository knowledge](0001-repository-knowledge.md) — A versioned OKF bundle with short navigation and shared standing instructions.

## Format

Every concept in `docs/` starts with four nonempty, single-line YAML strings:
`type`, `title`, `description`, and `status`. The type follows its area:
`Architecture`, `Contract`, `Decision`, `Operations`, or `Quality`.
Folder `README.md` indexes and files outside `docs/` do not need frontmatter.

Use plain strings without YAML punctuation, single-quoted strings with doubled
embedded apostrophes, or JSON-compatible double-quoted strings. Quote numeric-only
titles; digit-leading text such as `2026 roadmap` is allowed.

```md
---
type: Decision
title: "ADR NNNN — Title"
description: "The choice this decision records and why it matters."
status: draft
---

# ADR NNNN — Title

Date: YYYY-MM-DD

## Context
## Options considered
## Decision
## Consequences
## Evidence
```

## Lifecycle

- **stable:** guidance in force; this does not assert deployment or complete feature coverage.
- **draft:** proposed guidance, listed separately under a Proposals heading until implemented.
- **deprecated:** supported legacy guidance with an explicit replacement and compatibility scope.

Update decisions with the affected contracts and implementation. Consolidate
remaining guarantees before deleting retired decisions and their incoming links;
Git preserves previous versions. Keep ADR numbers stable and never reuse them.
Keep dates in the body and lifecycle status only in YAML; human-written pages
need no generated, verified, or expiry timestamps.

Each concept needs a rendered link with its title and one-line description in
its folder index. Links in code samples or images do not count as navigation.
Run `cargo test --locked --test docs` before publishing documentation changes.
