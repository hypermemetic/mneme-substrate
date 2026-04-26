---
id: TM-S01
title: "Spike: absorb vs coexist with orcha/pm"
status: Pending
type: spike
blocked_by: []
unlocks: [TM-2, TM-7]
severity: High
target_repo: plexus-substrate
---

## Question

Does TM absorb the responsibilities of `orcha/pm` (graph-to-ticket mapping and node event logs), or do they coexist as separate activations with a documented seam?

Binary pass: either `orcha/pm`'s role can be entirely subsumed by TM (one of the two is deleted without regressions in a prototype), or a clear seam can be documented between them where each owns a distinct, non-overlapping concern.

## Context

`orcha/pm` is an existing activation at `src/activations/orcha/pm/`. Its storage (`src/activations/orcha/pm/storage.rs`) contains two tables:

| Table | Role |
|---|---|
| `orcha_ticket_maps` | Maps `(graph_id, ticket_id) → node_id`. Populated when Orcha compiles a ticket into a graph. |
| `orcha_ticket_sources` | Stores the original `plans/<EPIC>/*.md` text that was compiled for a given `graph_id`. |
| `orcha_node_logs` | Event log for every node in every graph — task lifecycle, commands, outputs, errors. |

Its activation (`src/activations/orcha/pm/activation.rs`, ~700 lines) exposes RPC methods that answer "what happened to ticket X?" / "what nodes are blocked?" / "inspect this ticket's outputs". These queries operate over the intersection of ticket identity and graph runtime state.

TM, as scoped in TM-1, owns ticket authorship and lifecycle: create/edit tickets, track status, promote `Pending → Ready`, produce the DAG, answer "what is ready to work on?". TM does not model graph runtime — that is Orcha's concern.

So there are two plausible shapes:

**Shape A (absorb):** TM owns tickets, and the `graph_id → ticket_id → node_id` mapping plus node event logs move into TM as a "runtime view" of tickets. `orcha/pm` is deleted. Orcha writes to TM via its library API when it compiles a graph and when nodes transition.

**Shape B (coexist):** TM owns the ticket lifecycle only (authoring, status, DAG). `orcha/pm` continues to own runtime-side mappings (graph_id ↔ ticket_id ↔ node_id) and node event logs. The seam: TM holds the ticket's lifecycle state; `pm` holds the join from a specific graph run to the specific nodes that executed that ticket.

The wrong shape creates either overlap (two systems both writing ticket status) or gap (neither owns the ticket → graph mapping cleanly).

## Setup

1. Enumerate every RPC method on `orcha/pm`'s activation and every direct call-site that reaches into `PmStorage` from other substrate modules. Produce a list of concrete responsibilities.
2. For each responsibility, annotate it with `"ticket authorship"` (belongs in TM under Shape A) or `"graph runtime"` (stays in `pm` under Shape B) or `"ambiguous"` (could live in either).
3. If the `"ambiguous"` count is zero, the seam is self-evident → Shape B is viable. Record the seam.
4. If the `"ambiguous"` count is non-zero, attempt Shape A: sketch a TM schema that holds all `pm` tables plus TM's own, and sketch how Orcha would write to TM via library API on graph start / node transitions. Prototype the minimum code path that writes a `(graph_id, ticket_id, node_id)` record through a TM-shaped API and reads it back.
5. Run substrate's existing tests against the prototype. If Shape A holds (no regressions, no awkward join queries), mark absorb as viable.

## Pass condition

Exactly one of the following is true and documented:

- **Absorb passes.** A prototype replaces `orcha/pm`'s storage and activation calls with TM-shaped equivalents, `cargo test -p plexus-substrate` is green, and the write paths Orcha uses today transparently redirect through TM. The deletion of `orcha/pm` is left as a follow-up ticket (not part of this spike), but the prototype demonstrates it is mechanically tractable.
- **Coexist passes.** A seam document lists, for every `pm` responsibility, whether it stays in `pm` or moves to TM, with no `"ambiguous"` entries left over. The seam is pinned as a two-column table in TM-1's Context section before TM-2 is promoted.

Binary: one of absorb/coexist → PASS. Neither → FAIL.

## Fail → next

Neither shape works cleanly on the first pass. The next step is not another spike — it is a replanning trigger on TM-2 and TM-7, because the `TicketStore` trait shape changes depending on whether it has to cover `pm`'s schema. Report the specific responsibilities that resisted clean assignment; they become inputs to a redesign conversation.

## Fail → fallback

If neither absorb nor coexist is clean after the time budget, default to Shape B (coexist) with `pm` unchanged and TM narrowly scoped to authoring / lifecycle only. Document every ambiguous responsibility as an open question attached to a follow-up epic. This keeps TM shipping; the integration improves incrementally later.

## Time budget

Four focused hours for the enumeration + prototype. If the spike exceeds this, stop and report regardless of pass/fail state.

## Out of scope

- Actual deletion of `orcha/pm` (follow-up ticket if absorb passes).
- Migrating `orcha_node_logs` row-by-row into TM (follow-up ticket).
- Renaming anything in `orcha/pm` if coexist passes. The seam document is the output, not a refactor.

## Completion

Spike delivers:

1. A written decision: absorb or coexist, with the responsibility-annotation table from step 2.
2. If absorb: a throwaway branch with the prototype and green `cargo test -p plexus-substrate`, plus a one-paragraph description of what the prototype touched.
3. If coexist: the seam document, pasted into TM-1's Context section before TM-2 is promoted.
4. Pass/fail result, time spent, one-paragraph summary.

Report lands in TM-1's Context section as a reference before TM-2 is promoted to Ready.
