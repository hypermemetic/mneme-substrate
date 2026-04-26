---
id: TM-6
title: "TM human promotion gate — auth-gated Pending → Ready"
status: Pending
type: implementation
blocked_by: [TM-2, TM-3]
unlocks: [TM-7]
severity: High
target_repo: plexus-substrate
---

## Problem

Tickets are contracts that authorize work (per `~/CLAUDE.md` → "Ticket Approval"). Machine agents — including Claude, Orcha, and any future automated client — must never be able to promote a ticket from `Pending` to `Ready`. Only authenticated humans can. TM-3's `update_ticket_status` enforces state-machine validity but not caller identity. This ticket wraps the `Pending → Ready` transition specifically behind an auth check.

## Context

Target repo: `plexus-substrate`. Target file: `src/activations/tm/activation.rs` (new `promote` method) and a new `src/activations/tm/auth.rs` helper.

Substrate's RPC transport exposes a per-call auth context today via the `Activation` method signatures (every handler receives a context object carrying session/user metadata). The concrete shape of that context lives in `src/plexus/` — this ticket's first action is to confirm the exact current shape and use it. If the auth context doesn't today carry "is human caller", this ticket's spike-like first step is to surface that bit through the transport.

Design call: the gate lives at the RPC surface, not in `TicketStore` or in `update_ticket_status`. Reason: Orcha still needs to transition `Ready → Complete` as a machine agent. Gating the store or the generic status transition would also block those. Isolating the gate to a distinct `promote` method narrows the auth check to exactly the operation that requires human judgment.

`update_ticket_status` is modified in this ticket to **refuse** the specific `Pending → Ready` transition and return a new `RequiresPromote` variant directing callers to the gated method. All other transitions continue to work through `update_ticket_status`.

## Required behavior

### New RPC method

| Method | Args | Return | Behavior |
|---|---|---|---|
| `promote` | `id: TicketId` | `TmPromoteResult` | If caller is not an authenticated human, returns `NotAuthorized { caller }`. Else loads the ticket, verifies `status == Pending`, writes `status = Ready`, publishes the `status_changed` event (via TM-5's publish helper), returns `ok { id }`. |

### Result type

Tagged enum `TmPromoteResult`:

| Variant | Fields | Meaning |
|---|---|---|
| `ok` | `id: TicketId` | Promoted. |
| `not_authorized` | `caller: String` | Caller is not an authenticated human. |
| `not_found` | `id: TicketId` | No such ticket. |
| `invalid_state` | `id, status` | Ticket is not `Pending`. |
| `err` | `message: String` | Other failure. |

### Modification to `update_ticket_status` (TM-3's method)

Add a single case: if `from == Pending` and `to == Ready`, return `TmUpdateResult::RequiresPromote { id }` without mutating state. Any caller (human or machine) that submits this transition is redirected to `promote`. This keeps the state-machine surface honest.

### `Caller` / auth-context shape

A new `src/activations/tm/auth.rs` introduces:

```rust
pub enum Caller {
    Human { user_id: String },
    Agent { agent_id: String },
    Unauthenticated,
}

pub fn caller_from_context(ctx: &<substrate's context type>) -> Caller;
```

`caller_from_context` inspects the incoming request context. The exact shape of `ctx` is whatever substrate's current transport layer exposes. The mapping rules:

| Context shape | Returns |
|---|---|
| Context carries an authenticated user id tagged `human` (or equivalent) | `Caller::Human { user_id }` |
| Context carries an authenticated agent/machine token | `Caller::Agent { agent_id }` |
| No auth token, or token fails validation | `Caller::Unauthenticated` |

If the current transport does **not** distinguish human from agent callers, this ticket must extend it minimally to do so. That extension is in scope: add a single enum-valued field (e.g., `caller_kind: CallerKind`) on the auth context. A negative extension (remove or rename) is out of scope — this ticket is additive only.

### Logging / audit

Every successful `promote` writes an audit row via `TicketStore`. This ticket adds one method to the trait:

```rust
async fn record_promotion(
    &self,
    id: &TicketId,
    caller: &Caller,
    at: i64,
) -> Result<(), TmError>;
```

Backed by a new `tm_promotions` table in `SqliteTicketStore` (id, caller_kind, caller_id, at). This is the audit log — consulted for governance but not surfaced as an RPC method in this ticket.

## Risks

| Risk | Mitigation |
|---|---|
| Substrate's current transport has no way to say "this caller is a human". | This ticket adds that bit to the auth context as a minimal extension. If adding it is structurally large, split into TM-6a (extend context) + TM-6b (consume it in promote) before promotion. |
| A misconfigured transport routes every caller as `Unauthenticated`, blocking all promotes. | `Unauthenticated` never promotes; the user sees `NotAuthorized` and knows to check their auth setup. Never a silent success. |
| An `Agent` caller circumvents by calling `update_ticket_status(Pending, Ready)`. | TM-3's method now rejects that specific transition with `RequiresPromote`. The only path to Ready is `promote`. |
| Local dev / single-user mode has no auth. | In that mode, the context maps every caller to `Caller::Human { user_id: "local" }`. Pinned in TM-6's auth.rs. This is the only mode that ships "open". |

## What must NOT change

- TM-3's `update_ticket_status` surface for every transition other than `Pending → Ready`. All other transitions behave exactly as before.
- TM-2's `TicketStore` (this ticket extends it additively with `record_promotion`).
- Other activations' auth behavior — any context-shape extension is additive and backward-compatible.
- Every other substrate activation's test suite.

## Acceptance criteria

1. `cargo build -p plexus-substrate` succeeds.
2. `cargo test -p plexus-substrate` succeeds. Tests cover:

   | Scenario | Expected |
   |---|---|
   | `promote(id)` from a `Caller::Human` on a `Pending` ticket | `ok { id }`; ticket is `Ready`; audit row written. |
   | `promote(id)` from a `Caller::Agent` | `not_authorized { caller }`; ticket still `Pending`; no audit row. |
   | `promote(id)` from `Caller::Unauthenticated` | `not_authorized { caller }`. |
   | `promote(id)` on a `Ready` ticket | `invalid_state { id, status: Ready }`. |
   | `promote(id)` on an absent id | `not_found { id }`. |
   | `update_ticket_status(Pending, Ready)` from any caller | `RequiresPromote { id }`; ticket still `Pending`. |
   | `update_ticket_status(Ready, Complete)` from `Caller::Agent` | `ok { id }`; ticket is `Complete`. |

3. A human using `synapse tm promote TM-DEMO-1` from a configured-as-human synapse session flips the ticket to `Ready`.
4. A script using the agent-facing SDK/transport to call `promote` on the same ticket receives `not_authorized`.

## Completion

- PR adds the `promote` method, the modified `update_ticket_status` short-circuit, the `Caller` enum + `caller_from_context` helper, the `record_promotion` store method, the `tm_promotions` table, and the audit integration.
- PR description includes `cargo build -p plexus-substrate` + `cargo test -p plexus-substrate` output — both green — plus a transcript showing the two promote attempts (human succeeds, agent fails).
- Ticket status flipped from `Ready` → `Complete` in the same commit as the code.
