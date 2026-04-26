---
id: TM-2
title: "TicketStore trait + ticket types (Ticket, Epic, Status, DeprecationInfo)"
status: Pending
type: implementation
blocked_by: [TM-S01, TM-S02]
unlocks: [TM-3, TM-4, TM-5, TM-6, TM-7, TM-8, TM-9]
severity: High
target_repo: plexus-substrate
---

## Problem

Before TM exposes any RPC methods or integrates with Orcha, it needs (a) a set of ticket domain types that every downstream ticket agrees on, and (b) a `TicketStore` trait that defines the persistence surface, so backends (SQLite default, plus in-memory for tests) can be swapped without touching activation logic. This matches the per-activation storage pattern already established by `OrchaStore`, `LatticeStore`, `ClaudeCodeStore`, etc. (see STG epic scope and `plans/README.md` under "Trait surfaces").

Without this foundation ticket, TM-3 through TM-9 would each re-derive ticket shape and storage coupling, and they'd drift.

## Context

Target repo: `plexus-substrate` at `/Users/shmendez/dev/controlflow/hypermemetic/plexus-substrate`.

Upstream spike outputs:

- **TM-S01** decided whether `orcha/pm`'s schema moves into `TicketStore` (absorb) or stays separate (coexist). The trait's method set depends on that outcome — if absorb, the trait covers graph-to-ticket mapping and node event logs in addition to ticket lifecycle. If coexist, the trait covers ticket lifecycle only. This ticket's implementation reads TM-S01's decision document and includes/excludes methods accordingly.
- **TM-S02** decided the query surface shape (typed methods vs DSL vs hybrid). The trait's read-side methods follow that shape.

Domain newtype: `TicketId` is owned by the ST epic (`TicketId(String)`, pinned in `plans/README.md`). If ST has landed by the time this ticket is promoted, TM-2 uses `TicketId` directly. If not, TM-2 uses `String` with a `// TODO(ST): migrate to TicketId` marker and leaves a follow-up ticket.

Per-activation storage pattern (for reference — `src/activations/orcha/pm/storage.rs`):

- A config struct (`PmStorageConfig`) with a `db_path: PathBuf`.
- A storage struct (`PmStorage`) wrapping a `SqlitePool`.
- An `init_schema` method creating tables idempotently.
- Methods are concrete, not traited today. TM-2 introduces the **traited** shape going forward — this is the first activation that does so. The STG epic will retrofit existing activations.

## Required behavior

### Domain types

Introduce the following public types in `src/activations/tm/types.rs`:

**`Ticket`** — the primary record. Fields:

| Field | Type | Meaning |
|---|---|---|
| `id` | `TicketId` (or `String` if ST not yet landed) | Unique identifier, e.g., `TM-3`. |
| `epic` | `String` | Epic prefix, e.g., `TM`. |
| `title` | `String` | One-line summary. |
| `status` | `Status` | See below. |
| `ticket_type` | `TicketType` | See below. |
| `severity` | `Option<Severity>` | Critical / High / Medium / Low. |
| `blocked_by` | `Vec<TicketId>` | Upstream dependencies. |
| `unlocks` | `Vec<TicketId>` | Downstream dependencies. |
| `target_repo` | `Option<String>` | Named repo if cross-cutting. |
| `superseded_by` | `Option<TicketId>` | Set when status is `Superseded`. |
| `body` | `String` | Markdown body (everything after the frontmatter). |
| `created_at` | `i64` | Unix epoch seconds. |
| `updated_at` | `i64` | Unix epoch seconds. |

**`Status`** — enum matching the status values in the ticketing skill:

| Variant | Meaning |
|---|---|
| `Pending` | Awaiting human review. |
| `Ready` | Approved for implementation. |
| `Blocked` | Approved but waiting on `blocked_by`. |
| `Complete` | Implemented and committed. |
| `Idea` | Captured, not ready. |
| `Epic` | Overview doc. |
| `Superseded` | Absorbed by `superseded_by`. |

