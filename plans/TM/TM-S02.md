---
id: TM-S02
title: "Spike: typed query methods vs filter DSL"
status: Pending
type: spike
blocked_by: []
unlocks: [TM-4]
severity: High
target_repo: plexus-substrate
---

## Question

Does TM's query surface expose a finite set of typed query methods (`tickets.ready()`, `tickets.blocked_on(id)`, `tickets.epic_progress(epic)`, …), or a single filter-DSL entry point (`tickets.query("status = Ready AND severity = High")`)?

Binary pass: pick one of the two shapes, prototype three realistic queries against it from the synapse CLI, and confirm both the ergonomics and the Plexus RPC introspection output are acceptable.

## Context

Plexus RPC is schema-driven and synapse renders each method as a tab-completable tree node with typed parameters. This affects the query surface decision:

**Typed methods.** Each query is its own `#[plexus_macros::method]` with a statically-typed signature. Synapse renders them as discoverable, self-documenting CLI subcommands. Downside: every new query shape is a new method + new wire change.

**Filter DSL.** A single `query` method accepts a string expression and returns a list of `Ticket`s. Infinitely extensible without wire changes. Downside: parsing a DSL inside an activation, schema introspection loses information (every query is just "a string"), error messages are runtime not compile-time, and synapse cannot tab-complete individual query shapes.

Hybrid is possible (typed for common queries + one escape-hatch `query(...)`). Hybrid is a valid output of this spike if the three-query prototype shows it's necessary.

## Setup

1. Choose three realistic queries that TM consumers will run:
   - "list all Ready tickets across all epics, sorted by severity" (human workflow)
   - "return the full `blocked_by` chain for TM-7" (Orcha integration)
   - "return epic progress for TM: total tickets, Complete count, Ready count, Pending count" (dashboard)
2. **Prototype A — typed.** Implement the three queries as separate `#[method]` calls on a throwaway test activation. Exercise each from the synapse CLI. Note: signature clarity, tab-completion behavior, and how much code each query takes.
3. **Prototype B — DSL.** Implement a minimal filter parser on the same test activation, exposing one `query(expression: String, limit: Option<usize>)` method. Write the three queries as DSL expressions. Exercise each from the synapse CLI. Note: expression syntax, parser error paths, and how discoverability fares.
4. Compare the two prototypes on: (a) can a first-time user run each query without reading implementation code? (b) does synapse's tree render surface each query? (c) is the server-side code per-query less than ~30 lines?

## Pass condition

A decision is recorded that names one shape (typed / DSL / hybrid) and justifies it in one paragraph against the four comparison criteria above. The chosen shape has a prototype that ran all three queries end-to-end.

Binary: a chosen shape + working three-query prototype → PASS. No decision or a prototype that couldn't execute all three → FAIL.

## Fail → next

If the chosen shape (typed) turns out to require more than ~10 query methods to cover realistic needs — which falls outside the file-boundary parallelism assumption for TM-4 — write TM-S02b exploring the hybrid shape before TM-4 is promoted.

## Fail → fallback

Default to **typed methods** with a fixed set covering the six queries listed in TM-4's scope (`list`, `ready`, `blocked_on`, `unlocks_chain`, `epic_dag`, `epic_progress`). The fallback trades extensibility for discoverability — consistent with substrate's existing schema-driven surface.

## Time budget

Three focused hours for both prototypes. If the spike exceeds this, stop and report; partial results still inform the decision.

## Out of scope

- Full query optimization. Prototypes can be naive full-table scans.
- Pagination design. That's a TM-4 concern.
- Auth checks on queries. Reads are unauthenticated in the prototype.

## Completion

Spike delivers:

1. A written decision naming the chosen shape (typed / DSL / hybrid), with the comparison criteria filled in.
2. A throwaway branch containing the prototype that ran all three queries end-to-end.
3. Pass/fail result, time spent, one-paragraph summary.

Report lands in TM-4's Context section as a reference before TM-4 is promoted to Ready.
