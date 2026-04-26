---
id: HF-DC-7
title: "Retool hyperforge-ssh bin"
status: Pending
type: implementation
blocked_by: [HF-DC-4, HF-DC-5, HF-DC-6]
unlocks: [HF-DC-8]
severity: High
target_repo: hyperforge
---

## Problem

The `hyperforge-ssh` binary at `src/bin/hyperforge-ssh.rs` is the SSH handler. HF-DC-1's DAG places it last in the bin-retool phase because it may depend on the other bins' retooled surfaces (e.g., invoking `hyperforge-auth` as a sidecar via the auth bin's public protocol). This ticket retools `hyperforge-ssh` to import from the sibling crates `hyperforge-core` and `hyperforge-types`, and (where applicable) from re-exports the other bins surface post-retool.

## Context

HF-DC-1's DAG (quoted):

```
      HF-DC-4  HF-DC-5  HF-DC-6
    (hubs)  (hf bin)  (auth bin)
        │        │        │
        └────────┼────────┘
                 ▼
            HF-DC-7 (ssh bin)
```

So HF-DC-7 is explicitly serialized after 4/5/6 rather than parallel with them. Rationale: the SSH handler integrates hubs (for command dispatch) and auth (for credential flow), and validating it against all three sibling crates' post-retool public APIs is cleaner in one pass than trying to track incremental re-exports from three still-in-flight siblings.

File-write scope:
- `src/bin/hyperforge-ssh.rs` (only)
- Top-level crate `Cargo.toml` (dep adjustments for the ssh bin if applicable)

## Required behavior

| Behavior | Expected |
|---|---|
| Bin builds | `cargo build --bin hyperforge-ssh` succeeds. |
| Bin deps | `hyperforge-ssh` bin's Cargo entry has path deps on `hyperforge-hubs`, `hyperforge-core`, `hyperforge-types`. |
| Behavior unchanged | SSH command dispatch, stdin/stdout framing, error reporting — all identical to pre-ticket baseline on the existing SSH smoke fixture. |
| No intra-crate module imports | `rg '^use crate::(hub|hubs|auth|adapters|build_system|package|git|services|types)' src/bin/hyperforge-ssh.rs` returns zero matches. |
| Version bump | If HF-DC-S01 ratified a version bump for this bin, apply it. |

## Risks

| Risk | Mitigation |
|---|---|
| SSH handler needs a private dispatch helper from `hyperforge-hubs`. | Promote helper to `pub` in hubs, or pull the logic into the SSH bin. Record decision in commit. |
| SSH protocol framing depends on types that moved. | Import from `hyperforge_types::*`. |
| Combination of all three sibling-crate APIs surfaces a cross-crate bug that didn't show in HF-DC-4/5/6 individually. | The integration-gate `cargo test --workspace` is the backstop; a failure here is expected to produce a blocker ticket rather than a silent partial-pass. |

## What must NOT change

- SSH wire protocol.
- Stdin/stdout framing.
- Exit codes and error reporting shapes.
- `hyperforge`, `hyperforge-auth` bins.
- Library crates' contents.

## Acceptance criteria

1. `src/bin/hyperforge-ssh.rs` contains no `use crate::{hub, hubs, auth, adapters, build_system, package, git, services, types}` imports — only sibling crates and external deps.
2. `cargo build --workspace` succeeds.
3. `cargo test --workspace` succeeds. (Rule 12 integration gate.)
4. `cargo build --bin hyperforge-ssh` succeeds.
5. If the spike ratified a version bump: local tag created per spike.
6. Smoke test: the SSH handler round-trips a documented request via the existing SSH smoke fixture with identical response to pre-ticket baseline.
7. No file other than `src/bin/hyperforge-ssh.rs` and the top-level `Cargo.toml` is modified (file-boundary check).

## Completion

Deliverable: a commit that rewrites `src/bin/hyperforge-ssh.rs` to import from sibling crates, updates `Cargo.toml`, applies version bump if ratified, and tags locally. Flip this ticket's status to Complete.
