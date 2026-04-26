---
id: HF-DC-5
title: "Retool hyperforge bin to depend on hubs + core + types"
status: Pending
type: implementation
blocked_by: [HF-DC-3]
unlocks: [HF-DC-7, HF-DC-8]
severity: High
target_repo: hyperforge
---

## Problem

With `hyperforge-types` (HF-DC-2) and `hyperforge-core` (HF-DC-3) extracted, and `hyperforge-hubs` extracted in parallel (HF-DC-4), the `hyperforge` binary at `src/bin/hyperforge.rs` still imports internal modules via intra-crate paths. Once the extractions land it must import from the sibling crates instead. This ticket retools the `hyperforge` bin's import surface to depend on `hyperforge-hubs`, `hyperforge-core`, and `hyperforge-types` directly.

## Context

The `hyperforge` bin is the CLI adapter: arg parsing, tracing init, server startup, and wiring the 7 activations into a Plexus RPC server. Post-retool, its only path deps are the three library crates plus external CLI crates (`clap`, `tokio`, `tracing`, `tracing-subscriber`, etc.).

HF-DC-4, HF-DC-5, and HF-DC-6 can run in parallel because their file-write sets are disjoint:
- HF-DC-4 owns `src/hub.rs`, `src/hubs/*`, `crates/hyperforge-hubs/*`.
- HF-DC-5 owns `src/bin/hyperforge.rs` (and `src/main.rs` if present).
- HF-DC-6 owns `src/bin/hyperforge-auth.rs`.
- All three touch root `Cargo.toml`'s `[workspace]` members array — but additions are idempotent and unlikely to collide; the implementor coordinates with sibling tickets via the commit log.

This ticket does **not** move any file into a new crate. It rewrites import paths and `Cargo.toml` `[dependencies]` in the top-level crate's bin entry.

## Required behavior

| Behavior | Expected |
|---|---|
| Bin builds standalone | `cargo build --bin hyperforge` succeeds. |
| Bin deps are sibling crates | `hyperforge` (bin crate) `Cargo.toml` has path deps on `hyperforge-hubs`, `hyperforge-core`, `hyperforge-types` — and no reliance on internal module paths for library code. |
| CLI behavior unchanged | Every `hyperforge <subcommand>` invocation produces the same output, exit code, and error shape as pre-ticket baseline. |
| No intra-crate module imports | `rg '^use crate::(hub|hubs|adapters|build_system|package|auth|git|services|types)' src/bin/hyperforge.rs` returns zero matches. |
| Version bump | The top-level `hyperforge` bin crate bumps version per the spike's guidance (likely `4.2.0` or `5.0.0` — HF-DC-S01 records which, reflecting the workspace split as a minor-or-major). |

Files written by this ticket:
- `src/bin/hyperforge.rs`
- `src/main.rs` (if it exists and dispatches to the bin)
- Top-level crate's `Cargo.toml` (`[dependencies]` and `[package].version`)

No file under `src/hub.rs`, `src/hubs/*`, `src/bin/hyperforge-auth.rs`, or `src/bin/hyperforge-ssh.rs` is touched here.

## Risks

| Risk | Mitigation |
|---|---|
| A helper function in the bin relies on a `pub(crate)` item now in a sibling crate. | HF-DC-S01's public API audit ensures the item is either `pub` in the sibling crate or the helper moves into the sibling. If discovered mid-implementation, file a follow-up ticket per the cleanup-tickets-immediately memory. |
| The bin's arg grammar references types that moved to `hyperforge-types`. | Import from `hyperforge_types::*` directly — no re-export hop through the top-level crate. |
| Tracing init depends on env var parsing that touches multiple modules. | Env handling stays in the bin; no cross-crate leakage. |
| Bin tests rely on fixture modules previously intra-crate. | Fixtures move into `hyperforge/tests/` or are refactored to use the sibling crates' public APIs. |

## What must NOT change

- CLI arg grammar (flag names, positional args, subcommand names, help text wording of stable messages).
- Exit codes for every documented error path.
- Output format on stdout/stderr for stable-contract outputs.
- Behavior of `hyperforge-auth` or `hyperforge-ssh` bins.
- `hyperforge-types`, `hyperforge-core`, `hyperforge-hubs` crate contents.
- Files in `src/hub.rs`, `src/hubs/*`, or bins other than `hyperforge.rs`.

## Acceptance criteria

1. `src/bin/hyperforge.rs` contains no `use crate::{hub, hubs, adapters, build_system, package, auth, git, services, types}` imports — only imports from the three sibling crates and external deps.
2. Top-level `hyperforge` bin crate's `Cargo.toml` has path deps on `hyperforge-hubs`, `hyperforge-core`, `hyperforge-types`.
3. Top-level `hyperforge` bin crate's `Cargo.toml` `[package].version` is bumped per HF-DC-S01's version plan.
4. `cargo build --workspace` succeeds.
5. `cargo test --workspace` succeeds. (Rule 12 integration gate.)
6. Local git tag `hyperforge-v<new-version>` created (not pushed).
7. `synapse hf <method>` smoke suite returns identical responses to HF-0 baseline on all 74 methods.
8. Every workspace repo that depends on the `hyperforge` bin still runs as a child process the same way (audit sweep recorded in commit message).
9. No file under `src/bin/hyperforge-auth.rs` or `src/bin/hyperforge-ssh.rs` is modified (file-boundary check).

## Completion

Deliverable: a commit that rewrites `src/bin/hyperforge.rs` (and `src/main.rs` if it exists) to import from sibling crates, updates the top-level crate's `Cargo.toml`, bumps the version per spike, and tags locally. Flip this ticket's status to Complete.