**`TicketType`** — enum: `Implementation`, `Analysis`, `Spike`, `Epic`.

**`Severity`** — enum: `Critical`, `High`, `Medium`, `Low`.

**`Epic`** — record for epic overviews. Fields:

| Field | Type | Meaning |
|---|---|---|
| `prefix` | `String` | Short code, e.g., `TM`. |
| `title` | `String` | Full epic title. |
| `goal` | `String` | Extracted from `## Goal` section. |
| `ticket_ids` | `Vec<TicketId>` | All tickets in the epic. |

**`DeprecationInfo`** — optional, used on tickets that have been deprecated (per the tickets-are-contracts methodology). Fields:

| Field | Type | Meaning |
|---|---|---|
| `since` | `String` | Version/date when deprecated. |
| `removed_in` | `String` | Planned removal. |
| `message` | `String` | Migration guidance. |

All types derive: `Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema` to match substrate's existing activation type conventions (see `orcha/pm/activation.rs` imports for the template).

### `TicketStore` trait

Introduce `TicketStore` in `src/activations/tm/store.rs`. The trait is `async` (via `async_trait`) and the minimum method set is:

| Method | Signature | Behavior |
|---|---|---|
| `create_ticket` | `async fn create_ticket(&self, ticket: Ticket) -> Result<(), TmError>` | Insert; error on duplicate id. |
| `get_ticket` | `async fn get_ticket(&self, id: &TicketId) -> Result<Option<Ticket>, TmError>` | Lookup. `None` if absent. |
| `update_ticket_body` | `async fn update_ticket_body(&self, id: &TicketId, body: String) -> Result<(), TmError>` | Last-write-wins on the `body` field. Bumps `updated_at`. |
| `update_ticket_status` | `async fn update_ticket_status(&self, id: &TicketId, status: Status) -> Result<(), TmError>` | Transitions status. Bumps `updated_at`. Does **not** enforce the human gate — TM-6 owns that at the RPC layer. |
| `delete_ticket` | `async fn delete_ticket(&self, id: &TicketId) -> Result<(), TmError>` | Remove. Error if referenced by another ticket's `blocked_by`/`unlocks`. |
| `list_tickets` | `async fn list_tickets(&self, cursor: Option<String>, limit: usize) -> Result<(Vec<Ticket>, Option<String>), TmError>` | Paginated list; returns next cursor if more remain. |
| `list_by_status` | `async fn list_by_status(&self, status: Status) -> Result<Vec<Ticket>, TmError>` | Returns all tickets at a given status. Used by TM-4's `ready`. |
| `blocked_by` | `async fn blocked_by(&self, id: &TicketId) -> Result<Vec<Ticket>, TmError>` | Returns upstream dependencies. |
| `unlocks` | `async fn unlocks(&self, id: &TicketId) -> Result<Vec<Ticket>, TmError>` | Returns downstream dependencies. |
| `create_epic` | `async fn create_epic(&self, epic: Epic) -> Result<(), TmError>` | Insert an epic record. |
| `get_epic` | `async fn get_epic(&self, prefix: &str) -> Result<Option<Epic>, TmError>` | Lookup by prefix. |
| `list_epics` | `async fn list_epics(&self) -> Result<Vec<Epic>, TmError>` | All epics. |

If TM-S01 decided **absorb**, the trait additionally covers the methods enumerated in TM-S01's decision document — graph-to-ticket mapping and node event log methods. If **coexist**, those are omitted and left to `orcha/pm`.

Extra methods required by TM-5 (watch/stream) and TM-6 (promotion auditing) are added in those tickets, not here — TM-2 lands the minimum surface that TM-3, TM-4, TM-8, TM-9 need.

### `TmError` type

Introduce `TmError` as an enum with variants:

