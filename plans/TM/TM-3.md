---
id: TM-3
title: "TM CRUD RPC methods (create, get, update body, update status, delete, epic create)"
status: Pending
type: implementation
blocked_by: [TM-2]
unlocks: [TM-7, TM-8, TM-9]
severity: High
target_repo: plexus-substrate
---

## Problem

With `TicketStore` and the ticket types pinned, TM still exposes no callable surface. This ticket introduces the TM activation (`namespace = "tm"`) and its mutation methods — create, read-by-id, update body, update status (non-gated), delete, plus epic create / get. Read-side multi-ticket queries are owned by TM-4; watch streams by TM-5; the human promotion gate by TM-6. Those four tickets run in parallel with this one on disjoint files.

## Context

Target repo: `plexus-substrate`.

New file introduced by this ticket: `src/activations/tm/activation.rs`. The activation wraps a `Box<dyn TicketStore>` (from TM-2) and registers with the substrate plugin system the same way `orcha/pm`'s activation does (see `src/activations/orcha/pm/activation.rs` for the pattern).

Macro usage: `#[plexus_macros::activation(namespace = "tm", version = "0.1")]`.

The activation's methods use `#[plexus_macros::method]`. No children in this ticket (no `#[child]`). Hub-mode is not needed.

Input / output types for each RPC method use the domain types from TM-2 where possible, and tagged-enum result types for error paths (matching the `PmGraphStatusResult` pattern in `orcha/pm/activation.rs`).

`update_ticket_status` in this ticket does **not** enforce the `Pending → Ready` human gate. It accepts any caller and any valid state-machine transition. TM-6 wraps the `Ready`-promotion path behind auth. Orcha's automated transitions (e.g., `Ready → Complete`) flow through `update_ticket_status` unchanged.

## Required behavior

### RPC methods on the `tm` activation

| Method | Args | Return | Behavior |
|---|---|---|---|
| `create_ticket` | `ticket: Ticket` | `TmCreateResult` | Inserts via `TicketStore::create_ticket`. Returns `Ok { id }` on success, `AlreadyExists { id }` or `Err { message }` otherwise. Rejects `Ticket { status: Ready, .. }` — new tickets always land as `Pending` regardless of the submitted value. |
| `get_ticket` | `id: TicketId` | `TmGetResult` | Returns `Ok { ticket }`, `NotFound { id }`, or `Err { message }`. |
| `update_ticket_body` | `id: TicketId, body: String` | `TmUpdateResult` | Replaces the ticket body. Last-write-wins. Returns `Ok { id }` or an error. |
| `update_ticket_status` | `id: TicketId, status: Status` | `TmUpdateResult` | Transitions status using the table below. Does **not** enforce the human gate (TM-6 wraps the `Pending → Ready` path). Returns `Ok { id }`, `InvalidTransition { from, to }`, or an error. |
| `delete_ticket` | `id: TicketId` | `TmDeleteResult` | Removes via store. Returns `Ok { id }`, `Referenced { id, by }`, `NotFound { id }`, or an error. |
| `create_epic` | `epic: Epic` | `TmCreateResult` | Inserts via `TicketStore::create_epic`. Idempotent on re-create of an identical epic record. |
| `get_epic` | `prefix: String` | `TmGetEpicResult` | Returns `Ok { epic }` or `NotFound { prefix }`. |

### Valid status transitions

TM-3 enforces the state machine at the RPC layer. `update_ticket_status` accepts:

| From | Allowed transitions to |
|---|---|
| `Pending` | `Ready`, `Blocked`, `Idea`, `Superseded` |
| `Ready` | `Blocked`, `Complete`, `Superseded` |
| `Blocked` | `Ready`, `Superseded` |
| `Idea` | `Pending`, `Superseded` |
| `Complete` | (terminal; no outgoing) |
| `Superseded` | (terminal; no outgoing) |
| `Epic` | (no transitions; Epic records do not participate) |

Any other transition returns `InvalidTransition { from, to }`.

### Result types

`TmCreateResult`, `TmGetResult`, `TmUpdateResult`, `TmDeleteResult`, `TmGetEpicResult` are tagged enums (`#[serde(tag = "type", rename_all = "snake_case")]`) mirroring the `PmGraphStatusResult` style in `orcha/pm/activation.rs`. Variants: `ok`, the failure modes above, and a catch-all `err { message: String }`.

### Activation registration

The TM activation is registered with the substrate plugin system in `src/plugin_system/` the same way every other activation is. The activation is constructed with `SqliteTicketStore` as its default backend.

### Module wiring

- `src/activations/tm/mod.rs` re-exports the activation and types.
- `src/activations/mod.rs` adds `pub mod tm;`.
- The plugin registry adds a `TmActivation::new(...)` instantiation.

## Risks

| Risk | Mitigation |
|---|---|
| Status transition table conflicts with a realistic workflow. | Transitions are a pinned table in this ticket; changes require a new ticket, not silent drift. |
| `create_ticket` submitted with `status: Ready` would bypass the TM-6 gate. | This ticket rejects `status: Ready` on create (all new tickets land `Pending`). TM-6 adds auth on the `Pending → Ready` transition. Both gates combine correctly: there is no path to `Ready` for a non-human caller. |
| Schema drift in `Ticket` between TM-2 and this ticket. | Shared types file (`types.rs`) is authoritative. TM-3 only adds result enums, never re-declares `Ticket`. |

## What must NOT change

- TM-2's `TicketStore` trait surface (this ticket consumes it, does not modify it).
- Every other substrate activation's compile and test behavior.
- The activation registration pattern — follows the existing convention exactly.
- Synapse's ability to introspect other activations.

## Acceptance criteria

1. `cargo build -p plexus-substrate` succeeds with the TM activation registered.
2. `cargo test -p plexus-substrate` succeeds. New tests cover:

   | Scenario | Expected |
   |---|---|
   | `create_ticket` with a valid `Pending` ticket | `ok { id }`, and `get_ticket` returns the same ticket. |
   | `create_ticket` with `status: Ready` | Rejected; ticket persisted as `Pending`. |
   | `create_ticket` twice with the same id | Second call returns `AlreadyExists { id }`. |
   | `get_ticket` on absent id | `NotFound { id }`. |
   | `update_ticket_body` round-trip | `ok { id }`; `get_ticket` returns the new body; `updated_at` increased. |
   | `update_ticket_status` `Pending → Ready` | `ok { id }` (no auth at this layer yet; TM-6 adds it). |
   | `update_ticket_status` `Complete → Ready` | `InvalidTransition { from: Complete, to: Ready }`. |
   | `update_ticket_status` `Pending → Complete` | `InvalidTransition { from: Pending, to: Complete }`. |
   | `delete_ticket` on a leaf | `ok { id }`. |
   | `delete_ticket` on a referenced ticket | `Referenced { id, by }` listing the referrers. |
   | `create_epic` + `get_epic` | Round-trip matches. |

3. Running `synapse tm` (or the equivalent local-loopback call) surfaces all seven methods with their parameter names and types.
4. The activation appears in substrate's plugin registry listing.

## Completion

- PR adds `src/activations/tm/activation.rs` and updates `src/activations/mod.rs` + plugin registration.
- PR description includes `cargo build -p plexus-substrate` and `cargo test -p plexus-substrate` output — both green, plus the output of `synapse tm` showing the seven methods.
- Ticket status flipped from `Ready` → `Complete` in the same commit as the code.
