---
id: OB-4
title: "Pagination on unbounded list RPC methods"
status: Pending
type: implementation
blocked_by: []
unlocks: []
severity: High
target_repo: plexus-substrate
---

## Problem

Substrate has several RPC methods that return unbounded lists — `orcha.list_graphs`, `pm.list_ticket_maps`, and other `list_*` methods across activations. Today these return the full set on every call. As tables grow, the response size grows linearly; there is no mechanism to request a page, a bounded count, or a count-only answer. An operator with 10,000 graphs cannot ask "give me the first 50 most recent" — they get all 10,000 or nothing.

This ticket adds **offset/limit pagination** to every unbounded list method, plus a total count in the response. Old callers that send no pagination parameters receive the **first page** (with a sensible default limit) rather than the whole set — this is a deliberate behavior change, chosen because the current behavior is itself the bug. A default limit of 100 is a conservative choice that preserves small-list behavior while capping runaway responses. Callers that need "everything" explicitly page through.

## Context

**Pagination shape (pinned for this ticket):**

Offset/limit, not cursor. Rationale:
- Simpler to implement against SQLite `LIMIT ?  OFFSET ?`.
- Total count is cheap in our scale range (low tens of thousands maximum).
- Cursor-based pagination is better for high-cardinality infinite feeds; our list methods are bounded administrative queries.
- Cursor can be added later as an opt-in if a specific method needs it; pagination shape is per-method, not global.

**Request shape (pinned):**

```json
{
  "limit": 100,     // optional; default 100; max 1000
  "offset": 0       // optional; default 0
}
```

**Response shape (pinned):**

```json
{
  "items": [...],   // the requested page
  "total": 1234,    // total matching items (across all pages)
  "limit": 100,
  "offset": 0
}
```

The existing response shape (a bare array) wraps into `items`. Callers deserializing the old shape break — this is acknowledged as a **breaking change** for list methods. Mitigation:

- Bump the activation's version (per the pinned deprecation policy in `plans/README.md`).
- Document the new shape in CHANGELOG.
- Old clients fail at deserialize time with a clear error (unknown field or wrong type at root); the error surface does not silently corrupt data.
- The breaking change is scoped to list methods; no other methods change shape.

**Methods to paginate (initial sweep; re-verify against HEAD at implementation):**

| Activation | Method | Current return shape |
|---|---|---|
| Orcha | `list_graphs` | `Vec<GraphSummary>` |
| PM (Orcha's ticket manager) | `list_ticket_maps` | `Vec<TicketMap>` |
| PM | `list_tickets` (if present) | `Vec<TicketSummary>` |
| Lattice | `list_graphs`, `list_nodes` (if present) | various `Vec<_>` |
| Registry | `list_nodes` / `list_activations` | various |
| ClaudeCode | `list_sessions` (if present) | `Vec<SessionSummary>` |
| Loopback | `list_approvals` (if present) | `Vec<Approval>` |

The implementation's first step is to `rg 'fn list_|pub fn list_' src/activations/` (and Plexus RPC-method attributes) to enumerate every list method. Every enumerated method is either paginated, or explicitly excluded with documented reasoning (e.g., "`list_*_ids` returns a strictly-bounded set of less than 10 items by construction"). No silent skips.

**Default-limit rationale (pinned at 100).** A caller sending `{}` to `orcha.list_graphs` previously received all graphs. They now receive the first 100 by offset 0. For any operator with fewer than 100 graphs, behavior is indistinguishable from before. For operators with more, they now learn about pagination from the response's `total` field.

**Max-limit rationale (pinned at 1000).** A caller sending `{"limit": 99999}` gets clamped to 1000 — no error, but a `tracing::warn!` is logged. The clamp exists to prevent memory blow-up on misuse.

## Required behavior

### Per-method contract (applies to every paginated method)

| Caller input | Behavior |
|---|---|
| Omits pagination params | Server applies `limit=100, offset=0`. Response includes `limit: 100, offset: 0, total: N`. |
| Provides `limit=50, offset=0` | Server returns items 0-49. `total` is the full count. |
| Provides `limit=50, offset=100` | Server returns items 100-149. `total` is the full count. |
| Provides `offset` past total | Server returns `items: []`, `total: N`, `offset: <requested>`. No error. |
| Provides `limit > 1000` | Server clamps to `limit=1000`, logs `warn!`, returns 1000 items. Response `limit` field shows the clamped value. |
| Provides `limit=0` | Server returns `items: [], total: N, limit: 0, offset: <requested>`. (Useful for count-only queries.) |
| Provides negative values | Server errors with "invalid pagination parameter". |

### Count semantics

`total` reflects the count of items that match any **filter** the method otherwise accepts. A `list_graphs { status: "running" }` returns `total` = count of running graphs, not count of all graphs. For methods with no filters, `total` = table row count.

### Ordering

Every paginated method commits to a **deterministic order**. The order may be chosen per-method (e.g., "most recent first" for graphs, alphabetical for ticket maps) but once chosen is documented in the method's schema / doc comment. Pagination without deterministic order is meaningless — consecutive pages could repeat or skip items.

### Count cost

For SQLite-backed lists, `total` is computed via a second `SELECT COUNT(*)` query with the same WHERE clause. This doubles DB work for paginated calls; at our scale that cost is invisible. If profiling later shows the count query is a bottleneck, a `count_exact: false` request option can be added to return an approximate or capped count — follow-up ticket, not this one.

### Schema update

Every paginated method's `plugin_schema()` / IR-emitted method description declares:
- New param types (`limit: Option<u32>`, `offset: Option<u32>`).
- New return type wrapping the previous shape (`PaginatedList<T>` or per-method equivalent).
- Documented ordering guarantee in the method's description (pulled from `///` doc comments per the IR epic's doc-comment extraction).

