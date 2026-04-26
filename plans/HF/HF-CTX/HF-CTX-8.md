---
id: HF-CTX-8
title: "Human promotion gate: promote_ticket enforces authenticated human caller"
status: Pending
type: implementation
blocked_by: [HF-CTX-7]
unlocks: [HF-CTX-9]
severity: High
target_repo: hyperforge
---

## Problem

Tickets are contracts that authorize work (per `~/CLAUDE.md` → "Ticket Approval"). Machine agents — Orcha, Claude, any future automated client — must never promote a ticket from `Pending` to `Ready`. Only authenticated humans can. HF-CTX-3's `update_status` enforces state-machine validity and returns `requires_promote { id }` on `Pending → Ready`, redirecting callers here. This ticket adds the gated `promote_ticket` RPC method on `hyperforge.ctx` and the auth-context machinery that distinguishes human from agent callers.

## Context

Target repo: `hyperforge`. Target files: `src/ctx/promote.rs` (new method implementation) and `src/ctx/auth.rs` (new caller-identity helper).

Hyperforge's RPC transport already exposes a per-call auth context via the `Activation` method signatures. The concrete shape of that context lives in hyperforge's auth module (see `src/auth/`, `src/auth_hub/`). This ticket's first step is to confirm the current shape and — if it doesn't today carry "is human caller" — extend it additively with a `CallerKind` field.

Design call (per HF-CTX-1 + TM-6): the gate lives at the RPC surface, not in `TicketStore` or in `update_status`. Reason: machine agents still need to transition `Ready → Complete` on graph completion. Gating the store would block those. Isolating the gate to a distinct `promote_ticket` method narrows the auth check to exactly the operation that requires human judgment.

`update_status` (from HF-CTX-3) already refuses the `Pending → Ready` transition and returns `requires_promote { id }`. This ticket keeps that short-circuit in place; the only path to `Ready` is `promote_ticket`.

