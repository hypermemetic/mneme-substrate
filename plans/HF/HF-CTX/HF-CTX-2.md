---
id: HF-CTX-2
title: "Fact enum, TicketScope, TicketStore trait, SQLite + in-memory backends"
status: Pending
type: implementation
blocked_by: [HF-CTX-S02]
unlocks: [HF-CTX-3, HF-CTX-4, HF-CTX-5, HF-CTX-6]
severity: High
target_repo: hyperforge
---

## Problem

HF-CTX-S01 has pinned the `Fact` enum and `TicketScope` struct. HF-CTX-S02 has pinned the query signatures. With taxonomies pinned, hyperforge still has no code materializing them: no type definitions, no storage surface, no trait a backend implements. This ticket introduces the foundation — `Fact` and `TicketScope` types, a `TicketStore` trait covering CRUD + fact-append, a default SQLite backend, and an in-memory backend for tests — so HF-CTX-3 (ticket CRUD + scope parsing), HF-CTX-4 (fact emission hooks), HF-CTX-5 (query methods), and HF-CTX-6 (watch streams) can each depend on a stable shape.

## Context

Target repo: `hyperforge` at `/Users/shmendez/dev/controlflow/hypermemetic/hyperforge/`.

Upstream inputs:

- **HF-CTX-S01's decision document.** Pins every `Fact` variant, every `TicketScope` field, the `HyperforgeEvent → Fact` mapping, the types-crate ownership decision (`hyperforge-types` vs `hyperforge-ctx-types`), and the activation namespace.
- **HF-CTX-S02's decision document.** Pins each query's Rust signature, the `ZoomedView` shape, the `Graph` shape for `blast_radius`, the aggregation primitives for zoom, and the library-vs-RPC exposure per query.
- **HF-TT newtypes.** `PackageName`, `Ecosystem`, `ArtifactId`, `Version`, `CommitRef`, `RepoName`, `WorkspaceRoot`, `RepoPath`, `TagRef`, `TicketId` — all already live in hyperforge's types crate. This ticket consumes them; it does not introduce or duplicate them.

The `TicketStore` trait follows the per-activation storage pattern established by `OrchaStore`, `LatticeStore`, etc. in plexus-substrate. It's async (via `async_trait`), the default backend is SQLite, and an in-memory backend implements the same trait for tests.

Non-exhaustive enums: `Fact` carries `#[non_exhaustive]` so patch bumps can add variants. `TicketScope` is `#[non_exhaustive]` for the same reason.

Version bump: this is the first HF-CTX ticket that adds public surface. Hyperforge bumps from the current 4.2.x (post-HF-IR) to **4.3.0** in this ticket, and a local annotated tag `hyperforge-v4.3.0` is created (not pushed). Subsequent HF-CTX tickets contribute to the 4.3.x line in patch bumps until a minor bump is warranted (HF-CTX-7 zoom or HF-CTX-8 promote gate may warrant one).

## Required behavior

### Domain types

Introduce in the types crate pinned by HF-CTX-S01 (assume `hyperforge-types` for this ticket; adjust if S01 chose `hyperforge-ctx-types`):

**`Fact`** — tagged enum (`#[serde(tag = "kind", rename_all = "snake_case")]`), `#[non_exhaustive]`. Every variant carries the shared metadata at the outer record level via a struct wrapper:

```text
pub struct FactRecord {
    pub fact: Fact,
    pub valid_at: i64,                       // unix seconds
    pub source_commit: Option<CommitRef>,
    pub source_ticket: Option<TicketId>,
}
```

Variants and their payloads are whatever HF-CTX-S01 pinned. At minimum the seed set from HF-CTX-1 is present: `TicketCreated`, `TicketStatusChanged`, `TicketLanded`, `ArtifactIntroduced`, `ArtifactRemoved`, `ArtifactRenamed`, `ArtifactDeprecated`, `VersionPublished`, `VersionBumped`, `VersionPinChanged`, `CompatibilityBroken`, `CompatibilityRestored`, `SchemaChanged`, `MigrationApplied`, `ConfigChanged`, `DocAuthored`, `DecisionRecorded`, `ResearchConcluded`, `TouchedPath`, `Tagged`, `DependsOn`.

