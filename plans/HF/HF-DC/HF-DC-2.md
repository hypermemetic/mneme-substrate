---
id: HF-DC-2
title: "Extract hyperforge-types crate"
status: Pending
type: implementation
blocked_by: [HF-DC-S01]
unlocks: [HF-DC-3]
severity: High
target_repo: hyperforge
---

## Problem

Hyperforge's domain types (`Repo`, `RepoRecord`, `Forge`, `Visibility`, `PackageRegistry`, `BuildSystemKind`, `VersionBump`, `VersionMismatch`, `HyperforgeEvent`, `PackageInfo`, `CrateInfo`, and the other types ratified in HF-DC-S01) currently live inside the single `hyperforge` binary crate. External consumers (HF-CTX, future workspace tools, downstream substrate activations) cannot depend on these types without pulling the whole binary's dependency set. This ticket extracts the types into a dedicated `hyperforge-types` library crate at `crates/hyperforge-types/`.

## Context

Pre-conditions pinned by HF-DC-S01:

- Final crate name (confirmed `hyperforge-types` or the spike's replacement).
- Module-to-crate mapping identifying which modules and which items within mixed modules move to `hyperforge-types`.
- Public API surface list: the exact `pub` items at the crate root / named submodules.
- Dependency list: `hyperforge-types` depends only on small, stable external crates (`serde`, `thiserror`, `semver`, etc.); it does **not** depend on other hyperforge crates.

Workspace transition: this is the first ticket that introduces the Cargo workspace structure. Root `Cargo.toml` becomes `[workspace]` with member `crates/hyperforge-types`. The existing single-crate layout keeps working until HF-DC-3 through HF-DC-7 finish migrating; during HF-DC-2 the top-level crate continues to contain core/hubs/bins but consumes `hyperforge-types` as a workspace dependency.

## Required behavior

| Behavior | Expected |
|---|---|
| Workspace detection | `cargo metadata --format-version 1` reports `hyperforge-types` as a workspace member. |
| Types crate builds standalone | `cargo build -p hyperforge-types` succeeds. |
| Types crate has no hyperforge deps | `hyperforge-types/Cargo.toml` `[dependencies]` contains no path dependency on any other `hyperforge*` crate. |
| Existing code still builds | `cargo build --workspace` succeeds; the top-level `hyperforge` crate now imports domain types from `hyperforge_types::*` rather than defining them inline. |
| Public API surface matches spike | Every item listed in HF-DC-S01's public API surface for `hyperforge-types` is reachable via `hyperforge_types::<path>`. |
| Old internal paths still resolve during transition | Where the top-level crate previously exposed `hyperforge::Repo` etc., it continues to via `pub use hyperforge_types::Repo;` (so HF-DC-3..7 can migrate consumers incrementally). |
| Version pinned | `hyperforge-types` starts at `0.1.0` (or the version ratified in HF-DC-S01). |

Test coverage: any existing unit tests that cover the extracted types move with them. `cargo test -p hyperforge-types` passes.

## Risks

| Risk | Mitigation |
|---|---|
| A "type" module also contains business logic (constructors that hit disk, etc.). | HF-DC-S01 already identified these; the constructor function stays in `hyperforge` (moves to core in HF-DC-3), the type struct moves here. Pre-split call sites use the type via its new path. |
| Serde derives reference helper functions in non-types modules. | Helpers either move with the types (if small + pure) or the types use a different serialization strategy documented in the spike. No cross-crate `#[serde(serialize_with = ...)]` paths. |
| Transitive dependency bloat on `hyperforge-types`. | Keep the dep list minimal per spike's `hyperforge-types` dep matrix. Any dep pulled for convenience only should be moved to `hyperforge-core`. |
| Downstream sibling-workspace crates that depend on hyperforge break. | Audit sweep at end of ticket: any workspace crate with a path dep on hyperforge still compiles via re-exports from the top-level crate. |

## What must NOT change

- Behavior of any of the 7 activations or 74 methods.
- Public CLI behavior.
- Serialized form of any domain type on the wire (JSON field names, bincode layout, etc.). Types are moved bit-exact.
- `hyperforge` binary crate's existing version number (bumped in HF-DC-5, not here).

## Acceptance criteria

1. `crates/hyperforge-types/Cargo.toml` exists with name `hyperforge-types`, version `0.1.0`, and no path deps on sibling hyperforge crates.
2. Every item in HF-DC-S01's `hyperforge-types` public API surface list is reachable via `hyperforge_types::<path>`.
3. `cargo build --workspace` succeeds in `/Users/shmendez/dev/controlflow/hypermemetic/hyperforge/`.
4. `cargo test --workspace` succeeds in `/Users/shmendez/dev/controlflow/hypermemetic/hyperforge/`. (Rule 12 integration gate.)
5. A git tag `hyperforge-types-v0.1.0` is created locally (not pushed) per the version-bump memory.
6. Every workspace repo that depends on hyperforge still builds. Audit sweep recorded in the commit message.
7. The root `Cargo.toml` contains a `[workspace]` table with `hyperforge-types` as a member.
8. No runtime behavior change observable via `synapse hf <method>` calls — all 74 methods return identical responses to pre-ticket baseline on the HF-0 smoke fixture.

## Completion

Deliverable: a single commit (or cleanly-split commit series) that adds `crates/hyperforge-types/`, moves the ratified types into it, updates the top-level crate to depend on it, and re-exports moved types from the top-level crate so HF-DC-3 through HF-DC-7 can migrate consumers incrementally. Tag `hyperforge-types-v0.1.0` (local only). Flip this ticket's status to Complete.
