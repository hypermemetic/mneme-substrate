---
id: STG-3
title: "Migrate Arbor to ArborStore trait"
status: Pending
type: implementation
blocked_by: [STG-2]
unlocks: [STG-10]
severity: High
target_repo: plexus-substrate
---

## Problem

Arbor currently owns a concrete `ArborStorage` struct with a `SqlitePool` hand-wired in its constructor (`src/activations/arbor/storage.rs`, ~1077 lines). There is no seam for substituting the backend. Migrate Arbor to the pattern established by STG-2: an `ArborStore` trait, a `SqliteArborStore` implementing it (default production path), and an `InMemoryArborStore` (for tests and STG-10's end-to-end integration).

Arbor is the handle backend — it is the shared-storage activation per the technical debt audit. Its stability is critical. The migration must preserve every behavior: handle resolution, refcounts, node walks, all of it.

## Context

Target file set: `src/activations/arbor/` (activation.rs, methods.rs, mod.rs, storage.rs, types.rs, views.rs).

The pattern to follow is pinned in `docs/architecture/<nanotime>_storage-trait-pattern.md` (landed by STG-2) — read the migration checklist first.

**Cross-epic inputs:**

- **ST newtypes** relevant to Arbor: if ST has landed an `ArborId` newtype (pinned as `ArborId(Uuid)` pre-existing per the technical debt audit) and any other arbor-owned identifiers, `ArborStore` method signatures consume those newtypes. If ST has not landed the relevant newtypes at the time this ticket is promoted, the ticket either:
  - Waits until ST catches up (add `blocked_by: ST-<id>` before flipping to Ready), or
  - Uses bare types in the trait and accepts a follow-up refactor after ST lands.

  The decision is the implementor's call at promotion time; document which path was taken in the PR description.

- **`plans/README.md`** pins the trait name `ArborStore` exactly.

## Required behavior

- Extract a public `ArborStore` trait from the current `ArborStorage` concrete struct. Every current `pub` or `pub(crate)` method on `ArborStorage` that is called from outside `storage.rs` becomes a trait method.
- Rename the current `ArborStorage` concrete type to `SqliteArborStore` (or as the STG-2 pattern pins). It is the production default.
- Implement `InMemoryArborStore` backed by in-memory structures (exact shape per the STG-2 pattern — likely `HashMap`s + `Mutex`). Gated per STG-2's gating decision (`test-doubles` feature or equivalent).
- Arbor activation's primary constructor is `Arbor::new(store: Arc<dyn ArborStore>) -> Result<Self, ArborError>`.
- Convenience constructors per STG-2 pattern: `Arbor::with_sqlite(config) -> Result<Self, ArborError>` and `Arbor::with_memory() -> Self` (cfg-gated).
- `builder.rs` updates to call `Arbor::with_sqlite(...)` preserving the pre-epic production path.

## Regression guarantees

| Surface | Post-ticket behavior |
|---|---|
| Arbor Plexus RPC methods | Unchanged — same names, same params, same returns. |
| Arbor on-disk SQLite schema | Unchanged. |
| Handle resolution semantics | Unchanged — same results for same inputs. |
| Refcount behavior | Unchanged. |
| All existing Arbor tests | Pass against `SqliteArborStore`. |
| Performance | Within 10% of pre-epic baseline for any workload already covered by tests. (The overhead of one layer of indirection through a trait object is the expected cost.) |

## Risks

| Risk | Mitigation |
|---|---|
| Arbor has complex cross-method invariants (owner refcounts, node walks) that the in-memory backend fails to preserve. | Port every existing Arbor test to run against both backends. Binary pass — same assertions both sides. |
| Arbor's `storage.rs` contains internal helpers that are not trait methods. | Leave internal helpers on the `SqliteArborStore` impl. Only externally-called methods belong on the trait. |
| Cyclic parent-injection pattern in `builder.rs` interacts with Arbor's construction timing. | `with_sqlite` is synchronous in shape (an `async fn` returning `Result`); the injection pattern is unchanged. |

## What must NOT change

- Any Plexus RPC method on Arbor.
- SQLite schema, migrations, or file path.
- Handle resolution behavior, including every edge case currently tested.
- Refcount semantics.
- Any sibling activation that currently reaches into Arbor (Cone, ClaudeCode, Orcha schema walks — see `docs/architecture/16670380887168786687_substrate-technical-debt-audit.md`). The coupling is DC's problem, not STG's.
- Any file outside `src/activations/arbor/` other than `src/builder.rs` (for the constructor call-site update) and possibly `src/activations/storage.rs` (if new shared helpers are needed).

## Acceptance criteria

1. `cargo build -p plexus-substrate` succeeds.
2. `cargo test -p plexus-substrate` — all pre-epic tests pass.
3. `cargo test -p plexus-substrate --features test-doubles` (or STG-2's gating mechanism) — Arbor's test suite runs against both `SqliteArborStore` and `InMemoryArborStore` with identical assertions, all green.
4. `src/activations/arbor/` exposes `ArborStore` (trait), `SqliteArborStore` (struct impl), and `InMemoryArborStore` (struct impl, gated) as public items.
5. `Arbor::with_sqlite(ArborStorageConfig::default())` produces behavior indistinguishable from pre-epic `Arbor::with_defaults()` — verified by an integration test.
6. `builder.rs`'s default startup produces an identically-configured Arbor (on-disk DB path unchanged).

## Completion

- PR against `plexus-substrate` landing the trait extraction, both backends, updated tests, and `builder.rs` call-site update.
- PR description includes all test commands from acceptance criteria — all green.
- PR description notes which ST newtypes (if any) were threaded through, or explicitly flags any follow-up for post-ST migration.
- Ticket status flipped from `Ready` → `Complete` in the same commit as the code.
