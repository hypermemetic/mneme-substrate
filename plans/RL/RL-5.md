---
id: RL-5
title: "Fix Orcha pm.save_* error swallowing (7 sites in orcha/activation.rs)"
status: Pending
type: implementation
blocked_by: [RL-3]
unlocks: []
severity: High
target_repo: plexus-substrate
---

## Problem

`orcha/activation.rs` has seven `let _ = pm.save_*(...)` (or equivalent — approximate lines per the audit: 1411, 1468, 1842, 1916, 2067, 2193, 2292). Each is a ticket-state persistence call whose failure is silently dropped. Ticket state on disk can diverge from Orcha's in-memory state with zero observable signal — no log, no metric, no error propagated to the caller.

Audit summary: "Ticket state can diverge from reality with no logs."

## Context

`pm` is Orcha's ticket-persistence manager. Its `save_*` methods return `Result<_, ...>`. The current code pattern `let _ = pm.save_foo(...);` throws the error away.

Two pieces of the fix need to be kept separate:

1. **Structured error variant** in `OrchaError` for persistence failures. RL-3 lands first and introduces variant-shape conventions; RL-5 adds persistence-failure variants against the same file. Expected shape:
   ```
   OrchaError::PersistenceFailed {
       operation: &'static str,   // "save_ticket_map", "save_graph", etc.
       graph_id: Option<GraphId>,
       ticket_id: Option<TicketId>,
       cause: String,             // or a source: Box<dyn Error> if preferred
   }
   ```
   (`GraphId` / `TicketId` are ST's newtypes if ST has shipped; bare `String` otherwise.)

2. **Per-site handling.** Each of the seven sites needs a deliberate choice:
   - **Propagate.** The enclosing function returns `Result`; replace `let _ = ...` with `?` and let the error reach the caller.
   - **Log and continue.** The enclosing function returns `()` or the ticket is in a non-recoverable path (e.g., already in a failure handler); replace `let _ = ...` with a `match { Ok(_) => {}, Err(e) => tracing::error!(...) }` that emits a structured ERROR event carrying the variant's context.

No site should remain `let _ = ...`. The "log and continue" choice is explicit — not a default.

`orcha/error.rs` is **shared with RL-3**. This ticket rebases on RL-3. If RL-3's variants collide by name, rename here rather than in RL-3.

## Required behavior

| Orcha persistence call | Current observable behavior | Required observable behavior |
|---|---|---|
| `pm.save_*` succeeds | Normal path | Unchanged. |
| `pm.save_*` fails and the enclosing function returns `Result` | Error silently dropped; in-memory state diverges from disk; no log | Error propagates to the caller as `OrchaError::PersistenceFailed { ... }`. Tracing ERROR event logged. |
| `pm.save_*` fails and the enclosing function returns `()` | Error silently dropped | Tracing ERROR event logged with the full variant context (operation, graph_id / ticket_id, cause). Function continues. Per-site comment in source explains why propagation is not possible here. |
| Orcha RPC method that writes ticket state (e.g., `graphs.advance`) is called, persistence fails | Method returns success; caller believes state is saved | Method returns failure with the persistence error surfaced via the method's declared error shape. |

## Risks

- **Site count drift.** The audit pins 7 sites (approximate lines 1411, 1468, 1842, 1916, 2067, 2193, 2292). Line numbers will drift. Implementor greps for `let _ = pm.save` across `orcha/activation.rs` at HEAD and updates each match. If the count is materially different (> 10 or < 5), stop and note the discrepancy in the ticket before proceeding.
- **Non-recoverable paths.** Some save sites run inside a failure handler (e.g., marking a graph as failed). Propagating a save error there would mask the primary failure. The "log and continue" mode exists for exactly these — the per-site comment must state the reason.

## What must NOT change

- The set of RPC methods on Orcha's activation or their request/response shapes. Error-response shapes may gain a new variant via `OrchaError`, but successful responses are unchanged.
- The Orcha SQLite schema.
- `pm`'s public API (no change to `save_*` signatures).
- Existing `cargo test` pass rate.
- Files outside `orcha/activation.rs` and `orcha/error.rs`.

## Acceptance criteria

1. Grep for `let _ = pm.save` in `orcha/activation.rs` returns zero matches.
2. Grep for `let _ = pm.` in `orcha/activation.rs` returns zero matches (catches any related swallowing).
3. `orcha/error.rs` has a new `PersistenceFailed` variant (or equivalent) carrying `operation`, optional ids, and cause.
4. Every site that was `let _ = pm.save_*` now either `?`-propagates or has an explicit `match`/`if let Err(e)` block that logs at ERROR level via `tracing` with the operation name and available ids.
5. Every "log and continue" site has a one-line comment stating why propagation is not possible.
6. A unit test in the `orcha` module forces `pm.save_ticket_map` (or any one of the touched save methods) to fail (via a mock pm or a forced-error store) and asserts the Orcha-level method's return reflects the persistence failure (for a propagation site) or that the tracing ERROR event is emitted (for a log-and-continue site; use `tracing-test` or equivalent).
7. All existing `cargo test` targets pass.

## Completion

Implementor delivers:

- Patch to `orcha/activation.rs` replacing each of the (approx) 7 `let _ = pm.save_*` sites.
- Patch to `orcha/error.rs` adding the `PersistenceFailed` variant (rebased on RL-3's additions).
- At least one unit test per criterion 6.
- `cargo test` output confirming criterion 7.
- Status flip to `Complete` in the same commit that lands the code.