Payload field types use HF-TT newtypes — never `String` where a newtype fits.

**`TicketScope`** — struct, `#[non_exhaustive]`. Fields are whatever HF-CTX-S01 pinned. At minimum the HF-CTX-1 seed set: `repos: Vec<RepoName>`, `packages: Vec<PackageIdent>`, `ecosystems: Vec<Ecosystem>`, `starts_from: HashMap<RepoName, CommitRef>`, `ends_at: HashMap<RepoName, CommitRef>`, `versions_before: HashMap<PackageName, Version>`, `versions_after: HashMap<PackageName, Version>`, `introduces: Vec<ArtifactId>`, `deprecates: Vec<ArtifactId>`, `removes: Vec<ArtifactId>`, `touches: Vec<ArtifactId>`, `tags_created: Vec<TagRef>`.

`PackageIdent` is a struct `{ ecosystem: Ecosystem, package: PackageName }` (pinned by HF-CTX-S01).

**`Ticket`** — record. Fields:

| Field | Type | Meaning |
|---|---|---|
| `id` | `TicketId` | Unique identifier. |
| `epic` | `String` | Epic prefix. |
| `title` | `String` | One-line summary. |
| `status` | `Status` | Lifecycle state. |
| `ticket_type` | `TicketType` | Implementation / Analysis / Spike / Epic. |
| `severity` | `Option<Severity>` | Critical / High / Medium / Low. |
| `blocked_by` | `Vec<TicketId>` | Upstream dependencies. |
| `unlocks` | `Vec<TicketId>` | Downstream dependencies. |
| `target_repo` | `Option<RepoName>` | Cross-cutting repo. |
| `superseded_by` | `Option<TicketId>` | Set when status is `Superseded`. |
| `scope` | `TicketScope` | Parsed scope block. |
| `body` | `String` | Markdown body. |
| `created_at` | `i64` | Unix epoch seconds. |
| `updated_at` | `i64` | Unix epoch seconds. |

**`Status`** — enum: `Pending`, `Ready`, `Blocked`, `Complete`, `Idea`, `Epic`, `Superseded`.

**`TicketType`** — enum: `Implementation`, `Analysis`, `Spike`, `Epic`.

**`Severity`** — enum: `Critical`, `High`, `Medium`, `Low`.

**`Epic`** — record. Fields: `prefix: String`, `title: String`, `goal: String`, `ticket_ids: Vec<TicketId>`.

All types derive: `Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema`. `Fact` and `TicketScope` additionally carry `#[non_exhaustive]`.

### `TicketStore` trait

Introduce the trait (async via `async_trait`). Minimum method set covers CRUD + fact-append:

