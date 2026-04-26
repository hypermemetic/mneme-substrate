---
id: HF-CTX-7
title: "Recursive zoom with incremental rollups"
status: Pending
type: implementation
blocked_by: [HF-CTX-3, HF-CTX-4, HF-CTX-5, HF-CTX-6]
unlocks: [HF-CTX-8]
severity: High
target_repo: hyperforge
---

## Problem

HF-CTX-5 ships the base-case `zoom` for depths 0 and 1. Depths ≥ 2 require aggregating across meta-epics and cross-epic programs, which a naive scan would traverse O(facts × depth) per call. This ticket introduces per-epic rollup tables maintained incrementally on every ticket landing (or any fact append that changes an epic's aggregate) and re-implements `query_zoom` to read rollups rather than raw facts. The rollup layer makes `zoom(epic, depth = N)` O(child_epics × N) instead of O(facts).

## Context

Target repo: `hyperforge`.

Upstream inputs:

- **HF-CTX-3.** Ticket CRUD methods exist; `update_status` publishes `TicketEvent::status_changed` events, including `Ready → Complete` transitions (ticket-landing trigger).
- **HF-CTX-4.** Fact emission hooks append facts into the store; `FactSink::append` returns the new `FactId`.
- **HF-CTX-5.** `query_zoom(epic, depth)` is live for depths 0 and 1, returning `CtxError::Unsupported` for `depth > 1`.
- **HF-CTX-6.** Watch channels for ticket events and fact records are live; the rollup maintainer subscribes to both.

Zoom semantics per HF-CTX-1:

- Depth 0 — single ticket's fact bundle.
- Depth 1 — aggregate over all tickets in the ticket's immediate epic.
- Depth 2 — aggregate over the parent meta-epic (e.g., `HF` is the parent of `HF-CTX`, `HF-DC`, `HF-TT`, `HF-IR`).
- Depth N — recursive rollup.

Epic-parent relationship: an epic's prefix may be hierarchical. `HF-CTX` is a child of `HF`. Parent-derivation rule: strip the last hyphen-separated segment. `HF-CTX` → `HF`. `HF` has no parent (depth-2+ zoom on `HF` is workspace-wide, which per HF-CTX-1 is equivalent to a zoom with synthetic "workspace" scope).

Aggregation primitives (from HF-CTX-S02):

- `fact_counts: HashMap<String, u64>` — per-fact-kind count.
- `distinct_packages: Vec<PackageName>`.
- `distinct_ecosystems: Vec<Ecosystem>`.
- `time_span_start: i64`, `time_span_end: i64`.
- `children: Vec<ZoomedView>` — per child-epic view at one-less depth.

Rollup aggregation is **associative**: the epic-level rollup is the element-wise merge of child rollups (sum counts, union sets, min/max timestamps), so `zoom(depth = N)` composes `depth = N-1` rollups without re-scanning facts.

## Required behavior

### Rollup storage

Introduce `epic_rollups` table in the SQLite backend (and the equivalent map in `InMemoryTicketStore`):

| Column | Type | Meaning |
|---|---|---|
| `epic_prefix` | TEXT PRIMARY KEY | Epic prefix. |
| `fact_counts` | TEXT (JSON) | `HashMap<String, u64>`. |
| `distinct_packages` | TEXT (JSON) | `Vec<PackageName>`. |
| `distinct_ecosystems` | TEXT (JSON) | `Vec<Ecosystem>`. |
| `time_span_start` | INTEGER | Earliest `valid_at` in the epic's fact set. |
| `time_span_end` | INTEGER | Latest `valid_at`. |
| `ticket_count` | INTEGER | Number of tickets in the epic. |
| `updated_at` | INTEGER | Rollup's own last-update timestamp. |

Absent row means no facts have been recorded for the epic yet.

### Rollup maintainer

A background task started with `CtxActivation::start_rollup_maintainer()`:

1. Subscribes to the fact channel (HF-CTX-6's `watch_facts(FactFilter::default())`).
2. On each appended fact, look up the fact's `source_ticket`'s epic (via `get_ticket`). If the fact has no `source_ticket`, skip (ticket-less facts do not roll into an epic).
3. Update the epic's rollup row incrementally:
   - Increment `fact_counts[fact.kind]`.
   - If the fact's payload carries a `PackageName` field, add to `distinct_packages` (deduped).
   - Same for `Ecosystem`.
   - Update `time_span_start`/`time_span_end`.
   - Bump `updated_at`.

4. Subscribes to the ticket channel (`watch_all`). On `status_changed { to: Complete }` (ticket landing), force a recompute of the ticket's epic rollup from scratch (guard against missed fact events or reconciliation drift). This is the "rollup on ticket landing" step from HF-CTX-1.

5. Rollup maintenance is best-effort; a dropped event is followed by a background reconciler scan every `rollup_reconcile_interval` (default: 5 minutes) that walks `list_facts` for facts newer than each epic's `updated_at` and merges them into the rollup. Guarantees eventual consistency.

### `query_zoom` re-implementation

Replace HF-CTX-5's depth-0/1 default impl with a rollup-backed version:

- Depth 0 — identical to HF-CTX-5's depth-0 (single ticket facts; no rollups involved).
- Depth 1 — read the epic's rollup row; produce a `ZoomedView` whose `fact_counts`/`distinct_*`/`time_span_*` come from the rollup. `children: Vec<ZoomedView>` is a `depth=0` view per ticket in the epic.
- Depth N (N ≥ 2) — read the current epic's rollup + the rollups of every child epic (epics whose prefix is `<current>-*`); compose by element-wise merge. `children` is the list of child epics as `depth = N-1` views.

If a requested epic has no rollup row (no facts yet), return a `ZoomedView` with zero counts and empty children.

If a requested depth exceeds the epic tree's actual depth, the deepest meaningful level is returned; extra depth collapses to the workspace-wide rollup.

### Workspace-level rollup

A reserved synthetic epic prefix `"*"` (or `"<workspace>"` — pin at implementation time against HF-CTX-S02's decision) represents workspace-wide. The rollup maintainer keeps this row updated as a union of all other rollups. `query_zoom("*", depth)` returns the workspace rollup with child epics as its children.

### Error path cleanup

Remove the `CtxError::Unsupported` return from `query_zoom` for `depth > 1` — this ticket makes all depths supported.

## Risks

| Risk | Mitigation |
|---|---|
| Rollup drift between incremental update and truth. | Background reconciler every 5 minutes scans recent facts. Ticket-landing forces a full recompute of the affected epic. |
| Rollup maintainer crashes on startup, leaving stale rollups. | On activation start, run one reconcile pass over every epic before marking the maintainer healthy. |
| Child epic enumeration is slow on large workspaces. | Epic prefixes are stored in `epics` table; enumerating children is a single `WHERE prefix LIKE '<parent>-%'` query, indexed. |
| Recursive `zoom(depth = N)` on a pathologically deep epic tree. | Cap `depth` at 16 (pinned; raises `CtxError::DepthExceeded` beyond). Real epics go 3–4 deep. |
| Facts with a `source_ticket` whose epic isn't known (orphaned). | Maintainer logs a warning and skips; reconciler revisits if the ticket is later imported. |

## What must NOT change

- HF-CTX-3 / HF-CTX-4 / HF-CTX-5 / HF-CTX-6's method signatures and behavior. This ticket only adds the `epic_rollups` table, the maintainer task, and swaps `query_zoom`'s implementation.
- `ZoomedView` shape (from HF-CTX-S02). Rollup storage is a private implementation detail.
- `TicketStore` trait surface (no new methods). The rollup table is an internal schema detail of `SqliteTicketStore`.
- Every other hyperforge hub's behavior.
- Existing `plans/<EPIC>/*.md` files.

## Acceptance criteria

1. `cargo build --workspace` succeeds in hyperforge.
2. `cargo test --workspace` succeeds. New tests cover:

   | Scenario | Expected |
   |---|---|
   | Append 3 mixed facts with `source_ticket` in epic E. `query_zoom("E", 1)` | `fact_counts` shows 3 total across the three kinds; `time_span_*` brackets the three `valid_at`s. |
   | Epic hierarchy `HF-CTX` child of `HF`. Append 2 facts in `HF-CTX`, 1 in `HF-DC`. `query_zoom("HF", 2)` | `fact_counts` sums 3; `children` has 2 views (one for `HF-CTX`, one for `HF-DC`). |
   | `query_zoom("E", 0)` on a ticket in E | Identical to HF-CTX-5 depth-0 behavior. |
   | `query_zoom("E", 1)` immediately after appending a fact | Rollup reflects the fact within 100ms of the broadcast. |
   | `query_zoom("unknown-epic", 1)` | `ZoomedView` with zero counts; no error. |
   | Activation restart with pre-existing facts in the store | Post-restart `query_zoom` returns correct rollups (reconcile-on-start). |
   | `query_zoom("*", 3)` (workspace scope) | Returns union across every epic, children list populated. |
   | `query_zoom("E", 17)` | `CtxError::DepthExceeded { requested: 17, max: 16 }`. |
   | Missing a fact event (simulated broadcast drop) then reconciler runs | Rollup converges to correct state; measured before and after reconcile interval. |

3. Benchmark: `query_zoom` on an epic with 1,000 facts returns in <50ms p95 (rollups read, not raw facts).
4. `cargo build --workspace` + `cargo test --workspace` pass green end-to-end.

## Completion

- Commit adds the `epic_rollups` table, the rollup maintainer task, the reconciler, and the new `query_zoom` implementation; removes the `Unsupported` branch.
- Commit message includes the benchmark result and a transcript of `synapse hyperforge ctx zoom HF --depth 3` showing a populated view.
- If public-surface changes warrant a version bump within 4.3.x, bump (likely `4.3.x+1` for the depth-N support). Create a new local annotated tag.
- Ticket status flipped from `Ready` → `Complete` in the same commit as the code.
