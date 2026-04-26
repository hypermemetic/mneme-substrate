---
id: TM-4
title: "TM query methods (list, ready, blocked_on, unlocks_chain, epic_dag, epic_progress)"
status: Pending
type: implementation
blocked_by: [TM-2, TM-S02]
unlocks: [TM-7]
severity: High
target_repo: plexus-substrate
---

## Problem

Consumers need to discover which tickets are ready to work on, trace dependency chains, and inspect epic-level progress — without running N round-trip `get_ticket` calls. TM-3 covers single-ticket mutation; this ticket adds the read-side multi-ticket queries. The query surface shape (typed methods vs filter DSL vs hybrid) was decided in TM-S02.

## Context

Target repo: `plexus-substrate`. New file: `src/activations/tm/queries.rs` (or a `methods_queries.rs` sibling if the convention in the codebase prefers that — match the existing style under `src/activations/cone/methods.rs`).

TM-S02's decision is pinned before this ticket is promoted. This ticket assumes the **typed-methods** shape (the fallback and most likely pass condition). If TM-S02 instead chose DSL or hybrid, this ticket is re-scoped before promotion. The required behavior below describes the typed-methods shape.

Upstream types: `Ticket`, `Epic`, `Status`, `TicketId` (or `String`) — all from TM-2.

Pagination convention: the paginated methods return `(Vec<Ticket>, Option<String>)` where the `Option<String>` is an opaque cursor usable in a subsequent call's `cursor` argument. The cursor format is an implementation detail; consumers treat it as opaque.

## Required behavior

### RPC methods

| Method | Args | Return | Behavior |
|---|---|---|---|
| `list_tickets` | `cursor: Option<String>, limit: Option<usize>` | `TmListResult` | Paginated listing across all epics. `limit` defaults to 50, max 500. Returns `ok { tickets, next_cursor }`. |
| `ready` | `(none)` | `TmTicketsResult` | Returns all tickets with `status: Ready`, sorted by severity (Critical first, then High, Medium, Low, None), then by id. |
| `blocked_on` | `id: TicketId` | `TmTicketsResult` | Returns the full `blocked_by` chain for the given ticket — transitive upstream dependencies, in breadth-first order starting from `id`'s direct blockers. Excludes `id` itself. |
| `unlocks_chain` | `id: TicketId` | `TmTicketsResult` | Returns the transitive downstream — every ticket that (directly or transitively) has `id` in its `blocked_by` chain. Excludes `id` itself. Ordered breadth-first from `id`'s direct unlockers. |
| `epic_dag` | `prefix: String` | `TmEpicDagResult` | Returns an adjacency list for every ticket in the epic: `Vec<(TicketId, Vec<TicketId>)>` where the inner list is the ticket's direct `blocked_by`. Also returns a `mermaid` field — a valid Mermaid DAG diagram string usable in markdown. |
| `epic_progress` | `prefix: String` | `TmEpicProgressResult` | Returns counts by status for the epic: `total`, `epic_records`, `pending`, `ready`, `blocked`, `complete`, `idea`, `superseded`. |

### Result types

Tagged enums, matching TM-3's style:

- `TmListResult` — `ok { tickets: Vec<Ticket>, next_cursor: Option<String> }`, `err { message }`.
- `TmTicketsResult` — `ok { tickets: Vec<Ticket> }`, `not_found { id }` (for `blocked_on`/`unlocks_chain`), `err { message }`.
- `TmEpicDagResult` — `ok { adjacency: Vec<(TicketId, Vec<TicketId>)>, mermaid: String }`, `not_found { prefix }`, `err { message }`.
- `TmEpicProgressResult` — `ok { prefix, total, pending, ready, blocked, complete, idea, superseded, epic_records }`, `not_found { prefix }`, `err { message }`.

### Mermaid output shape

For an epic with three tickets A, B, C where B is `blocked_by: [A]` and C is `blocked_by: [B]`, `epic_dag("EPIC")` returns `mermaid`:

```
graph TD
    A
    B --> A
    C --> B
```

The arrow direction encodes "points to upstream dependency". Epic-level records (`status: Epic`) are included as nodes but never have blockers. Any `Superseded` ticket is rendered with a distinct label prefix (e.g., `[superseded]`).

### Cycle tolerance

`blocked_on` and `unlocks_chain` traverse the graph with cycle detection. A malformed DAG with cycles returns the partial traversal (each ticket visited exactly once) and does not error — cycles are a data-quality issue surfaced elsewhere, not a query failure.

## Risks

| Risk | Mitigation |
|---|---|
| TM-S02 picked DSL or hybrid instead of typed. | Re-scope this ticket before promotion. The method set above is the typed-shape fallback. |
| Large epics cause `epic_dag` to return multi-MB mermaid strings. | Acceptable for current scale (<100 tickets/epic). Revisit when an epic exceeds 200 tickets. Out of scope for this ticket. |
| Sorting `ready` by severity when severity is optional. | Missing severity sorts last (after `Low`). Pinned in the method behavior. |
| Cycles in `blocked_on` loop forever. | Cycle detection via visited-set, as described. |

## What must NOT change

- TM-2's `TicketStore` trait surface. This ticket consumes it, does not extend it (if a new store method is genuinely needed, it is added to TM-2 first via an amendment ticket).
- TM-3's mutation methods and their results.
- Every other substrate activation's behavior.

## Acceptance criteria

1. `cargo build -p plexus-substrate` succeeds.
2. `cargo test -p plexus-substrate` succeeds. Tests cover:

   | Scenario | Expected |
   |---|---|
   | `ready()` with 3 Ready tickets of severities Critical / Medium / None | Returns all three in Critical → Medium → None order. |
   | `ready()` with no Ready tickets | Returns `ok { tickets: [] }`. |
   | `blocked_on("B")` where A blocks B blocks C | Returns `[A]`. |
   | `blocked_on("C")` where A blocks B blocks C | Returns `[B, A]` (breadth-first). |
   | `unlocks_chain("A")` where A blocks B blocks C | Returns `[B, C]`. |
   | `epic_dag("EPIC")` | Adjacency matches input; mermaid string contains each ticket id exactly once. |
   | `epic_progress("EPIC")` with 1 Ready, 2 Pending, 1 Complete, 1 Epic record | Returns `total: 5, pending: 2, ready: 1, complete: 1, epic_records: 1, blocked: 0, idea: 0, superseded: 0`. |
   | `list_tickets(limit: 10)` with 25 tickets | Returns first 10 + a non-empty `next_cursor`; follow-up call returns next 10; third call returns remaining 5 + `next_cursor: None`. |
   | `blocked_on` on a ticket in a cycle | Returns each ticket in the cycle exactly once, does not loop. |

3. `synapse tm ready`, `synapse tm epic_dag TM`, and `synapse tm epic_progress TM` all produce human-readable output against a populated DB.

## Completion

- PR adds the new methods to `src/activations/tm/activation.rs` (or a queries sibling file).
- PR description includes `cargo build -p plexus-substrate` + `cargo test -p plexus-substrate` output — both green — plus a transcript of the three synapse calls above.
- Ticket status flipped from `Ready` → `Complete` in the same commit as the code.
