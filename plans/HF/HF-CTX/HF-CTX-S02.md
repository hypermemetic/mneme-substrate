---
id: HF-CTX-S02
title: "Spike: knowledge-graph query surface + zoom algebra"
status: Pending
type: spike
blocked_by: [HF-CTX-S01]
unlocks: [HF-CTX-2, HF-CTX-5, HF-CTX-7]
severity: High
target_repo: hyperforge
---

## Question

What are the pinned Rust signatures of the seven knowledge-graph queries — `compat_broken`, `work_on`, `blast_radius`, `zoom`, `currently_deprecated`, `incompat_observers`, `who_touches` — and the aggregation primitives that compose `zoom(depth)`? Are queries exposed as Rust library API only, as Plexus RPC methods only, or as both?

Binary pass: a decision document that (a) pins each query's exact signature against the types from HF-CTX-S01, (b) pins the aggregation primitives used by `zoom` at each depth, (c) decides the library-vs-RPC exposure for each query, (d) prototypes each query end-to-end against a throwaway in-memory fact log populated from a fixture.

## Context

Upstream input: HF-CTX-S01 has pinned the `Fact` variants, `TicketScope` fields, the `HyperforgeEvent → Fact` mapping, and the types crate that owns these records.

Seed signatures (from HF-CTX-1's "Knowledge-graph query surface" table — to ratify / adjust):

| Query | Seed signature | Answers |
|---|---|---|
| `compat_broken` | `fn(Consumer, Dep) -> Option<TicketId>` | Last ticket completed before consumer/dep became incompatible. |
| `work_on` | `fn(ArtifactId) -> Vec<TicketId>` | All tickets that touched/introduced/deprecated/removed the artifact. |
| `blast_radius` | `fn(TicketId, depth: u32) -> Graph` | Transitive consumers/producers, bounded depth. |
| `zoom` | `fn(epic_prefix: String, depth: u32) -> ZoomedView` | Recursive aggregate. |
| `currently_deprecated` | `fn() -> Vec<ArtifactId>` | All artifacts with open `Deprecated` fact and no `Removed` fact. |
| `incompat_observers` | `fn(PackageName, Version) -> Vec<TicketId>` | Who flagged this version as incompatible. |
| `who_touches` | `fn(RepoPath) -> Vec<TicketId>` | Tickets that touched the file. |

`Consumer` and `Dep` in `compat_broken`'s seed signature resolve to `PackageName` (or a `(PackageName, Ecosystem)` tuple — pin in this spike). `Graph` in `blast_radius` is an adjacency structure that must be defined. `ZoomedView` in `zoom` is a struct whose shape this spike pins.

Zoom semantics from HF-CTX-1:

- Depth 0 = single ticket's fact bundle.
- Depth 1 = aggregate over all tickets in the ticket's immediate epic.
- Depth 2 = aggregate over the epic's parent meta-epic.
- Depth N = recursive rollup all the way to workspace-wide.

Aggregation primitives candidate set: fact counts per kind, distinct packages touched, distinct ecosystems touched, time span start/end, list of child epics with their summaries.

Library vs RPC exposure: the two dimensions are in tension:

- **Library-only.** Queries are `fn` on a `TicketStore` trait or handle. Fast in-process, no wire serialization. Consumers: Orcha, other activations in the same substrate. Not callable from synapse.
- **RPC-only.** Queries live as `#[plexus_macros::method]` on `hyperforge.ctx`. Synapse-callable, remote-callable. Serialization overhead.
- **Both.** Library API is the authoritative surface; RPC methods are thin wrappers. Consumers pick.

The seed expectation from HF-CTX-1 is **both** — library for other activations, RPC for humans/CLIs. This spike confirms or amends.

## Setup

1. For each of the seven queries, pin the exact Rust signature against HF-CTX-S01's types. Note each argument / return type explicitly. Where the seed signature is ambiguous (e.g., `Consumer`), resolve to a concrete type.

2. Pin `ZoomedView` struct. Fields candidate:

   | Field | Type | Meaning |
   |---|---|---|
   | `scope` | `String` | Epic prefix or workspace identifier. |
   | `depth` | `u32` | Requested depth. |
   | `fact_counts` | `HashMap<String, u64>` | Per-fact-kind count (key = fact discriminator name). |
   | `distinct_packages` | `Vec<PackageName>` | All packages touched in scope. |
   | `distinct_ecosystems` | `Vec<Ecosystem>` | All ecosystems in scope. |
   | `time_span_start` | `i64` | Earliest `valid_at` in scope. |
   | `time_span_end` | `i64` | Latest `valid_at` in scope. |
   | `children` | `Vec<ZoomedView>` | Recursive rollups (non-empty iff `depth > 0`). |

   Ratify, extend, or adjust.

3. Pin `Graph` (or `BlastRadiusGraph`) shape for `blast_radius`. Candidate: `{ nodes: Vec<TicketId>, edges: Vec<(TicketId, TicketId, EdgeKind)> }` where `EdgeKind` distinguishes `BlockedBy`, `Touches`, `Introduces`, etc.

4. **Prototype.** Populate a throwaway in-memory fact log with a hand-built fixture: 3 epics × 4 tickets each (12 tickets), a handful of each `Fact` variant. Implement each of the seven queries against the fixture. Confirm each returns the correct answer on at least two fixture scenarios per query.

5. Run the prototype from both a Rust test harness (library API) and from a synapse CLI invocation against a throwaway activation (RPC API). Compare:
   - Does the library shape read naturally in Rust consumer code?
   - Does the RPC shape surface tab-completably in synapse?
   - Is either direction lossy? (E.g., does the RPC serialization drop information the library retains?)

6. Pin the library-vs-RPC exposure per query. Default expectation: all seven exposed both ways. Deviate only with a documented reason.

## Pass condition

A decision document contains:

1. Pinned Rust signatures for all seven queries.
2. Pinned `ZoomedView` struct.
3. Pinned `Graph` (or equivalent) for `blast_radius`.
4. Aggregation primitives for `zoom(depth)` enumerated, with rollup rules for each depth step.
5. Library-vs-RPC exposure decision per query (with reasoning if any query is library-only or RPC-only).
6. Throwaway prototype demonstrated each query end-to-end against the fixture.

Binary: all six pinned + prototype runs → PASS. Any left open → FAIL.

## Fail → next

If the prototype reveals that `zoom`'s aggregation is slower than O(tickets_in_scope) per call, write HF-CTX-S02b exploring precomputed rollups (per-epic rollup tables maintained incrementally on ticket landing) before HF-CTX-7 is promoted. HF-CTX-7 already pins rollup-based zoom; this spike confirms the base algebra independently.

## Fail → fallback

If library-vs-RPC exposure can't be pinned cleanly, default to **both** for all seven queries. Rationale: consistent surface, consumers choose. Revisit if any query's RPC form proves lossy against the library form.

## Time budget

Four focused hours for signature pinning + fixture-prototype. If the spike exceeds this, stop and report.

## Out of scope

- Real fact-log storage (HF-CTX-2's job).
- Real query performance optimization (HF-CTX-5 / HF-CTX-7 own this).
- Pagination on query results — assume unbounded for the prototype.
- Auth checks on queries — reads are unauthenticated.

## Completion

Spike delivers:

1. The decision document with all six items pinned.
2. The query signatures become the authoritative input to HF-CTX-5.
3. The aggregation primitives become the authoritative input to HF-CTX-7.
4. Pass/fail result, time spent, one-paragraph summary.

Report lands in HF-CTX-1's Context section as a reference before HF-CTX-2 is promoted to Ready.
