---
id: HF-DC-4
title: "Extract hyperforge-hubs crate"
status: Pending
type: implementation
blocked_by: [HF-DC-3]
unlocks: [HF-DC-7, HF-DC-8]
severity: High
target_repo: hyperforge
---

## Problem

Hyperforge's 7 activations — `HyperforgeHub`, `WorkspaceHub`, `RepoHub`, `BuildHub`, `ImagesHub`, `ReleasesHub`, `AuthHub` — currently live in the top-level crate as `src/hub.rs` and `src/hubs/*`. With types (HF-DC-2) and core (HF-DC-3) extracted, these activation impls are the last logic-bearing set of modules in the top-level crate before it can become a thin bin adapter. This ticket extracts the activations into `crates/hyperforge-hubs/`, depending on `hyperforge-core` and `hyperforge-types`.

## Context

Pre-conditions pinned by HF-DC-S01:

- Module-to-crate mapping confirming `src/hub.rs` and every file under `src/hubs/` moves to `hyperforge-hubs`.
- Public API surface list for `hyperforge-hubs`: the activation structs/impls, constructor functions, and any hub-shared helper types.
- Dependency matrix: `hyperforge-hubs` depends on `hyperforge-core` and `hyperforge-types` (path deps); pulls `plexus-core` and `plexus-macros` from the workspace.

Activation definitions are themselves public — synapse-cc codegen and external activation consumers import these impl types by name. The crate's public API must preserve current import paths (through re-exports in the top-level crate during transition) until HF-DC-5/6/7 migrate the bins.

File-boundary note: HF-DC-4, HF-DC-5, and HF-DC-6 are marked parallel in HF-DC-1's DAG. HF-DC-4 owns `src/hub.rs` + `src/hubs/*` + `crates/hyperforge-hubs/*`. HF-DC-5 owns `src/bin/hyperforge.rs` + `src/main.rs` (if any). HF-DC-6 owns `src/bin/hyperforge-auth.rs`. These file-write sets must stay disjoint per rules 10/11. See each ticket's Required behavior for exact file lists.

## Required behavior

| Behavior | Expected |
|---|---|
| Hubs crate builds standalone | `cargo build -p hyperforge-hubs` succeeds. |
| Hubs depend on core + types | `hyperforge-hubs/Cargo.toml` `[dependencies]` includes `hyperforge-core` and `hyperforge-types` as path deps. |
| Hubs do not depend on bins | No path dep on `hyperforge` (the bin crate). |
| Activation namespaces unchanged | `synapse hf <method>` on every one of the 74 methods returns the same shape as pre-ticket baseline. |
| Public API surface matches spike | Every activation impl listed in HF-DC-S01's `hyperforge-hubs` public API surface is reachable via `hyperforge_hubs::<path>`. |
| Old paths resolve during transition | Top-level crate re-exports the activations so the bins (pre-HF-DC-5/6/7 migration) still compile. |
| Version pinned | `hyperforge-hubs` starts at `0.1.0` (or spike's ratified version). |

Files written by this ticket are limited to:
- `crates/hyperforge-hubs/Cargo.toml` (new)
- `crates/hyperforge-hubs/src/**` (new — population moves from the old paths)
- Root `Cargo.toml` (add workspace member)
- `src/lib.rs` (top-level crate) — updated re-exports only
- Old `src/hub.rs` and `src/hubs/*` files (deletions via `git mv`)

No bin file (`src/bin/*.rs`) is touched by this ticket. Retooling the bins is HF-DC-5/6/7.

## Risks

| Risk | Mitigation |
|---|---|
| Activation macros (`#[plexus_macros::activation]`, `#[plexus_macros::method]`) depend on crate-local paths. | Macros resolve via `$crate::...` under the new crate; verify with `cargo expand -p hyperforge-hubs` on one representative activation. |
| An activation uses a `pub(crate)` helper that's now in the wrong crate. | HF-DC-S01's public API audit surfaces these. Helper promoted to `pub` in core/types or moved into the hubs crate, depending on its shape. |
| `HyperforgeHub` owns references to the other hubs (composition). | Hub composition stays in the hubs crate. Cross-hub references become intra-crate imports. |
| Hub tests depend on fixture setup in the top-level crate. | Test-only deps and fixtures move with the tests into `hyperforge-hubs/tests/`. |

## What must NOT change

- Any of the 74 method signatures, argument names, return shapes, or wire serialization.
- Activation namespaces (`hyperforge`, `workspace`, `repo`, `build`, `images`, `releases`, `auth`).
- Public CLI behavior.
- `hyperforge-types` and `hyperforge-core` crate contents or versions.
- The top-level crate's bin files (`src/bin/*.rs`).

## Acceptance criteria

1. `crates/hyperforge-hubs/Cargo.toml` exists with name `hyperforge-hubs`, version `0.1.0`, path deps on `hyperforge-core` and `hyperforge-types`, no path dep on the top-level `hyperforge` crate.
2. Every activation listed in HF-DC-S01's `hyperforge-hubs` public API surface is reachable via `hyperforge_hubs::<path>`.
3. `cargo build --workspace` succeeds.
4. `cargo test --workspace` succeeds. (Rule 12 integration gate.)
5. A git tag `hyperforge-hubs-v0.1.0` is created locally (not pushed).
6. `synapse hf <method>` smoke suite returns identical responses to the HF-0 baseline on all 74 methods.
7. Every workspace repo that depends on hyperforge still builds. Audit sweep recorded in the commit message.
8. `[workspace]` table in root `Cargo.toml` includes `hyperforge-hubs`.
9. No file under `src/bin/` is modified by this ticket (file-boundary check).

## Completion

Deliverable: a commit (or cleanly-split series) that adds `crates/hyperforge-hubs/`, moves `src/hub.rs` and `src/hubs/*` into it, updates re-exports, and leaves the bins untouched. Tag `hyperforge-hubs-v0.1.0` (local only). Flip this ticket's status to Complete.
