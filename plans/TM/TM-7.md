---
id: TM-7
title: "TM Orcha integration — library API for ready-ticket pull and status write-back"
status: Pending
type: implementation
blocked_by: [TM-3, TM-4, TM-5, TM-6]
unlocks: []
severity: High
target_repo: plexus-substrate
---

## Problem

Today, Orcha reads ticket source text from `plans/<EPIC>/*.md` files (via `orcha/pm`'s `orcha_ticket_sources` table, populated by the compile step). After TM ships, Orcha should instead pull ready tickets directly from TM and write status transitions back as it starts and completes graphs. This ticket integrates the two activations via library API, per DC conventions (not wire-level for the hot path).

## Context

Target repo: `plexus-substrate`.

Orcha lives at `src/activations/orcha/` and has several moving parts: `activation.rs`, `graph_runner.rs`, `graph_runtime.rs`, `orchestrator.rs`, `ticket_compiler.rs`. The ticket-compile path (`ticket_compiler.rs`) already parses the `# ID: Title [type]` format and builds a graph DAG — that remains Orcha's responsibility and is unchanged.

What this ticket changes:

1. **Ticket intake.** Instead of reading files, Orcha calls `TmActivation::get_ticket` / `ready` via a library-API handle. The handle is threaded through Orcha's construction (or pulled from a shared registry).
2. **Status write-back.** On graph start, Orcha calls `update_ticket_status(id, Status::Ready)` on tickets that were promoted but not yet in-flight (no-op if already Ready). On graph success, Orcha calls `update_ticket_status(id, Status::Complete)`. On graph failure, Orcha calls `update_ticket_status(id, Status::Blocked)`.
3. **Event subscription.** Orcha subscribes to TM's `watch_all` stream and reacts to `status_changed` events where `to == Ready` by enqueuing the ticket for compile + run.

Library API exposure: TM's activation exposes a `TmHandle` — a cheap `Clone`-able struct holding `Arc<dyn TicketStore>` + `broadcast::Sender<TicketEvent>`. Orcha receives a `TmHandle` at construction. This matches the DC convention: cross-activation calls go through a curated library surface, not via reaching into sibling internals.

If TM-S01 decided absorb: `orcha/pm`'s current responsibilities are now served by TM, and the deletion of `pm` is a follow-up ticket outside this epic.
If TM-S01 decided coexist: Orcha continues to use `pm` for graph-to-ticket mappings and node logs, and only its ticket-lifecycle calls (read "ready" tickets, write status transitions) go through TM.

## Required behavior

### `TmHandle`

New public struct in `src/activations/tm/handle.rs`:

```rust
#[derive(Clone)]
pub struct TmHandle {
    store: Arc<dyn TicketStore + Send + Sync>,
    events: broadcast::Sender<TicketEvent>,
}
```

Methods (library API, not RPC):

| Method | Signature | Notes |
|---|---|---|
| `get_ticket` | `async fn get_ticket(&self, id: &TicketId) -> Result<Option<Ticket>, TmError>` | Direct store call. |
| `ready_tickets` | `async fn ready_tickets(&self) -> Result<Vec<Ticket>, TmError>` | `list_by_status(Ready)`. |
| `set_status` | `async fn set_status(&self, id: &TicketId, status: Status) -> Result<(), TmError>` | Validates the transition (same state machine as TM-3). Publishes event. **Does not** bypass TM-6's promote gate — if called with `Pending → Ready`, returns `TmError::RequiresPromote`. |
| `subscribe` | `fn subscribe(&self) -> broadcast::Receiver<TicketEvent>` | Live event tap. |

The `TmActivation` constructor exposes `fn handle(&self) -> TmHandle`.

### Orcha wiring

- `OrchaActivation` takes an optional `TmHandle` at construction. Provided by the substrate plugin system when TM is registered; absent in standalone Orcha tests.
- A new Orcha-internal module `orcha/tm_sync.rs` owns:
  - A background task that subscribes to `TmHandle::subscribe()`, filters for `status_changed { to: Ready, .. }`, and forwards the ticket to Orcha's existing compile queue.
  - Hooks inserted into Orcha's graph-lifecycle points (`graph_start`, `graph_complete`, `graph_failed`) to call `tm_handle.set_status(...)`.
- When `TmHandle` is absent (`None`), Orcha behaves exactly as before — it reads from `plans/` files and `orcha/pm`. This is the pre-TM compatibility mode; it is kept until a follow-up epic retires it.

### Ticket → graph compile flow

When a Ready ticket arrives via the TM watch stream:

1. Orcha fetches the full ticket via `tm_handle.get_ticket(id)`.
2. Orcha reconstructs the source text the compiler expects from the ticket's `body`, `id`, and the `[type]` tag (derived from `ticket_type`).
3. Orcha calls `ticket_compiler::compile_tickets` on the reconstructed text.
4. Orcha runs the graph as usual.

If TM-S01 decided absorb: step 2's `graph_id → ticket_id → node_id` mapping is persisted into TM via the `TmHandle`'s absorb-expanded methods. If coexist: it continues into `orcha/pm`.

### Status transitions issued by Orcha

| Graph event | Ticket id | Transition |
|---|---|---|
| `graph_start(graph_id)` | Each ticket in the graph | (currently Ready, stays Ready — no transition needed; optionally emit a custom "started" event in a follow-up) |
| `graph_complete(graph_id)` | Each successfully completed ticket | `Ready → Complete` |
| `graph_failed(graph_id, reason)` | Each failed ticket | `Ready → Blocked` |
| Per-node failure triggering retry | (no transition — Orcha retries internally) | — |

## Risks

| Risk | Mitigation |
|---|---|
| TM-S01 coexist + Orcha read path now spans two storage systems. | That is exactly the seam TM-S01 pins. Orcha reads ticket lifecycle from TM, ticket-to-graph mapping from `pm`. Seam document is the authoritative reference. |
| `TmHandle` is absent and Orcha regresses silently. | Construction-time log: `OrchaActivation started {with,without} TmHandle`. The without-TmHandle path is preserved for tests and for backward compatibility. |
| Race: Orcha marks a ticket `Complete`, then a human re-promotes. | `Complete` is terminal (TM-3's state machine). A human attempting `Complete → Ready` via promote returns `invalid_state`. If re-running is needed, the human must delete and re-create the ticket or use a future "reopen" method (out of scope). |
| `watch_all` subscribers can miss events under load. | TM-5's lag handling applies: Orcha treats `TicketEvent::Lagged` as a cue to re-run `ready_tickets()` for reconciliation. |

## What must NOT change

- Orcha's graph compile, run, retry, and event semantics for any graph not originating from a TM ticket.
- `orcha/pm`'s schema and RPC surface (unless TM-S01 decided absorb, in which case the deletion is a follow-up).
- TM-3/4/5/6 method signatures.
- Every other substrate activation's behavior.

## Acceptance criteria

1. `cargo build -p plexus-substrate` succeeds.
2. `cargo test -p plexus-substrate` succeeds. Tests cover:

   | Scenario | Expected |
   |---|---|
   | Orcha starts with `TmHandle: None` | Reads from `plans/` exactly as before (regression pin). |
   | Orcha starts with `TmHandle: Some(..)`, TM has 1 Ready ticket | Within 500ms of startup, Orcha has picked up the ticket and queued a compile. |
   | A human promotes a new ticket via `TmHandle::set_status` indirect (i.e., via TM-6's promote) → Orcha receives `status_changed` → compiles and runs → calls `set_status(..., Complete)` | Ticket ends as `Complete` in TM, including on the exported `plans/<EPIC>/*.md` if TM-8 is also integrated. |
   | Orcha graph_failed on a TM ticket | Ticket status in TM is `Blocked`. |
   | `TmHandle::set_status(id, Pending → Ready)` direct call | Returns `TmError::RequiresPromote`. Orcha never performs this transition. |

3. End-to-end demo (captured in the PR): create a ticket via `synapse tm create_ticket`, promote it via `synapse tm promote`, observe Orcha compile + run + transition to `Complete`. Transcript in the PR.
4. `synapse tm watch_all` run alongside shows every transition as it happens.

## Completion

- PR adds `src/activations/tm/handle.rs`, wires `TmHandle` into `TmActivation`, adds `orcha/tm_sync.rs`, and inserts the graph-lifecycle hooks in Orcha.
- PR description includes `cargo build -p plexus-substrate`, `cargo test -p plexus-substrate`, and the end-to-end demo transcript.
- Ticket status flipped from `Ready` → `Complete` in the same commit as the code.