| Variant | Meaning |
|---|---|
| `NotFound { id: String }` | No ticket with that id. |
| `AlreadyExists { id: String }` | Duplicate create. |
| `InvalidTransition { from: Status, to: Status }` | Status transition rejected. |
| `Referenced { id: String, by: Vec<String> }` | Delete blocked by references. |
| `Storage(String)` | Underlying backend error. |

Derives: `Debug, Clone, Serialize, Deserialize, JsonSchema, thiserror::Error` (or `Display` if thiserror isn't already in the workspace).

### Default SQLite backend

Implement `SqliteTicketStore` in `src/activations/tm/storage.rs`, wrapping a `SqlitePool` and using the `activation_db_path("tm", "tm.db")` helper from `src/activations/storage.rs` (matches `pm.db`, `arbor.db`, etc. convention).

Schema: tables for `tickets` and `epics` with indices on `status`, `epic`. Use `CREATE TABLE IF NOT EXISTS` + column-additive ALTERs for idempotent init, matching `PmStorage::init_schema`.

### Module layout

```
src/activations/tm/
  mod.rs        # module exports
  types.rs      # Ticket, Status, Severity, TicketType, Epic, DeprecationInfo, TmError
  store.rs      # TicketStore trait
  storage.rs    # SqliteTicketStore (and in-memory InMemoryTicketStore for tests)
```

The activation file (`activation.rs`) is introduced by TM-3 — this ticket does not create one.

## Risks

| Risk | Mitigation |
|---|---|
| `TicketId` not yet shipped by ST. | Use `String` with a `TODO(ST)` marker. Trait signatures switch later in a one-line rename. |
| TM-S01 decides absorb but schema extends beyond what this ticket scoped. | Re-scope this ticket before promotion; add the absorbed methods to the trait's required set. |
| SQLite schema changes mid-epic. | Additive ALTERs only; never rename columns. Matches existing `PmStorage` convention. |
| Cyclical ticket references (`blocked_by` and `unlocks` disagree). | `create_ticket` and `update` reject cycles; `blocked_by` / `unlocks` queries tolerate any shape that slipped through. Consistency is best-effort in storage, authoritative in the RPC-layer validation (TM-3). |

## What must NOT change

- Every other substrate activation continues to compile and test green. This ticket is purely additive.
- `orcha/pm`'s schema and surface are unchanged unless TM-S01 decided absorb — and even then, the actual deletion of `pm` is out of this ticket's scope (it's a follow-up).
- The activation registration in `src/plugin_system/` does **not** yet wire up TM — that happens in TM-3 when the activation exists.
- No `.md` files under `plans/` are touched.

## Acceptance criteria

1. `cargo build -p plexus-substrate` succeeds with TM's new module present.
2. `cargo test -p plexus-substrate` succeeds. New tests cover:
   - `SqliteTicketStore` round-trip of a `Ticket` with every `Status` variant.
   - `SqliteTicketStore` round-trip of an `Epic` with a non-empty `ticket_ids`.
   - `list_by_status(Status::Ready)` returns only the `Ready` tickets when a mix is inserted.
   - `delete_ticket` on a ticket referenced by another's `blocked_by` returns `TmError::Referenced`.
   - `blocked_by` and `unlocks` return the correct set for a three-ticket chain (A blocks B blocks C).
   - `get_ticket` on an absent id returns `Ok(None)`.
3. An `InMemoryTicketStore` variant is present and passes the same test suite (parametrized over the two backends).
4. The module `src/activations/tm/` compiles in isolation; no other file in substrate imports `tm::types` or `tm::store` yet (this ticket is foundation only).
5. The types derive `JsonSchema` — verified by constructing each and passing it through `schemars::schema_for!` without panic.

## Completion

- PR adds the four files under `src/activations/tm/`.
- PR description includes `cargo build -p plexus-substrate` and `cargo test -p plexus-substrate` output — both green.
- Ticket status flipped from `Ready` → `Complete` in the same commit as the code.
- PR notes that TM-3 through TM-9 are unblocked.
