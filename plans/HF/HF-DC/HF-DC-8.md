---
id: HF-DC-8
title: "Workspace polish: feature flags, crate-level docs, CI, README"
status: Pending
type: implementation
blocked_by: [HF-DC-4, HF-DC-5, HF-DC-6, HF-DC-7]
unlocks: []
severity: Medium
target_repo: hyperforge
---

## Problem

Post-HF-DC-2 through HF-DC-7, the workspace has four member crates (`hyperforge-types`, `hyperforge-core`, `hyperforge-hubs`, `hyperforge`) plus two auxiliary bins (`hyperforge-auth`, `hyperforge-ssh`). The mechanics work — every crate builds, every test passes — but the outward-facing surface lacks:
- Audited feature flags (defaults, opt-ins, exclusivity).
- Crate-level `//!` documentation describing each crate's purpose and public API surface at a glance.
- A CI matrix that builds each member crate standalone (catching cross-crate coupling regressions early).
- A `hyperforge/README.md` section documenting the new workspace layout and which crate to depend on for which use case.

This ticket is the polish pass that leaves HF-DC in a state future sub-epics (HF-TT, HF-IR, HF-CTX) and external consumers can build on confidently.

## Context

HF-DC-1's "Completion" section requires "a brief `hyperforge/README.md` update" and workspace-wide build/test gates. This ticket fulfills that plus the feature-flag audit and CI matrix work that HF-DC-S01 flagged as deferrable to polish.

Polish scope is deliberately not about adding new capability. Every change is either documentation, CI config, or a feature-flag default adjustment that preserves current behavior.

## Required behavior

| Area | Expected |
|---|---|
| Feature flag audit | Each member crate's `[features]` section is audited. Default features preserve current behavior. Non-default features (e.g., `prelude`, `git`, `net`, `test-utils`) are documented in each crate's `//!` doc. No feature is defined without a consumer. |
| Crate-level `//!` docs | Each member crate's `src/lib.rs` starts with a `//!` module-level doc block: one-paragraph purpose, key types/functions (with `[`links`]`), feature flags, MSRV note if relevant. |
| CI matrix | CI workflow builds each member crate standalone (`cargo build -p <crate>`), plus `--workspace` and `--all-features`. Catches cross-crate leakage if a member accidentally depends on top-level re-exports. |
| README | `hyperforge/README.md` has a section "Workspace layout" listing the four member crates with one-line descriptions and a "which crate for which use case" subsection (e.g., "Depend on `hyperforge-types` if you only need domain types"; "Depend on `hyperforge-core` if you need business logic but not hub dispatch"; "Depend on `hyperforge-hubs` if you need activation impls to embed in a Plexus RPC server"). |
| No new public types | This ticket adds no new `pub` items. |
| No runtime behavior change | The 74 methods continue to return identical responses. |

## Risks

| Risk | Mitigation |
|---|---|
| Feature-flag default adjustment breaks a downstream crate silently. | CI matrix + workspace-wide audit sweep catches. Any default change is explicit in the commit message. |
| Crate-level docs reference types or functions that were renamed in HF-TT (next sub-epic). | Docs reference current names as of HF-DC; HF-TT will update docs in the same commits that rename types. |
| CI matrix runtime inflates by N× (N crates + workspace + all-features). | Accept the cost. Parallelism in CI runner keeps wall time reasonable. |

## What must NOT change

- Runtime behavior of any activation method.
- Any crate's public API surface (no additions, no removals, no renames).
- Crate versions (this is a docs/CI/flag-audit ticket; no version bump unless a feature-flag default change is user-visible, in which case bump per the version-bump memory).

## Acceptance criteria

1. Each of `hyperforge-types`, `hyperforge-core`, `hyperforge-hubs`, `hyperforge` (and the two bin crates if they carry lib-facing surface) has a `//!` doc block at `src/lib.rs` covering: purpose, key types, feature flags.
2. A feature-flag audit note is committed (either as a section in the HF-DC-S01 architecture doc or as its own doc) listing each crate's features, defaults, and rationale.
3. CI workflow runs, per crate: `cargo build -p <crate>` and `cargo test -p <crate>`. Plus `cargo build --workspace` and `cargo test --workspace`. Plus `cargo build --workspace --all-features`.
4. `hyperforge/README.md` contains a "Workspace layout" section with the crate table and "which crate for which use case" subsection.
5. `cargo build --workspace` succeeds.
6. `cargo test --workspace` succeeds. (Rule 12 integration gate.)
7. No activation method's shape or response differs from pre-ticket baseline.
8. If a feature-flag default change is user-visible: a version bump is applied and tagged locally per the version-bump memory.

## Completion

Deliverable: a commit that adds `//!` docs to every member crate's `lib.rs`, updates CI config, adds the README workspace-layout section, and records the feature-flag audit. Flip this ticket's status to Complete.