| Method | Signature (sketch) | Behavior |
|---|---|---|
| `create_ticket` | `async fn create_ticket(&self, ticket: Ticket) -> Result<(), CtxError>` | Insert; error on duplicate id. |
| `get_ticket` | `async fn get_ticket(&self, id: &TicketId) -> Result<Option<Ticket>, CtxError>` | Lookup. |
| `update_ticket_body` | `async fn update_ticket_body(&self, id: &TicketId, body: String) -> Result<(), CtxError>` | Replaces body; bumps `updated_at`. |
| `update_ticket_status` | `async fn update_ticket_status(&self, id: &TicketId, status: Status) -> Result<(), CtxError>` | Writes status; bumps `updated_at`. (HF-CTX-3 enforces the state-transition table; HF-CTX-8 enforces the human gate.) |
| `update_ticket_scope` | `async fn update_ticket_scope(&self, id: &TicketId, scope: TicketScope) -> Result<(), CtxError>` | Replaces `scope`; bumps `updated_at`. |
| `delete_ticket` | `async fn delete_ticket(&self, id: &TicketId) -> Result<(), CtxError>` | Remove. Error if referenced. |
| `list_tickets` | `async fn list_tickets(&self, cursor: Option<String>, limit: usize) -> Result<(Vec<Ticket>, Option<String>), CtxError>` | Paginated. |
| `list_by_status` | `async fn list_by_status(&self, status: Status) -> Result<Vec<Ticket>, CtxError>` | Filtered. |
| `blocked_by_tickets` | `async fn blocked_by_tickets(&self, id: &TicketId) -> Result<Vec<Ticket>, CtxError>` | Upstream deps. |
| `unlocks_tickets` | `async fn unlocks_tickets(&self, id: &TicketId) -> Result<Vec<Ticket>, CtxError>` | Downstream deps. |
| `create_epic` | `async fn create_epic(&self, epic: Epic) -> Result<(), CtxError>` | Insert epic. |
| `get_epic` | `async fn get_epic(&self, prefix: &str) -> Result<Option<Epic>, CtxError>` | Lookup. |
| `list_epics` | `async fn list_epics(&self) -> Result<Vec<Epic>, CtxError>` | All epics. |
| `append_fact` | `async fn append_fact(&self, record: FactRecord) -> Result<FactId, CtxError>` | Append-only; returns the new row's id. |
| `list_facts` | `async fn list_facts(&self, filter: FactFilter, cursor: Option<String>, limit: usize) -> Result<(Vec<FactRecord>, Option<String>), CtxError>` | Paginated fact scan with a filter struct. |

