---
id: HF-CTX-6
title: "Watch / stream methods (watch_ticket, watch_epic, watch_all, watch_facts)"
status: Pending
type: implementation
blocked_by: [HF-CTX-2]
unlocks: [HF-CTX-7]
severity: Medium
target_repo: hyperforge
---

## Problem

Consumers running `synapse hyperforge ctx watch` and downstream activations (e.g., Orcha) watching for newly-ready tickets or newly-appended facts need push-based update channels, not periodic polling. This ticket introduces streaming RPC methods on the `hyperforge.ctx` activation — `watch_ticket`, `watch_epic`, `watch_all`, `watch_facts` — and the broadcast channels that back them. HF-CTX-3's CRUD methods and HF-CTX-4's fact-emission hooks publish to these channels on every successful mutation / append.

## Context

Target repo: `hyperforge`. Target file: `src/ctx/watch.rs` (new). Touches the shared activation struct in `src/ctx/activation.rs` to wire the broadcast senders; that wiring is a constructor-level additive change disjoint from HF-CTX-3's CRUD method implementations.

Plexus RPC streaming convention: returning `impl Stream<Item = T>` from a `#[plexus_macros::method]` produces a server-streaming RPC. See existing streaming usage in `src/hubs/` (any hub that tails logs or progress events) for the pattern.

Event broadcast: three broadcast senders live inside the `CtxActivation` struct:

- `tokio::sync::broadcast::Sender<TicketEvent>` for ticket mutations (shared across `watch_ticket`, `watch_epic`, `watch_all`).
- `tokio::sync::broadcast::Sender<FactRecord>` for fact appends (shared across `watch_facts`).

HF-CTX-3's CRUD methods publish to the ticket channel after every successful write. HF-CTX-4's fact-emission hooks publish to the fact channel after every successful `append_fact`. (Facts already flow through the sink; the watch channel is a second, non-durable tap.)

Slow subscribers that lag behind the broadcast buffer get a `Lagged` marker event.

### Parallelism with sibling tickets

- HF-CTX-3 (CRUD) owns `src/ctx/activation.rs`'s method implementations. HF-CTX-6 adds a `publish(&self, event: TicketEvent)` helper and three `watch_*` methods — they live in a sibling file (`src/ctx/watch.rs`) pulled in via `mod`. The only shared edit is the activation's constructor (add two `broadcast::Sender` fields) and the CRUD methods' tail (call `self.publish(...)`) .
- Coordination: HF-CTX-3 adds `// TODO(HF-CTX-6): publish event` markers at each mutation tail. HF-CTX-6 replaces the markers with `self.publish(...)` calls.
- If HF-CTX-3 lands first with the markers in place, HF-CTX-6 replaces them. If HF-CTX-6 lands first, HF-CTX-3 writes the `publish` call directly (the helper is already available).

File-boundary status at commit time decides which path applies.

## Required behavior

### `TicketEvent` type

Introduce in the types crate alongside `Ticket`:

Tagged enum (`#[serde(tag = "event", rename_all = "snake_case")]`):

