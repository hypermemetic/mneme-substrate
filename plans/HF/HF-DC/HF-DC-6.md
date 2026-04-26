---
id: HF-DC-6
title: "Retool hyperforge-auth bin"
status: Pending
type: implementation
blocked_by: [HF-DC-3]
unlocks: [HF-DC-7, HF-DC-8]
severity: High
target_repo: hyperforge
---

## Problem

The `hyperforge-auth` binary at `src/bin/hyperforge-auth.rs` is the secrets sidecar. Post-HF-DC-3 (core extraction) its intra-crate imports of auth and core modules must be replaced with path imports from the sibling crates `hyperforge-core` and `hyperforge-types`. This ticket retools the `hyperforge-auth` bin accordingly.

## Context

The auth sidecar runs as a separate process, invoked by the main `hyperforge` bin for privileged operations. It depends on `hyperforge-core::auth::*` and a narrow slice of `hyperforge-types`. It does **not** depend on `hyperforge-hubs` (no RPC-server role).

HF-DC-4, HF-DC-5, HF-DC-6 are parallel per HF-DC-1's DAG. File-write boundaries:
- HF-DC-6 owns `src/bin/hyperforge-auth.rs` and nothing else under `src/`.
- It may add a path dep line to root `Cargo.toml` for `hyperforge-core` / `-types`; coordinate with sibling tickets via the commit log.

## Required behavior

| Behavior | Expected |
|---|---|
| Bin builds | `cargo build --bin hyperforge-auth` succeeds. |
| Bin deps | `hyperforge-auth` bin's Cargo entry has path deps on `hyperforge-core` and `hyperforge-types` (not `hyperforge-hubs`). |
| Behavior unchanged | Secrets read/write paths produce identical on-wire and on-disk behavior to pre-ticket baseline. |
| No intra-crate module imports | `rg '^use crate::(auth|adapters|build_system|package|git|services|types)' src/bin/hyperforge-auth.rs` returns zero matches. |
| Version bump | If HF-DC-S01 ratified a version bump for this bin (likely yes, same mint as the main bin), apply it. |

Files written by this ticket:
- `src/bin/hyperforge-auth.rs`
- Top-level crate `Cargo.toml` (bin-specific `[[bin]]` section dep lines if applicable; the top-level crate's overall `[dependencies]` if the sidecar shares them)

No other file is touched.

## Risks

| Risk | Mitigation |
|---|---|
| The sidecar uses a private helper now in `hyperforge-core`. | Helper promoted to `pub` in core (either in-scope here or filed as a cleanup ticket per cleanup-tickets-immediately memory). |
| Keychain / OS-specific auth paths depend on types that moved. | Import from `hyperforge_types::*` directly. |
| Sidecar protocol between main bin and auth bin changes. | It must not. If an import change requires a protocol tweak, that's a red flag; pause and file a spike. |

## What must NOT change

- Sidecar wire protocol between `hyperforge` and `hyperforge-auth`.
- Keychain / keyring interactions or secrets-on-disk formats.
- Exit codes, stdout/stderr formats of the sidecar.
- `hyperforge`, `hyperforge-ssh` bins.
- `hyperforge-types`, `hyperforge-core`, `hyperforge-hubs` crate contents.

## Acceptance criteria

1. `src/bin/hyperforge-auth.rs` contains no `use crate::{auth, adapters, build_system, package, git, services, types}` imports — only sibling crates and external deps.
2. `cargo build --workspace` succeeds.
3. `cargo test --workspace` succeeds. (Rule 12 integration gate.)
4. `cargo build --bin hyperforge-auth` succeeds.
5. If the spike ratified a version bump: local tag created per spike.
6. Smoke test: `hyperforge-auth` can be invoked as a sidecar by the main bin and performs a secret read/write round-trip identically to pre-ticket baseline.
7. No file other than `src/bin/hyperforge-auth.rs` and the top-level `Cargo.toml` is modified (file-boundary check).

## Completion

Deliverable: a commit that rewrites `src/bin/hyperforge-auth.rs` to import from sibling crates, updates `Cargo.toml`, applies version bump if ratified, and tags locally. Flip this ticket's status to Complete.
