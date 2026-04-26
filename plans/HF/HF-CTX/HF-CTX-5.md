---
id: HF-CTX-5
title: "Knowledge-graph query methods (library API + RPC)"
status: Pending
type: implementation
blocked_by: [HF-CTX-2]
unlocks: [HF-CTX-7]
severity: High
target_repo: hyperforge
---

## Problem

HF-CTX-2 lands the fact-append and ticket CRUD surface, but no queries run against it yet. This ticket implements the seven knowledge-graph queries pinned in HF-CTX-S02 — `compat_broken`, `work_on`, `blast_radius`, `zoom` *(base case, depth ≤ 1)*, `currently_deprecated`, `incompat_observers`, `who_touches` — and exposes each as both (a) a Rust library method on the `TicketStore` trait and (b) a `#[plexus_macros::method]` on the `hyperforge.ctx` activation.

## Context

Target repo: `hyperforge`.

Upstream inputs:

- **HF-CTX-2.** `TicketStore` trait with CRUD + fact-append is live. `Fact`, `FactRecord`, `TicketScope` are pinned. `list_facts(filter, ...)` provides the primitive scan that every query builds on.
- **HF-CTX-S02.** Each query's exact Rust signature is pinned. `ZoomedView` struct is pinned. `Graph` / `BlastRadiusGraph` shape is pinned. Library-vs-RPC exposure per query is pinned (default: both).

