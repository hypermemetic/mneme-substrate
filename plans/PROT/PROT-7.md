---
id: PROT-7
title: "plexus-substrate 0.6.0: rebuild against plexus-core 0.6 / plexus-macros 0.6 / plexus-transport 0.3"
status: Pending
type: implementation
blocked_by: [PROT-3, PROT-4]
unlocks: [PROT-10]
severity: High
target_repo: plexus-substrate
---

## Problem

plexus-substrate pins `plexus-core = "0.5"`, `plexus-macros = "0.5"`, `plexus-transport = "0.2"` (via `[patch.crates-io]` already removed post-autonomous-run). PROT-2/3/4 bump each to 0.6.0 / 0.6.0 / 0.3.0. Substrate needs pin bumps, rebuild, verify all activations' dispatch works under the new macro codegen.

Also: substrate's activations (solar, orcha, claudecode, cone, arbor, etc.) each use `#[plexus_macros::activation]`. The PROT-3 schema dispatch change is transparent to user code but may shift generated code enough to surface pre-existing warnings or latent bugs.

## Context

- Repo: `/Users/shmendez/dev/controlflow/hypermemetic/plexus-substrate/`
- Version: 0.5.0 → 0.6.0.
- Files to edit:
  - `Cargo.toml` — dependency pins.
  - Any activation that references `SchemaResult::Method` directly (grep verifies none expected).

## Required behavior

1. **Bump pins** in `Cargo.toml`:
   ```
   plexus-core = "0.6"
   plexus-macros = "0.6"
   plexus-transport = "0.3"
   ```

2. **Rebuild**: `cargo build` and `cargo test` at substrate root. Expected: both green, all 113 lib tests pass.

3. **Runtime verification** via child schema drill-down. Start a substrate server locally. Invoke via raw websocket or synapse (post-PROT-6):
   ```
   synapse <substrate-backend> claudecode session <session-id>   # drills to SessionActivation
   synapse <substrate-backend> cone of <cone-id>                  # drills to ConeActivation
   synapse <substrate-backend> orcha pm                           # drills to Pm child
   ```
   Each must return a valid tree view of the child activation, not "No schema in response".

4. **Version bump** substrate: 0.5.0 → 0.6.0.

5. **Tag** `plexus-substrate-v0.6.0` locally.

## Risks

| Risk | Mitigation |
|---|---|
| plexus-macros 0.6's new schema dispatch trips any activation whose `#[child]` methods have ambiguous routing (e.g., a method name that collides with a child accessor name). | Substrate audit: grep `#[plexus_macros::method]` and `#[plexus_macros::child]` across all activations. No collisions expected. |
| A deprecated pattern still lingering in substrate source (grep showed `#[allow(deprecated)]` in IR-10/IR-16 lineage). | Should have been cleaned by IR-16 / HF-CLEAN-style patterns. Verify grep returns only justified allows with TODO markers pointing at PM-06 or similar follow-ups. |
| Substrate consumes `SchemaResult::Method` directly. | Grep confirms: no direct references expected in substrate source. |
| `cargo tree -d` shows multiple plexus-core versions. | Audit sweep per version-bump memory. If duplicates exist, trace pin chain. |

## What must NOT change

- All 113 existing lib tests pass unchanged.
- No activation's public method surface changes.
- No deprecation annotations added or removed beyond PROT-3 cleanup.
- Activation namespaces (`solar`, `orcha`, `claudecode`, `cone`, `arbor`) — unchanged.
- Child-gate paths (e.g., `claudecode.session.<id>`, `cone.of.<id>`) — unchanged.

## Acceptance criteria

1. `cargo build --workspace` and `cargo test --lib` at substrate root green.
2. `cargo tree -d` in substrate shows a single version of each plexus-* crate.
3. `grep -rn 'SchemaResult::Method' plexus-substrate/src/` returns zero results.
4. Runtime check: Child-activation drill-down works via synapse post-PROT-6. Verified by PROT-10 end-to-end.
5. substrate `Cargo.toml` version is `0.6.0`. Tag `plexus-substrate-v0.6.0` exists locally.

## Completion

PR against substrate. Status flipped to Complete at PROT-10's end-to-end verification pass.
