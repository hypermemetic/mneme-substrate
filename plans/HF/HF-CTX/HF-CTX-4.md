---
id: HF-CTX-4
title: "Fact-emission hooks in BuildHub / RepoHub / ReleasesHub / ImagesHub / AuthHub"
status: Pending
type: implementation
blocked_by: [HF-CTX-2]
unlocks: [HF-CTX-7]
severity: High
target_repo: hyperforge
---

## Problem

Hyperforge's existing hubs (`BuildHub`, `RepoHub`, `ReleasesHub`, `ImagesHub`, `AuthHub`) perform the operations that define facts — version bumps, publishes, tag creates, image pushes, release uploads, auth checks — but they currently emit only runtime `HyperforgeEvent`s to the in-memory event stream. Those events are not durable and do not compose into the fact log. This ticket introduces a `FactSink` trait, injects it at each hub's construction, and adds `sink.append(...)` calls at every operation point that produces a fact per HF-CTX-S01's `HyperforgeEvent → Fact` mapping.

## Context

Target repo: `hyperforge`.

Upstream inputs:

- **HF-CTX-2.** The `Fact` enum, `FactRecord`, `TicketStore::append_fact` are all live.
- **HF-CTX-S01's mapping table.** Specifies exactly which `HyperforgeEvent` variants produce which `Fact`s, and which `Fact`s have no prior `HyperforgeEvent` counterpart (added net-new by this ticket).

Seed emission points (concrete hub operations that already exist today, derived from the hub layout at `src/hubs/`):

| Hub | Operation | Fact emitted |
|---|---|---|
| `BuildHub::publish` | Crate/cabal/npm publish. | `VersionPublished { package, ecosystem, version, at_commit, at_tag }` |
| `BuildHub::bump_version` | Cargo.toml / cabal / package.json version bump. | `VersionBumped { package, from, to, at_commit, at_ticket }` |
| `BuildHub::publish` (on failure after at least one succeeded) | Publish partial-failure state. | No fact (status stays runtime-event). |
| `RepoHub::push` | Git push to remote. | Zero or more `Tagged` facts (one per tag advanced). |
| `RepoHub::create_tag` | Git tag create. | `Tagged { tag, commit, package }` |
| `RepoHub::dirty_audit` (per-file result) | Modified file under a ticketed work session. | `TouchedPath { ticket_id, path, change_kind }` — emitted only when a ticket-session context is active (see "Emission context" below). |
| `ReleasesHub::create` | GitHub/GitLab release create. | `VersionPublished` + `Tagged` (compound). |
| `ReleasesHub::upload_asset` | Release asset upload. | No fact (asset uploads are release-scoped runtime details). |
| `ImagesHub::push` | Container image push. | `VersionPublished { package: <image_name>, ecosystem: Oci, version: <tag>, at_commit, at_tag }` |
| `ImagesHub::delete` | Image delete. | `ArtifactRemoved { artifact_id: <image_ref>, at_commit, at_ticket }` |
| `AuthHub::check` | Auth credential check. | No fact by default (runtime-only). |

HF-CTX-S01's pinned mapping may add, remove, or adjust these. This ticket follows S01 exactly.

"Net-new" facts — ones with no `HyperforgeEvent` counterpart — are emitted from the CTX-owned paths, not from the existing hubs. HF-CTX-3's CRUD methods emit `TicketCreated`, `TicketStatusChanged`, `TicketLanded`, `DependsOn`. HF-CTX-5's query methods emit none. HF-CTX-8's promote gate emits `TicketStatusChanged`. This ticket scope is **only the hubs** listed above; CTX-activation-internal emission is owned by the respective CTX tickets.

File-boundary discipline: this ticket touches the existing hub files — `src/hubs/build/*`, `src/hubs/repo.rs`, `src/hubs/releases.rs`, `src/hubs/images.rs`, `src/auth_hub/*`. These files are disjoint from HF-CTX-3's (`src/ctx/activation.rs`), HF-CTX-5's queries file, and HF-CTX-6's watch file. All four tickets run in parallel per the DAG.

## Required behavior

### `FactSink` trait

Introduce in the types crate alongside `Fact`:

```text
#[async_trait]
pub trait FactSink: Send + Sync {
    async fn append(&self, record: FactRecord) -> Result<FactId, CtxError>;
}
```

A blanket impl wraps any `Arc<dyn TicketStore>` as a `FactSink` (by forwarding to `append_fact`).

A no-op `NullFactSink` is also provided for tests and for environments where fact logging is disabled.

### Hub constructor injection

Each of `BuildHub`, `RepoHub`, `ReleasesHub`, `ImagesHub`, `AuthHub` takes an `Arc<dyn FactSink>` as an additional constructor argument. Default instantiation in the hub registry threads in the same `TicketStore` backing the `hyperforge.ctx` activation.

Existing hub constructor callers that do not yet know about `FactSink` accept the no-op sink (backward-compatible default): the hub's constructor has a `new_with_sink(..., sink: Arc<dyn FactSink>)` variant alongside the original `new(...)` which wires the no-op sink. Once the hub registry wires the real sink in, every production path goes through `new_with_sink`.

### Emission hooks