File-boundary discipline: this ticket owns one new file `src/ctx/queries.rs`. It extends the `TicketStore` trait (additive only — no signature changes to HF-CTX-2's methods) and adds query methods to `src/ctx/activation.rs` by appending to the end — HF-CTX-3 owns the CRUD portion. This ticket appends a distinct method cluster; the conflict between HF-CTX-3 and HF-CTX-5 on the activation file is resolved by landing order (whichever lands second adds methods with clear cluster separators).

If file-boundary contention becomes real at commit time, split `src/ctx/activation.rs` into `src/ctx/activation_crud.rs` (HF-CTX-3) and `src/ctx/activation_queries.rs` (HF-CTX-5) via a shared `mod.rs`. This is a mechanical split, not a semantic change.

Query semantics per HF-CTX-1 and HF-CTX-S02:

- **`compat_broken(Consumer, Dep)`** — walks facts backward from `now`: finds the latest `CompatibilityBroken { consumer, dep }` with no subsequent `CompatibilityRestored` for the same pair. Returns the `source_ticket` of the ticket that completed *immediately before* the break, via `list_by_status(Complete)` ordered by `updated_at` and intersected against the break's `valid_at`.
- **`work_on(ArtifactId)`** — scans `list_facts(filter { artifact: Some(id) })` across `ArtifactIntroduced`, `ArtifactRemoved`, `ArtifactRenamed`, `ArtifactDeprecated`, and `TouchedPath` (where the touched path maps to the artifact). Returns distinct `source_ticket`s ordered by `valid_at`.
- **`blast_radius(TicketId, depth)`** — BFS starting at the ticket, following `DependsOn` and `TouchedPath` / `ArtifactIntroduced` / etc. up to `depth` hops. Returns nodes + edges.
- **`zoom(epic_prefix, depth)`** — depth 0: single-ticket fact bundle; depth 1: epic-wide aggregation; depth ≥ 2: precomputed rollups (owned by HF-CTX-7). **This ticket implements depths 0 and 1 only.** HF-CTX-7 adds the rollup infrastructure and extends `zoom` to arbitrary depth.
- **`currently_deprecated()`** — scans all `ArtifactDeprecated` facts; subtracts any artifact also in an `ArtifactRemoved` fact. Returns remaining `ArtifactId`s.
- **`incompat_observers(PackageName, Version)`** — scans `CompatibilityBroken` facts whose payload mentions the given `(package, version)` pair (on either side). Returns distinct `source_ticket`s.
- **`who_touches(RepoPath)`** — scans `TouchedPath` facts filtered by `path == query path`. Returns distinct `source_ticket`s.

## Required behavior

### Library API: trait extension

Add to the `TicketStore` trait (HF-CTX-2's file — additive):

| Method | Signature | Behavior |
|---|---|---|
| `query_compat_broken` | `async fn query_compat_broken(&self, consumer: PackageIdent, dep: PackageIdent) -> Result<Option<TicketId>, CtxError>` | See semantics above. |
| `query_work_on` | `async fn query_work_on(&self, artifact: ArtifactId) -> Result<Vec<TicketId>, CtxError>` | See semantics. |
| `query_blast_radius` | `async fn query_blast_radius(&self, ticket: TicketId, depth: u32) -> Result<BlastRadiusGraph, CtxError>` | See semantics. |
| `query_zoom` | `async fn query_zoom(&self, epic_prefix: String, depth: u32) -> Result<ZoomedView, CtxError>` | Depth 0 and 1 only. `depth > 1` returns `CtxError::Unsupported` until HF-CTX-7 lands. |
| `query_currently_deprecated` | `async fn query_currently_deprecated(&self) -> Result<Vec<ArtifactId>, CtxError>` | See semantics. |
| `query_incompat_observers` | `async fn query_incompat_observers(&self, package: PackageName, version: Version) -> Result<Vec<TicketId>, CtxError>` | See semantics. |
| `query_who_touches` | `async fn query_who_touches(&self, path: RepoPath) -> Result<Vec<TicketId>, CtxError>` | See semantics. |

`Unsupported` is a new `CtxError` variant (add in this ticket).

Each query has a default trait-level implementation expressed in terms of `list_facts` and the existing CRUD methods, so both `SqliteTicketStore` and `InMemoryTicketStore` get the behavior for free. Backends override only for performance — none needed in this ticket.

### RPC surface: `hyperforge.ctx` activation

Each library method is additionally exposed as a `#[plexus_macros::method]` on the `hyperforge.ctx` activation. RPC method names and argument/return types match the library methods. Result types are tagged enums:

| RPC method | Return type | Variants |
|---|---|---|
| `compat_broken` | `CtxCompatBrokenResult` | `ok { ticket: Option<TicketId> }`, `err { message }` |
| `work_on` | `CtxTicketsResult` | `ok { tickets: Vec<TicketId> }`, `err { message }` |
| `blast_radius` | `CtxBlastRadiusResult` | `ok { graph: BlastRadiusGraph }`, `err { message }` |
| `zoom` | `CtxZoomResult` | `ok { view: ZoomedView }`, `unsupported { reason }`, `err { message }` |
| `currently_deprecated` | `CtxArtifactsResult` | `ok { artifacts: Vec<ArtifactId> }`, `err { message }` |
| `incompat_observers` | `CtxTicketsResult` | `ok { tickets: Vec<TicketId> }`, `err { message }` |
| `who_touches` | `CtxTicketsResult` | `ok { tickets: Vec<TicketId> }`, `err { message }` |

### Result types

`BlastRadiusGraph`, `ZoomedView` — shapes pinned by HF-CTX-S02. Both derive `Debug, Clone, Serialize, Deserialize, JsonSchema`.

### Pagination

Each query that returns a `Vec<TicketId>` has an implicit upper bound of 10,000 results. Callers expecting more must use `list_facts` directly with paginated scans. (HF-CTX-1 risk: large fact logs. Pin: 10k result cap at the query layer is enough for realistic knowledge-graph use; paginated facts are the escape hatch.)

## Risks

| Risk | Mitigation |
|---|---|
| HF-CTX-S02's signatures diverge from the seed set above. | This ticket consumes S02's output verbatim; adjust the signature table before promotion if S02 pinned different shapes. |
| `query_zoom(depth = 0 | 1)` overlaps with HF-CTX-7's rollup-based implementation. | This ticket implements the base cases; HF-CTX-7 swaps the depth-≥-1 implementation for the rollup-backed path and extends to arbitrary depth. Trait method signature stays stable. |
| File-boundary collision with HF-CTX-3 on `activation.rs`. | See Context. Mechanical split into `activation_crud.rs` / `activation_queries.rs` available if both commits land close together. |
| Query results are non-deterministic when facts have identical `valid_at` seconds. | Secondary sort by `FactId` (monotonic) on any query that orders by time. Pinned in the implementation. |
| Slow full-scan queries on large fact logs. | This ticket's default implementations accept O(facts) scans. HF-CTX-7's rollups optimize later. Performance work is a follow-up epic. |

## What must NOT change

- HF-CTX-2's existing trait method signatures (this ticket only adds methods).
- HF-CTX-3's CRUD methods and result shapes.
- HF-CTX-4's fact emission points.
- Every other hyperforge hub's behavior.
- `HyperforgeEvent` wire shape.
- Existing `plans/<EPIC>/*.md` files.

## Acceptance criteria

1. `cargo build --workspace` succeeds in hyperforge.
2. `cargo test --workspace` succeeds. New tests cover:

   | Scenario | Expected |
   |---|---|
   | Fixture: consumer C depends on dep D; `CompatibilityBroken { C, D, ... }` fact at t=100; ticket T completed at t=90. `compat_broken(C, D)` | Returns `Some(T)`. |
   | Same fixture + `CompatibilityRestored { C, D, ... }` at t=110. `compat_broken(C, D)` | Returns `None`. |
   | Fixture with `ArtifactIntroduced(A) @ ticket X`, `ArtifactDeprecated(A) @ ticket Y`, `TouchedPath(path_of_A) @ ticket Z`. `work_on(A)` | Returns `[X, Y, Z]` in `valid_at` order. |
   | `blast_radius(ticket, depth = 1)` on a ticket with two `DependsOn` upstream | Graph contains the ticket + both upstreams as nodes; two `DependsOn` edges. |
   | `blast_radius(ticket, depth = 2)` with a chain A → B → C | Graph contains all three; two `DependsOn` edges. |
   | `zoom(epic, depth = 0)` on a known ticket | Returns `ZoomedView { scope: "TICKET-ID", depth: 0, fact_counts, ..., children: [] }`. |
   | `zoom(epic, depth = 1)` on an epic with 3 tickets | `ZoomedView { scope: "EPIC", depth: 1, children: [3 views for each ticket] }`. |
   | `zoom(epic, depth = 2)` | Returns `unsupported { reason }` (pre-HF-CTX-7). |
   | Fixture with `ArtifactDeprecated(A)` and no `ArtifactRemoved(A)`. `currently_deprecated()` | Returns `[A]`. |
   | Same fixture + `ArtifactRemoved(A)`. `currently_deprecated()` | Returns `[]`. |
   | Fixture with `CompatibilityBroken { package: P, version: V, ... } @ ticket T`. `incompat_observers(P, V)` | Returns `[T]`. |
   | Fixture with `TouchedPath { path: "src/foo.rs", ... } @ [T1, T2]`. `who_touches("src/foo.rs")` | Returns `[T1, T2]`. |

3. The seven queries are callable via synapse. `synapse hyperforge ctx compat-broken <consumer> <dep>` returns well-formed output against a populated DB.
4. `cargo build --workspace` + `cargo test --workspace` pass green end-to-end.

## Completion

- Commit adds `src/ctx/queries.rs` (or the equivalent library-API module), adds the seven RPC methods on the activation, and extends `TicketStore` with the seven library methods + default impls.
- Commit message includes a transcript of at least two synapse calls showing correct output.
- `cargo build --workspace` + `cargo test --workspace` output — both green.
- If public-surface changes warrant a version bump within 4.3.x, bump; else contribute to the existing patch line.
- Ticket status flipped from `Ready` → `Complete` in the same commit as the code.
