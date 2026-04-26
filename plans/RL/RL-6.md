---
id: RL-6
title: "Fix Loopback approval-resolution error swallowing"
status: Pending
type: implementation
blocked_by: []
unlocks: []
severity: High
target_repo: plexus-substrate
---

## Problem

`claudecode_loopback/activation.rs:114` (approximate) contains `let _ = storage.resolve_approval(...)`. This is the code path that resolves timed-out approval requests — a dropped error here means timeouts silently vanish, approval state stays in an inconsistent "unresolved" shape on disk, and downstream callers (Orcha's graph runner, ClaudeCode's tool-use pairing) see stale approvals.

Audit summary: "Timeout resolution failures vanish."

## Context

`storage.resolve_approval(...)` updates the approval record to a terminal state (approved / denied / timeout). Its failure modes are database-level: pool exhaustion, constraint violation, connection drop.

Loopback owns a `LoopbackError` enum (naming may differ — `ClaudeCodeLoopbackError` is the likely actual name). The required change is:

1. Add a structured variant for approval-resolution failures carrying the `ApprovalId` (ST newtype if ST has shipped; bare `Uuid` or `String` otherwise) and the attempted target state.
2. At the call site, decide per the same rules as RL-5: propagate with `?` if the enclosing function returns `Result`; otherwise log at ERROR level with the full context and continue. Per-site comment states which mode and why.

## Required behavior

| Loopback resolve_approval call | Current observable behavior | Required observable behavior |
|---|---|---|
| `storage.resolve_approval` succeeds | Normal path | Unchanged. |
| `storage.resolve_approval` fails in a timeout path (return type `()`) | Silently dropped; approval stays unresolved on disk; no log | Tracing ERROR event logged with `ApprovalId` and target state; approval state left as HEAD leaves it (this ticket does not change recovery semantics). Function continues if the enclosing path requires it. |
| `storage.resolve_approval` fails in an RPC-method path (return type `Result`) | Silently dropped; method returns success | Error propagates; RPC method returns failure with the structured variant. |
| Approval resolution succeeds (regression) | Expected response shape | Unchanged. |

## Risks

- **Single call site or multiple?** Audit pinpoints line 114. Implementor greps `let _ = storage.resolve_approval` across `claudecode_loopback/` at HEAD. If more than one site exists, fix all of them with the same convention.
- **Timeout reaper loop.** If the resolve_approval call lives in a background reaper task (spawned from Loopback's init), a failure could repeat indefinitely as the reaper retries. RL-6 does not implement backoff — it only adds logging. If retry-storm behaviour emerges, a follow-up ticket is opened to add backoff; this ticket is not the place.

## What must NOT change

- The set of RPC methods on the Loopback activation or their request/response shapes.
- Loopback's SQLite schema.
- The `ApprovalId` type (either ST's newtype if it has shipped, or the existing `ApprovalId(Uuid)` in Loopback — preserved per audit "already-newtyped concepts").
- The semantics of when approvals are resolved (timeout behaviour, client resolution, agent resolution). This ticket changes only *what happens when the resolve call fails*.
- Existing `cargo test` pass rate.
- Files outside `claudecode_loopback/activation.rs` and `claudecode_loopback/error.rs`.

## Acceptance criteria

1. Grep for `let _ = storage.resolve_approval` in `claudecode_loopback/` returns zero matches.
2. `claudecode_loopback/error.rs` has a new structured variant for approval-resolution failures, carrying `ApprovalId` and target state.
3. Each previously-swallowed site either `?`-propagates or has an explicit `match`/`if let Err(e)` block logging at ERROR with the variant's context.
4. A unit test forces `storage.resolve_approval` to fail (via mock storage or a forced-error harness) and asserts the tracing ERROR event is emitted (for a log-and-continue site) or the method's `Err` matches the variant (for a propagation site).
5. All existing `cargo test` targets pass.

## Completion

Implementor delivers:

- Patch to `claudecode_loopback/activation.rs` replacing each `let _ = storage.resolve_approval` site.
- Patch to `claudecode_loopback/error.rs` adding the new variant.
- At least one unit test per criterion 4.
- `cargo test` output confirming criterion 5.
- Status flip to `Complete` in the same commit that lands the code.