## Risks

| Risk | Mitigation |
|---|---|
| Breaking change surprises consumers. | Accepted; consumers are primarily substrate operators and synapse (which is maintained alongside substrate). Activation version bump + CHANGELOG entry per the deprecation policy. Synapse's rendering layer updates to consume the new shape in a coordinated ticket (file follow-up if needed). |
| Ordering is unstable for methods without an obvious ORDER BY. | Every list method explicitly documents its ORDER BY. If no natural order exists, use the insertion timestamp or the primary key. No method ships without a pinned order. |
| Some list methods live outside SQLite (e.g., in-memory Vec) and require a different pagination impl. | In-memory pagination is cheap (slice the Vec). Each method's impl does what's idiomatic for its backing store; the wire contract stays uniform. |
| Pagination on streaming methods is out of scope (streams paginate by nature — caller cancels when done). | Streaming methods explicitly documented as "pagination does not apply; use stream cancellation". The sweep distinguishes streaming from batch list methods. |
| `list_*` that already take filter params have their filter semantics confused by pagination limits. | Filters apply **first**, then pagination. `total` reflects post-filter count. Tests cover this explicitly. |
| PM activation's list methods may be removed entirely once TM epic ships. | Paginate them anyway — TM inherits the pattern. Sunsetting an activation is orthogonal to making it correct while alive. |

## What must NOT change

- Non-list RPC methods. Their request/response shapes are unchanged.
- Activation namespace strings or method names. Pagination rides on existing names; no method is renamed.
- Error responses for non-pagination-parameter errors. Pagination validation errors are new; existing validation behavior for other fields is unchanged.
- Default behavior of list methods when the caller has fewer items than the default limit (100). Such callers see no observable change.
- Schema hashes — these change under IR's normal deprecation flow when a method's signature changes, which is the intended effect of this ticket.

## Acceptance criteria

1. A call to every list method identified in the sweep (committed in the PR as a checklist) accepts `limit` and `offset` params and returns the pinned response shape (`items`, `total`, `limit`, `offset`).
2. Calling `orcha.list_graphs {}` returns at most 100 items regardless of how many graphs exist. Verifiable with a test that inserts 150 graphs and observes `items.len() == 100` and `total == 150`.
3. Calling `orcha.list_graphs { "offset": 100 }` returns items 100-149 from the ordering defined in the schema. Verifiable by asserting the first and last items of the page.
4. Calling `orcha.list_graphs { "limit": 0 }` returns `items: []` and `total: <full count>`. Verifiable with a test.
5. Calling `orcha.list_graphs { "limit": 10000 }` returns at most 1000 items, response `limit` shows the clamped value, and substrate stderr logs a warning.
6. Negative `offset` or `limit` returns a structured error response; substrate does not crash.
7. `cargo test --workspace` passes after the breaking shape change is accounted for (existing tests updated to consume the new shape).
8. `rg 'fn list_' src/activations/` output is reconciled against the paginated-methods checklist — every `fn list_*` is either paginated or explicitly excluded with documented reasoning in a comment above its definition.
9. Each paginated method's method description (via `///` doc comment, surfaced per IR's doc-comment extraction) names the ordering guarantee (e.g., `/// Returns graphs sorted by created_at descending.`).
10. CHANGELOG entry added for the substrate release describing the breaking shape change and the default-limit behavior.
11. Synapse's rendering of list results handles the new shape (wrapped in `items`); if synapse changes are required, they are landed alongside or in a coordinated follow-up ticket referenced in this PR.

## Completion

PR against `plexus-substrate`. CI green. Status flipped from `Ready` to `Complete` in the same commit. When OB-4 lands, substrate's administrative query surface stops being O(table size) on every call.