`FactFilter` is a struct with optional fields: `ticket: Option<TicketId>`, `kind: Option<String>` (matches the fact's `kind` discriminator), `package: Option<PackageName>`, `artifact: Option<ArtifactId>`, `since: Option<i64>`, `until: Option<i64>`. All-None means unfiltered.

`FactId` is a newtype `FactId(u64)` — monotonic row id.

Query-method signatures (the seven from HF-CTX-S02) are **added to the trait in HF-CTX-5**, not here. HF-CTX-2 lands the CRUD + fact-append surface that every sibling HF-CTX ticket needs.

### `CtxError` type

Enum variants:

| Variant | Meaning |
|---|---|
| `NotFound { id: String }` | No ticket with that id. |
| `AlreadyExists { id: String }` | Duplicate create. |
| `InvalidTransition { from: Status, to: Status }` | Status transition rejected. |
| `Referenced { id: String, by: Vec<String> }` | Delete blocked by references. |
| `Storage(String)` | Underlying backend error. |

Derives: `Debug, Clone, Serialize, Deserialize, JsonSchema, thiserror::Error`.

### Default SQLite backend

`SqliteTicketStore` wraps a `SqlitePool`. Schema: tables for `tickets`, `epics`, `facts` with indices on:

- `tickets.status`
- `tickets.epic`
- `facts(valid_at, source_ticket)`
- `facts.kind`
- `facts(package, ecosystem)` (when the fact carries those fields; indexed via JSON extract or a denormalized column — pinned by the HF-CTX-S01 schema decision if made, else implementation choice here).

`CREATE TABLE IF NOT EXISTS` + column-additive ALTERs for idempotent init.

### In-memory backend

`InMemoryTicketStore` implements the same trait with `RwLock<HashMap<...>>`-backed collections. Used in tests; also serves as a reference implementation for smoke testing the trait contract.

### Module layout

```
<hyperforge-types or hyperforge-ctx-types>/src/
  fact.rs         # Fact, FactRecord, FactId, FactFilter
  ticket.rs       # Ticket, Status, TicketType, Severity, Epic, TicketScope, PackageIdent
  error.rs        # CtxError
  lib.rs          # re-exports

<hyperforge>/src/ctx/  (or wherever HF-CTX-S01 pins the layer)
  store.rs        # TicketStore trait
  sqlite.rs       # SqliteTicketStore
  memory.rs       # InMemoryTicketStore
  mod.rs          # re-exports
```

### Version bump + tag

- `hyperforge`'s crate version bumps to `4.3.0` in this ticket's commit.
- A local annotated tag `hyperforge-v4.3.0` is created in the same commit. Not pushed.
- If the types crate is separate (`hyperforge-types`), it also bumps to match its own cadence (minor bump) and gets its own tag.

## Risks

| Risk | Mitigation |
|---|---|
| HF-CTX-S01's final taxonomy diverges materially from the seed set. | This ticket consumes S01's output verbatim; if S01 diverged, re-scope this ticket before promotion. |
| SQLite schema locks in column shapes that the next fact variant can't fit. | `Fact` payloads are stored as JSON (TEXT column); indexed columns are denormalized on write. Adding a fact variant is a code-only change. |
| `FactFilter` grows unbounded over time. | This ticket lands only the fields the seven HF-CTX-5 queries need. New filter fields added additively in later tickets. |
| Trait churn between HF-CTX-2 and HF-CTX-5 breaks sibling HF-CTX-3/4/6 tickets. | HF-CTX-5 adds methods to the trait; it does not modify existing methods. Tests in HF-CTX-2 pin the CRUD + fact-append surface. |

## What must NOT change

- Every other hyperforge hub (`BuildHub`, `RepoHub`, `ReleasesHub`, `ImagesHub`, `AuthHub`) continues to compile and test green. This ticket is additive.
- `HyperforgeEvent`'s wire shape. The `HyperforgeEvent → Fact` mapping is live-only going forward (HF-CTX-4); no events are retroactively converted in this ticket.
- The HF-IR reformed activation surface. Fact types are new; existing methods are untouched.
- Existing `plans/<EPIC>/*.md` files. This ticket does not read or write them.

## Acceptance criteria

1. `cargo build --workspace` succeeds in hyperforge with the new types and store module present.
2. `cargo test --workspace` succeeds in hyperforge. New tests cover:

   | Scenario | Expected |
   |---|---|
   | `SqliteTicketStore` round-trip of a `Ticket` with every `Status` variant. | Inserted state equals fetched state byte-for-byte on every field. |
   | `SqliteTicketStore` round-trip of an `Epic` with non-empty `ticket_ids`. | Equal. |
   | `list_by_status(Status::Ready)` returns only Ready tickets when a mix is inserted. | Correct filter. |
   | `delete_ticket` on a referenced ticket returns `CtxError::Referenced`. | Error; ticket remains. |
   | `blocked_by_tickets` / `unlocks_tickets` on a 3-ticket chain (A blocks B blocks C). | `blocked_by_tickets(B) == [A]`, `unlocks_tickets(B) == [C]`. |
   | `get_ticket` on absent id. | `Ok(None)`. |
   | `append_fact` of one `TicketCreated` record, then `list_facts(filter { ticket: Some(..) })`. | Single-record list back. |
   | `append_fact` of 10 different-kind facts, then `list_facts(filter { kind: Some("artifact_introduced") })`. | Only the `ArtifactIntroduced` records. |
   | `list_facts` pagination with 25 facts, `limit: 10`. | First call 10 + cursor; second call 10 + cursor; third call 5 + `None`. |
   | Every `Fact` variant and every `TicketScope` field is constructible and survives a `serde_json::to_value → from_value` round-trip. | Equal. |

3. `InMemoryTicketStore` passes the same test suite (parametrized over the two backends).

4. The crate's `Cargo.toml` shows version `4.3.0` (or the pinned bump from HF-CTX-S01 if different). A local annotated git tag `hyperforge-v4.3.0` exists on the commit that lands this ticket.

5. `schemars::schema_for!` produces a valid schema for each of `Fact`, `FactRecord`, `TicketScope`, `Ticket`, `Epic`, `CtxError` without panic.

## Completion

- Commit lands the new types crate additions, the `ctx` module under hyperforge, version bump, and the local annotated tag.
- Commit message notes HF-CTX-3, HF-CTX-4, HF-CTX-5, HF-CTX-6 are unblocked.
- `cargo build --workspace` + `cargo test --workspace` output appended to the ticket's commit description — both green.
- Ticket status flipped from `Ready` → `Complete` in the same commit as the code.