Emission: a successful promote publishes a `TicketEvent::status_changed { id, from: Pending, to: Ready, ... }` (via HF-CTX-6's publish helper) and appends a `Fact::TicketStatusChanged { ticket_id, from: Pending, to: Ready, at_commit }` (if commit context is known, else `None`). Both emissions are integrated — watchers and fact consumers see the promotion like any other status change.

## Required behavior

### New RPC method

| Method | Args | Return | Behavior |
|---|---|---|---|
| `promote_ticket` | `id: TicketId` | `CtxPromoteResult` | If caller is not `Caller::Human`, returns `not_authorized { caller }`. Else loads the ticket, verifies `status == Pending`, writes `status = Ready`, publishes `status_changed` event, appends `TicketStatusChanged` fact, writes audit row, returns `ok { id }`. |

### Result type

Tagged enum `CtxPromoteResult` (`#[serde(tag = "type", rename_all = "snake_case")]`):

| Variant | Fields | Meaning |
|---|---|---|
| `ok` | `id: TicketId` | Promoted. |
| `not_authorized` | `caller: String` | Caller is not `Caller::Human`. |
| `not_found` | `id: TicketId` | No such ticket. |
| `invalid_state` | `id: TicketId, status: Status` | Ticket is not `Pending`. |
| `err` | `message: String` | Other failure. |

### `Caller` / auth-context shape

Introduce in `src/ctx/auth.rs`:

```text
pub enum Caller {
    Human { user_id: String },
    Agent { agent_id: String },
    Unauthenticated,
}

pub fn caller_from_context(ctx: &<hyperforge's context type>) -> Caller;
```

Mapping rules:

| Context shape | Returns |
|---|---|
| Context carries an authenticated user id tagged `human` (or equivalent) | `Caller::Human { user_id }` |
| Context carries an authenticated agent/machine token | `Caller::Agent { agent_id }` |
| No auth token, or token fails validation | `Caller::Unauthenticated` |

If hyperforge's current transport does **not** distinguish human from agent callers, this ticket extends the auth context additively — adding a single enum-valued field (e.g., `caller_kind: CallerKind`) on the auth context. Extension is backward-compatible; existing callers that don't set the field default to `Unauthenticated`.

Local dev / single-user mode: if the context indicates "local session" (e.g., loopback transport with no remote identity), map to `Caller::Human { user_id: "local" }`. This is the only mode that ships open. Pinned in `auth.rs`.

### Audit log

Every successful `promote_ticket` writes an audit row via a new `TicketStore` method:

```text
async fn record_promotion(
    &self,
    id: &TicketId,
    caller: &Caller,
    at: i64,
) -> Result<(), CtxError>;
```

Backed by a new `promotions` table in `SqliteTicketStore` (and the equivalent map in `InMemoryTicketStore`):

| Column | Type |
|---|---|
| `ticket_id` | TEXT |
| `caller_kind` | TEXT |
| `caller_id` | TEXT |
| `at` | INTEGER |

Consulted for governance; not surfaced as an RPC method in this ticket.

### Fact emission

On a successful promote, append a `Fact::TicketStatusChanged { ticket_id, from: Pending, to: Ready, at_commit: None }` record to the fact sink. Rationale: promotion is a fact in the knowledge graph; `who_touches` and audit queries should see it.

## Risks

| Risk | Mitigation |
|---|---|
| Hyperforge's current transport has no way to distinguish human from agent callers. | This ticket adds the bit to the auth context additively. If adding it is structurally large, split into HF-CTX-8a (extend context) + HF-CTX-8b (consume it in promote) before promotion. |
| A misconfigured transport routes every caller as `Unauthenticated`, blocking all promotes. | `Unauthenticated` never promotes; user sees `not_authorized` and knows to check auth setup. Never a silent success. |
| An `Agent` caller circumvents by calling `update_status(Pending, Ready)`. | HF-CTX-3's method returns `requires_promote { id }` without mutation. Only path to Ready is `promote_ticket`. |
| Loopback sessions in tests default to `Unauthenticated`, breaking existing tests. | Tests use a helper that wraps loopback contexts as `Human { user_id: "test" }`. Pinned in the test module. |
| Promotion audit table grows unbounded. | No growth concern at hyperforge scale. Future retention policy is out of scope. |

## What must NOT change

- HF-CTX-3's `update_status` behavior for every transition other than `Pending → Ready`. All other transitions behave exactly as before.
- HF-CTX-2's existing `TicketStore` methods. This ticket additively adds `record_promotion`.
- HF-CTX-4's fact-emission hooks.
- HF-CTX-5's / HF-CTX-6's / HF-CTX-7's surfaces.
- Every other hyperforge hub's behavior.
- Other hyperforge auth callers — any context-shape extension is additive and backward-compatible.

## Acceptance criteria

1. `cargo build --workspace` succeeds in hyperforge.
2. `cargo test --workspace` succeeds. New tests cover:

   | Scenario | Expected |
   |---|---|
   | `promote_ticket(id)` from `Caller::Human` on a `Pending` ticket | `ok { id }`; ticket is `Ready`; audit row written; `TicketStatusChanged` fact appended. |
   | `promote_ticket(id)` from `Caller::Agent` | `not_authorized { caller }`; ticket still `Pending`; no audit row; no fact. |
   | `promote_ticket(id)` from `Caller::Unauthenticated` | `not_authorized { caller }`. |
   | `promote_ticket(id)` on a `Ready` ticket | `invalid_state { id, status: Ready }`. |
   | `promote_ticket(id)` on an absent id | `not_found { id }`. |
   | `update_status(Pending, Ready)` from any caller | `requires_promote { id }`; ticket still `Pending` (regression pin on HF-CTX-3). |
   | `update_status(Ready, Complete)` from `Caller::Agent` | `ok { id }` (regression pin — agents can complete). |
   | Subscribing to `watch_ticket(id)` while a promote runs | Receives `status_changed { from: Pending, to: Ready }`. |
   | Subscribing to `watch_facts(FactFilter { ticket: Some(id), .. })` while a promote runs | Receives a `TicketStatusChanged` fact record. |

3. A human using `synapse hyperforge ctx promote_ticket <id>` from a configured-as-human session flips the ticket to `Ready`.
4. A script using the agent-facing SDK/transport calling `promote_ticket` on the same ticket receives `not_authorized`.
5. `cargo build --workspace` + `cargo test --workspace` pass green end-to-end.

## Completion

- Commit adds `src/ctx/promote.rs`, `src/ctx/auth.rs`, the `record_promotion` store method, the `promotions` table, the fact-emission on successful promote, and any backward-compatible auth-context extension needed.
- Commit message includes `cargo build --workspace` + `cargo test --workspace` output, plus a transcript of both promote attempts (human succeeds, agent fails).
- Version bump within 4.3.x (public auth surface change warrants at least a patch; minor if context shape changed). Create a new local annotated tag.
- Ticket status flipped from `Ready` → `Complete` in the same commit as the code.