| Variant | Fields | Emitted when |
|---|---|---|
| `created` | `ticket: Ticket` | `create_ticket` succeeded. |
| `body_updated` | `id: TicketId, updated_at: i64` | `update_body` succeeded. |
| `status_changed` | `id: TicketId, from: Status, to: Status, updated_at: i64` | `update_status` succeeded (including through HF-CTX-8's promote). |
| `scope_updated` | `id: TicketId, scope: TicketScope, updated_at: i64` | `update_ticket_scope` succeeded. |
| `deleted` | `id: TicketId` | `delete_ticket` succeeded. |
| `epic_created` | `epic: Epic` | `create_epic` succeeded. |
| `lagged` | `missed: u64` | Subscriber fell behind the broadcast buffer. |

Derives: `Debug, Clone, Serialize, Deserialize, JsonSchema`.

### Streaming RPC methods

| Method | Args | Return | Behavior |
|---|---|---|---|
| `watch_ticket` | `id: TicketId` | `impl Stream<Item = TicketEvent>` | Stream of events concerning exactly one ticket. Filters to events whose `id` field matches. `created` and `deleted` where `ticket.id` / `id` match are included. Stream terminates on client disconnect. |
| `watch_epic` | `prefix: String` | `impl Stream<Item = TicketEvent>` | Stream of events concerning any ticket whose epic prefix matches. Also emits `epic_created` when the epic itself is created. |
| `watch_all` | `(none)` | `impl Stream<Item = TicketEvent>` | Unfiltered stream of every `TicketEvent`. |
| `watch_facts` | `filter: FactFilter` | `impl Stream<Item = FactRecord>` | Stream of facts as they are appended. Filters per `FactFilter` (same struct as `list_facts`). |

All four streams emit a synthetic snapshot at subscription start:

- `watch_ticket(id)` — if the ticket exists, one synthetic `created { ticket }` event for the current state, then tails.
- `watch_epic(prefix)` — if the epic exists, one `epic_created { epic }` + one `created { ticket }` per current ticket in the epic, then tails.
- `watch_all()` — one `created { ticket }` for every current ticket, then tails.
- `watch_facts(filter)` — **no snapshot**. Fact history is unbounded; callers use `list_facts` for historical queries. `watch_facts` is live-only from subscription forward.

After the synthetic prefix (where applicable), each switches to tailing the broadcast channel.

### Broadcast channels

- Ticket events: `tokio::sync::broadcast::channel(1024)` — buffer size 1024.
- Fact events: `tokio::sync::broadcast::channel(4096)` — buffer size 4096 (facts are higher-volume than ticket mutations).

A subscriber that falls more than the buffer size behind receives a `TicketEvent::Lagged { missed }` (for ticket streams) or — for fact streams — a synthetic `FactRecord` with a reserved sentinel kind (pinned in HF-CTX-S01 if S01 pinned a `Lag` fact variant; otherwise drop-on-lag with a warning log, and the client is expected to reconcile via `list_facts`).

The stream is not terminated on lag.

### Publish helper

A `publish_ticket(&self, event: TicketEvent)` method on the activation, lossy (`let _ = sender.send(event);`). Called at the tail of every successful mutation in HF-CTX-3's methods.

A `publish_fact(&self, record: FactRecord)` method on the activation, same semantics. Called from the `FactSink` implementation bound to this activation's store — the activation's sink wrapper both appends to the store and publishes to the fact channel.

### Constructor change

`CtxActivation::new(...)` adds two fields to the struct:

```text
pub struct CtxActivation {
    store: Arc<dyn TicketStore>,
    tickets: broadcast::Sender<TicketEvent>,
    facts: broadcast::Sender<FactRecord>,
}
```

Construction builds both channels with the pinned buffer sizes. No backward-compatible constructor needed because `CtxActivation` is new surface (HF-CTX-3 introduced it).

## Risks

| Risk | Mitigation |
|---|---|
| Broadcast channel buffer fills under high write rate. | Lag surfaced as `TicketEvent::Lagged` / sentinel `FactRecord` rather than silent drop. Subscriber re-syncs via `list_facts` / `list_tickets`. |
| Synthetic snapshot on `watch_all()` is huge for large DBs. | Snapshot on subscribe is acceptable for hyperforge scale (<10k tickets). Add `snapshot: bool` arg in a follow-up if contention shows up. |
| Event order across concurrent writers. | Broadcast guarantees order per producer; consumers treat events as idempotent. |
| Coordination with HF-CTX-3's publish call sites. | TODO-marker convention pinned in Context. If HF-CTX-6 lands second, grep sanity-check before merge. |
| `watch_facts` subscribers miss the initial fact burst from HF-CTX-10's importer. | By design: `watch_facts` is live-only. Post-import, subscribers re-sync via `list_facts` once. |

## What must NOT change

- HF-CTX-2's `TicketStore` trait surface (streaming lives in the activation, not the store).
- HF-CTX-3's CRUD behavior and result shapes (the `publish` tail is a side-effect; failure paths do not publish).
- HF-CTX-4's `FactSink` trait (the activation's sink wrapper is a new type; it implements `FactSink` by delegating to the store and then publishing).
- Every other hyperforge hub's behavior.

## Acceptance criteria

1. `cargo build --workspace` succeeds in hyperforge.
2. `cargo test --workspace` succeeds. New tests cover:

   | Scenario | Expected |
   |---|---|
   | Subscribe to `watch_ticket("X")`, create ticket X | One `created` event within 500ms. |
   | Subscribe to `watch_ticket("X")` for an existing X | Synthetic `created` snapshot first, then live events. |
   | Subscribe to `watch_epic("HF-CTX")`, create two HF-CTX tickets | Two `created` events; unrelated-epic writes do not appear. |
   | Subscribe to `watch_all()`, perform one each of create / update-body / update-status / delete | Four events in submission order. |
   | Update a ticket's status → `watch_ticket` gets `status_changed { from, to }` | Exactly one event. |
   | Delete a ticket → `watch_ticket` gets `deleted` | Stream remains open after the delete event. |
   | Subscribe to `watch_facts(filter { kind: Some("version_bumped") })`, append 3 mixed facts | Only the `VersionBumped` record appears on the stream. |
   | Subscribe to `watch_facts(filter { ticket: Some(T) })` | Only facts with `source_ticket == Some(T)`. |
   | A ticket subscriber ignores >1024 events | Receives a `lagged { missed }` marker; subsequent events continue. |

3. `synapse hyperforge ctx watch_all` in one shell surfaces live events from writes issued in another shell.
4. `cargo build --workspace` + `cargo test --workspace` pass green end-to-end.

## Completion

- Commit adds `src/ctx/watch.rs`, the `TicketEvent` enum, the publish helpers, the broadcast-channel fields in `CtxActivation`, and the four streaming RPC methods.
- Commit message includes `cargo build --workspace` + `cargo test --workspace` output plus a transcript of `synapse hyperforge ctx watch_all` observing a live mutation.
- If public-surface changes warrant a version bump within 4.3.x, bump; else contribute to the existing patch line.
- Ticket status flipped from `Ready` → `Complete` in the same commit as the code.
