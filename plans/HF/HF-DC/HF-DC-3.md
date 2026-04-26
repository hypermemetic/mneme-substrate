---
id: HF-DC-3
title: "Extract hyperforge-core crate"
status: Pending
type: implementation
blocked_by: [HF-DC-2]
unlocks: [HF-DC-4, HF-DC-5, HF-DC-6]
severity: High
target_repo: hyperforge
---

## Problem

With `hyperforge-types` extracted (HF-DC-2), hyperforge's business logic — adapters, build_system, package, auth, git, services — still lives in the top-level crate. Downstream consumers that need logic (not just types) still pull every bin's dependency set. This ticket extracts business logic into a dedicated `hyperforge-core` library crate at `crates/hyperforge-core/`, depending on `hyperforge-types`.

## Context

Pre-conditions pinned by HF-DC-S01:

- Module-to-crate mapping for each of the business-logic modules (`adapters/`, `build_system/`, `package/`, `auth/`, `git/`, `services/`, and any siblings).
- Public API surface list for `hyperforge-core`: the exact `pub` items at the crate root / submodules.
- Re-export strategy for `hyperforge-core::prelude` (if ratified) surfacing common `hyperforge-types` symbols.
- Feature flags, if any (e.g., `prelude`, `git`).

Dependency shape: `hyperforge-core` depends on `hyperforge-types` (path dep), plus the external crates business logic requires. It does **not** depend on hub or bin crates.

After this ticket, the top-level `hyperforge` crate contains only `src/hub.rs` + `src/hubs/*` (activations — moved in HF-DC-4) and `src/bin/*` (bins — retooled in HF-DC-5/6/7). All other previously-public modules are either in `hyperforge-types` or `hyperforge-core`.

## Required behavior

| Behavior | Expected |
|---|---|
| Core crate builds standalone | `cargo build -p hyperforge-core` succeeds. |
| Core depends on types | `hyperforge-core/Cargo.toml` `[dependencies]` includes `hyperforge-types = { path = "../hyperforge-types" }`. |
| Core does not depend on hubs / bins | No path deps on `hyperforge-hubs` or `hyperforge` (the top-level crate). |
| Existing hubs still build | `cargo build --workspace` succeeds; hub modules in the top-level crate now import business logic via `hyperforge_core::*`. |
| Public API surface matches spike | Every item listed in HF-DC-S01's `hyperforge-core` public API surface is reachable via `hyperforge_core::<path>`. |
| Old internal paths still resolve during transition | Top-level crate re-exports as needed (`pub use hyperforge_core::foo;`) so HF-DC-4/5/6/7 can migrate consumers incrementally. |
| Prelude | If the spike ratified a `hyperforge-core::prelude` module, it re-exports the common `hyperforge-types` items listed there. |
| Version pinned | `hyperforge-core` starts at `0.1.0` (or the version ratified in HF-DC-S01). |

Test coverage: all existing unit tests that cover extracted modules move with them. `cargo test -p hyperforge-core` passes.

## Risks

| Risk | Mitigation |
|---|---|
| A business-logic module imports from the hubs (e.g., a service method dispatches via a hub handle). | If the hub dep is real, that module isn't core — it's hub-shaped. HF-DC-S01 flagged these; decision is recorded there. No cross-crate hub→core→hub cycles. |
| Auth logic bleeds into I/O-heavy paths that only make sense in bin context. | Split: pure auth primitives go in core; session/keychain/filesystem drivers go in the bin. HF-DC-S01 pins the boundary. |
| libgit2 / libssh2 / native-tls bloat core's transitive deps. | Feature-flag behind `git` / `net` features per spike; default-off if spike ratified. |
| Build time regression as workspace recompiles. | Accept the one-time cost; measure `cargo build --workspace --release` before vs. after and record in the commit. |

## What must NOT change

- Behavior of any of the 7 activations or 74 methods.
- Public CLI behavior.
- Wire formats.
- `hyperforge-types` crate contents or version (that's HF-DC-2's contract).

## Acceptance criteria

1. `crates/hyperforge-core/Cargo.toml` exists with name `hyperforge-core`, version `0.1.0`, path dep on `hyperforge-types`, no path deps on hubs or bins.
2. Every item in HF-DC-S01's `hyperforge-core` public API surface list is reachable via `hyperforge_core::<path>`.
3. `cargo build --workspace` succeeds.
4. `cargo test --workspace` succeeds. (Rule 12 integration gate.)
5. A git tag `hyperforge-core-v0.1.0` is created locally (not pushed).
6. Every workspace repo that depends on hyperforge still builds. Audit sweep recorded in the commit message.
7. The `[workspace]` table in root `Cargo.toml` includes `hyperforge-core` as a member.
8. No runtime behavior change observable via `synapse hf <method>` on the HF-0 smoke fixture.
9. If a `prelude` feature was ratified: `use hyperforge_core::prelude::*;` compiles in a test file and resolves the symbols listed in the spike's prelude table.

## Completion

Deliverable: a commit (or cleanly-split series) that adds `crates/hyperforge-core/`, moves business-logic modules into it, updates the top-level crate to depend on it, and re-exports symbols needed by not-yet-migrated callers. Tag `hyperforge-core-v0.1.0` (local only). Flip this ticket's status to Complete.
