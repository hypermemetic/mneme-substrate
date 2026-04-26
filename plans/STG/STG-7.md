---
id: STG-7
title: "Migrate Cone to ConeStore trait"
status: Pending
type: implementation
blocked_by: [STG-2]
unlocks: [STG-10]
severity: Medium
target_repo: plexus-substrate
---

## Problem

Cone currently owns a concrete `ConeStorage` struct with a `SqlitePool` hand-wired in its constructor (`src/activations/cone/storage.rs`, ~664 lines). There is no seam for substituting the backend. Migrate Cone to the pattern established by STG-2: a `ConeStore` trait, a `SqliteConeStore` (default production path), and an `InMemoryConeStore` (for tests and STG-10's end-to-end integration).

## Context

Target file set: `src/activations/cone/` (activation.rs, methods.rs, mod.rs, storage.rs, tests.rs, types.rs).

Pattern pinned in `docs/architecture/<nanotime>_storage-trait-pattern.md` (STG-2). Read the migration checklist first.

**Cross-epic inputs:**

- **ST newtypes** relevant to Cone: `ModelId`. Possibly others depending on what Cone's store tracks.
- **Technical debt audit** notes Cone hardcodes `use crate::activations::bash::Bash` and walks Arbor's schema — those are DC's problem. Preserve current behavior here.
- **`plans/README.md`** pins `ConeStore` exactly.

## Required behavior

- Extract a public `ConeStore` trait from the current `ConeStorage` concrete struct.
- `SqliteConeStore` is the production default.
- `InMemoryConeStore` gated per STG-2's mechanism.
- Primary constructor: `Cone::new(store: Arc<dyn ConeStore>) -> Result<Self, ConeError>`.
- Convenience constructors per STG-2 pattern.
- `builder.rs` updates to preserve pre-epic production behavior.

## Regression guarantees

| Surface | Post-ticket behavior |
|---|---|
| Cone Plexus RPC methods | Unchanged. |
| Cone on-disk SQLite schema | Unchanged. |
| Model catalog semantics | Unchanged. |
| All existing Cone tests (including `tests.rs`) | Pass against `SqliteConeStore`. |

## Risks

| Risk | Mitigation |
|---|---|
| Cone's tests construct a concrete Bash instance (per technical debt audit). | Keep that pattern — DC's problem, not STG's. Just ensure STG migration doesn't make it worse. |
| In-memory backend semantic drift from SQLite. | Port every test to both backends; binary pass. |

## What must NOT change

- Any Plexus RPC method on Cone.
- SQLite schema or file path.
- Model catalog semantics.
- Cone's coupling to Bash or Arbor (that's DC's problem).
- Any file outside `src/activations/cone/` other than `src/builder.rs` and possibly `src/activations/storage.rs`.

## Acceptance criteria

1. `cargo build -p plexus-substrate` succeeds.
2. `cargo test -p plexus-substrate` — all pre-epic tests pass.
3. `cargo test -p plexus-substrate --features test-doubles` — Cone's test suite runs against both backends with identical assertions, all green.
4. `src/activations/cone/` exposes `ConeStore`, `SqliteConeStore`, `InMemoryConeStore` as public items.
5. `Cone::with_sqlite(...)` produces behavior indistinguishable from pre-epic construction — verified by an integration test that exercises the model catalog.
6. `builder.rs`'s default startup produces identical on-disk state.

## Completion

- PR against `plexus-substrate` landing the trait extraction, both backends, updated tests, and `builder.rs` call-site update.
- PR description includes all test commands and ST newtype integration status.
- Ticket status flipped from `Ready` → `Complete` in the same commit as the code.
