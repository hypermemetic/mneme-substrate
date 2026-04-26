---
id: TM-5
title: "TM watch / stream methods (watch_ticket, watch_epic, watch_all)"
status: Pending
type: implementation
blocked_by: [TM-2]
unlocks: [TM-7]
severity: Medium
target_repo: plexus-substrate
---

## Problem

Humans running `synapse tm watch` and Orcha polling for newly-ready tickets both need a push-based update channel, not periodic polling. This ticket introduces streaming RPC methods on the `tm` activation that emit a `TicketEvent` for every mutation and surface filtered views (per-ticket, per-epic, all).

## Context

Target repo: `plexus-substrate`. Target file: `src/activations/tm/activation.rs` (new streaming methods alongside the existing CRUD methods from TM-3).

Plexus RPC streaming convention: returning `impl Stream<Item = T>` from a `#[method]` produces a server-streaming RPC. See existing streaming usage in `src/activations/arbor/` and `src/activations/orcha/activation.rs` for the pattern.

Event broadcast: a single `tokio::sync::broadcast::Sender<TicketEvent>` lives inside the `TmActivation` struct, shared across all three watch methods. TM-3's mutation methods publish to this channel after every successful write. Slow subscribers that lag behind the broadcast buffer get a `Lagged` marker event (the standard `broadcast::error::RecvError::Lagged` case mapped to a `TicketEvent::Lagged { missed: u64 }` variant).

Parallelism with TM-3: this ticket adds **publish calls** inside TM-3's existing methods and **three new stream methods**. TM-3 is the only other file-writer in `activation.rs`. To keep file-boundary parallelism, the publish-on-write hook is added as a small `fn publish(&self, event: TicketEvent)` helper on `TmActivation` in this ticket, and TM-3's mutation methods are expected to call it. Coordination: TM-3 has a single TODO marker per mutation method pointing at `self.publish(...)`; TM-5 lands the `publish` helper and edits those TODO sites. If TM-3 is still in flight when TM-5 starts, work can overlap via the marker convention. If TM-3 has already merged, TM-5 adds the publish helper and replaces the TODOs in one PR.

## Required behavior

### `TicketEvent` type

Introduce in `src/activations/tm/types.rs` (extending TM-2's file):

Tagged enum (`#[serde(tag = "event", rename_all = "snake_case")]`):

| Variant | Fields | Emitted when |
|---|---|---|
| `created` | `ticket: Ticket` | `create_ticket` succeeded. |
| `body_updated` | `id: TicketId, updated_at: i64` | `update_ticket_body` succeeded. |
| `status_changed` | `id: TicketId, from: Status, to: Status, updated_at: i64` | `update_ticket_status` succeeded. |
| `deleted` | `id: TicketId` | `delete_ticket` succeeded. |
| `epic_created` | `epic: Epic` | `create_epic` succeeded. |
| `lagged` | `missed: u64` | Subscriber fell behind the broadcast buffer. |

Derives: `Debug, Clone, Serialize, Deserialize, JsonSchema`.

### Streaming RPC methods

| Method | Args | Return | Behavior |
|---|---|---|---|
| `watch_ticket` | `id: TicketId` | `impl Stream<Item = TicketEvent>` | Stream of events concerning exactly one ticket. Filters to events whose `id` field matches; `created` and `deleted` match too. Stream terminates when the client disconnects. |
| `watch_epic` | `prefix: String` | `impl Stream<Item = TicketEvent>` | Stream of events concerning any ticket whose epic prefix matches. Also emits `epic_created` when the epic itself is created. |
| `watch_all` | `(none)` | `impl Stream<Item = TicketEvent>` | Unfiltered stream of every `TicketEvent`. |

All three streams emit a synthetic snapshot at subscription start:

- `watch_ticket(id)` emits a single `created { ticket }` event for the current state of `id` (if it exists) before any live events. If the ticket doesn't exist, no synthetic event is emitted and the stream waits for the first live event referencing it.
- `watch_epic(prefix)` emits `epic_created { epic }` if the epic exists, followed by `created { ticket }` for every current ticket in the epic.
- `watch_all()` emits `created { ticket }` for every current ticket.

After the synthetic prefix, all three switch to tailing the broadcast channel.

### Broadcast channel

- `tokio::sync::broadcast::channel(1024)` — buffer size 1024.
- A subscriber that falls more than 1024 events behind receives a `TicketEvent::Lagged { missed }` and then continues with current events. The stream is not terminated on lag.

### Publish integration with TM-3

TM-3's methods call `self.publish(event)` after every successful storage mutation. The publish helper:

```rust
fn publish(&self, event: TicketEvent) {
    let _ = self.events.send(event); // lossy — no subscribers is fine
}
```

TM-5 lands this helper and inserts the publish calls at the end of `create_ticket`, `update_ticket_body`, `update_ticket_status`, `delete_ticket`, `create_epic`.

## Risks

| Risk | Mitigation |
|---|---|
| Broadcast channel buffer fills up under high write rate. | Lag is surfaced as `TicketEvent::Lagged` rather than silent drop; the subscriber can reconcile by re-subscribing after a full `list_tickets` scan. |
| Synthetic snapshot on `watch_all()` is huge for large DBs. | For this epic, snapshot-on-subscribe is acceptable (substrate workspaces have <500 tickets). Add a `snapshot: bool` arg in a follow-up if real-world usage shows contention. |
| Event order across concurrent writers. | Broadcast guarantees order per producer but not across producers; this is acceptable — consumers treat events as idempotent. |
| TM-3 lands before TM-5 and the `publish` call sites don't exist. | TM-5's PR adds them. TM-3's PR adds a `TODO(TM-5): publish` marker at each site — grep sanity-check before TM-5 merges. |

## What must NOT change

- TM-2's `TicketStore` trait (streaming lives in the activation, not the store).
- TM-3's mutation behavior (the broadcast publish is a side-effect added at the tail of each successful mutation; failure paths do not publish).
- Every other substrate activation.

## Acceptance criteria

1. `cargo build -p plexus-substrate` succeeds.
2. `cargo test -p plexus-substrate` succeeds. Tests cover:

   | Scenario | Expected |
   |---|---|
   | Subscribe to `watch_ticket("X")`, create ticket X, expect 1 `created` event. | Event received within 500ms. |
   | Subscribe to `watch_ticket("X")` for an existing X, expect a synthetic `created` snapshot first. | Snapshot event precedes any live events. |
   | Subscribe to `watch_epic("TM")`, create two TM tickets, expect 2 `created` events. | Both events received; unrelated-epic writes do not appear. |
   | Subscribe to `watch_all()`, perform one of each mutation type, expect one event per mutation. | Events received in the order mutations were submitted. |
   | Update a ticket's status → `watch_ticket` subscriber receives `status_changed { from, to }`. | Exactly one event. |
   | Delete a ticket → `watch_ticket` subscriber receives `deleted`. | Subscriber gets the event, then the stream remains open (does not auto-terminate on delete). |
   | A subscriber that ignores events for >1024 mutations receives a `lagged` event. | `lagged.missed >= 1` and the subsequent events continue. |

3. `synapse tm watch_all` surfaces live events as they occur in a separate shell.

## Completion

- PR adds the three stream methods, the `TicketEvent` enum, the `publish` helper, and the integration points in TM-3's mutation methods.
- PR description includes `cargo build -p plexus-substrate`, `cargo test -p plexus-substrate`, and a transcript of `synapse tm watch_all` observing a live mutation from a second shell.
- Ticket status flipped from `Ready` → `Complete` in the same commit as the code.
