---
type: Decision
title: "ADR 0001 — Repository knowledge"
description: "A versioned OKF bundle with short navigation and shared standing instructions."
status: stable
---

# ADR 0001 — Repository knowledge

Date: 2026-10-31

## Context

The engine serves standalone CLI users and embedding hosts. Its process contract,
cache lifecycle, and limitations need a discoverable source of truth without
loading every document into each agent session.

## Options considered

1. Keep detailed guidance in flat files and standing instructions.
2. Maintain a separate documentation service or catalog.
3. Keep a small, typed knowledge bundle beside the implementation.

## Decision

Use `docs/` as the OKF knowledge bundle. Its root `README.md` maps populated
areas; each area's `README.md` lists concepts by title and one-line description.
Concepts use the four-field YAML envelope and lifecycle defined in the
[decision index](README.md). Existing contracts remain cohesive; the format does
not require splitting documents or creating empty areas.

Root `README.md` presents the product, `ARCHITECTURE.md` maps responsibilities,
and `AGENTS.md` stays the short standing-instructions file. License notices
remain at the root. Tool-specific instruction files, when present, point to or
import `AGENTS.md` and contain only their actual differences.

Update guidance in the same change as its behavior. Keep drafts separate from
implemented guidance, and remove retired documents after preserving any current
guarantees in their replacements. Git retains history; no duplicate catalog,
`index.md`, `log.md`, generation timestamps, or `.okf/` service is required.

Validate the bundle through Cargo integration tests, including rendered Markdown
navigation, metadata, local paths, and index coverage. This keeps contributor
checks in the existing Rust toolchain without a separate documentation runtime.

## Consequences

People and agents share a navigable source of truth while initial instructions
stay short. Contributors must maintain metadata and links with code changes.
The checks establish document structure, not the correctness of every claim or
the existence of an external deployment.

## Evidence

- [Bundle map](../README.md) and [contracts index](../contracts/README.md) provide navigation.
- [Integration contract](../contracts/integration.md) distinguishes engine behavior from host work still to be implemented.
- [Documentation checks](../../tests/docs.rs) validate metadata, local paths, and rendered index links through `cargo test --locked --test docs`.
- [CI](../../.github/workflows/ci.yml) runs the Cargo test suite on Linux, macOS, and Windows.

No application protocol or persisted format changes are part of this decision;
compatibility concerns documentation paths and navigation.