For each row in the mapping table (HF-CTX-S01's authoritative version — the seed table above is the starting point), insert a `sink.append(FactRecord { fact, valid_at, source_commit, source_ticket })` call after the operation succeeds. Failed operations do not emit facts (failure stays an `Error` event on the runtime stream only).

Rules:

- `valid_at` is always `SystemTime::now().unix_seconds()` at emission.
- `source_commit` is populated when the operation knows its commit (publish and tag paths know it; dirty-audit paths know it).
- `source_ticket` is populated when the operation is running inside a ticketed work session — see "Emission context" below.
- Fact append failure does **not** fail the operation. The hub logs a warning on sink failure and the user-visible operation still succeeds. (Rationale: fact emission is observability; it must not regress hub behavior if the DB is down.)

### Emission context

A `WorkContext` struct carries the current ticket id (and associated metadata) through hub calls:

```text
#[derive(Clone)]
pub struct WorkContext {
    pub ticket_id: Option<TicketId>,
    pub commit: Option<CommitRef>,
}
```

Every hub method that emits a fact accepts an optional `WorkContext` argument (or threads it through an existing context object — pinned by the hub's existing signature style). When absent, `source_ticket` and `source_commit` in emitted facts are `None`.

Callers that want ticket-scoped emission (e.g., Orcha running a graph whose root is a ticket) pass a `WorkContext` with the ticket id populated. Callers that invoke hubs directly (e.g., a human running `synapse hyperforge build publish`) pass `WorkContext::default()`, and emitted facts have no `source_ticket`.

### Compound emission

`ReleasesHub::create` is a compound operation: it creates a tag and publishes a release. This ticket emits both facts as two separate `FactRecord`s with the same `valid_at` (or monotonically increasing within the same call). The sink receives each as a separate append. Rationale: facts are atomic; compound operations produce multiple atomic facts.

### Observability

Every successful fact emission is logged at `debug` level with the fact's `kind` discriminator and `source_ticket`/`source_commit` if present. Warning on sink-append failure includes the fact's kind and the underlying error.

## Risks

| Risk | Mitigation |
|---|---|
| `FactSink` injection churns every hub constructor signature. | Backward-compat via `new_with_sink` vs `new`. Existing consumers that construct hubs directly (tests, mocks) keep working with the no-op sink. |
| Fact append latency slows hub operations. | Sink append is async and can be fire-and-forget (spawn a task) in hubs where latency matters. Pinned per-operation: `BuildHub::publish` awaits; `RepoHub::dirty_audit` batches and spawns. |
| HF-CTX-S01's mapping diverges from the seed table. | This ticket consumes S01's mapping verbatim. If S01 diverged, re-scope this ticket before promotion. |
| Multiple hubs race on the same `append_fact` for a compound operation. | Each `FactRecord` is independent; SQLite's row-level sequencing handles ordering. Tests pin this. |
| No `WorkContext` is threaded to a hub call, leading to orphaned facts (no `source_ticket`). | By design: hubs called outside a ticketed session produce ticket-less facts. Query side (HF-CTX-5) handles both cases. |

## What must NOT change

- Every existing hub's user-visible behavior: argument lists, return values, error paths. Fact emission is additive plumbing.
- `HyperforgeEvent`'s wire shape — events continue flowing through the runtime stream as before. Facts are durable siblings, not replacements.
- Every other hyperforge hub's compile and test behavior.
- HF-CTX-2's trait surface.
- Existing `plans/<EPIC>/*.md` files.

## Acceptance criteria

1. `cargo build --workspace` succeeds in hyperforge.
2. `cargo test --workspace` succeeds. New tests cover:

   | Scenario | Expected |
   |---|---|
   | `BuildHub::publish` of a crate against a test sink, no `WorkContext` | One `VersionPublished` record appended; `source_ticket == None`. |
   | `BuildHub::publish` with a `WorkContext { ticket_id: Some(TID), .. }` | One `VersionPublished` record with `source_ticket == Some(TID)`. |
   | `BuildHub::bump_version` from 0.4.0 → 0.5.0 | One `VersionBumped { from: 0.4.0, to: 0.5.0 }` record. |
   | `RepoHub::create_tag` | One `Tagged` record with the tag ref and commit. |
   | `ReleasesHub::create` | Both `VersionPublished` and `Tagged` records present. |
   | `ImagesHub::push` | One `VersionPublished` with `ecosystem: Oci`. |
   | `ImagesHub::delete` | One `ArtifactRemoved`. |
   | `BuildHub::publish` with a sink that returns an error on `append` | Operation still succeeds; warning logged; fact table unchanged. |
   | Hub constructor without a sink (`new`) | No-op sink; hub operates; no facts emitted. |
   | `list_facts(filter { kind: Some("version_published") })` after a mixed sequence | Returns exactly the `VersionPublished` records in insertion order. |

3. A regression pin: every existing hub test from before this ticket continues to pass unchanged (no hub behavior regressed).
4. `cargo build --workspace` + `cargo test --workspace` pass green end-to-end.

## Completion

- Commit adds `FactSink` + `NullFactSink` to the types crate, injects sinks into all five hubs, and inserts the emission hooks per HF-CTX-S01's mapping.
- Commit message includes a summary of which hubs emit which facts.
- `cargo build --workspace` + `cargo test --workspace` output — both green — appended to the ticket's commit description.
- If public-surface changes to hub constructors warrant a minor bump beyond HF-CTX-2's `4.3.0`, bump now (e.g., `4.4.0`) and create a new local annotated tag. If purely additive-internal, contribute to `4.3.x` as a patch.
- Ticket status flipped from `Ready` → `Complete` in the same commit as the code.
