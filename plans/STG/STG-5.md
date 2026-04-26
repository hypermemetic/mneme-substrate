---
id: STG-5
title: "Migrate Lattice to LatticeStore trait"
status: Pending
type: implementation
blocked_by: [STG-2]
unlocks: [STG-10]
severity: High
target_repo: plexus-substrate
---

## Problem

Lattice currently owns a concrete `LatticeStorage` struct with a `SqlitePool` hand-wired in its constructor (`src/activations/lattice/storage.rs`, ~1134 lines). There is no seam for substituting the backend. Migrate Lattice to the pattern established by STG-2: a `LatticeStore` trait, a `SqliteLatticeStore` (default production path), and an `InMemoryLatticeStore` (for tests and STG-10's end-to-end integration).

Lattice is the second-largest storage surface in substrate after ClaudeCode. Its DAG-tracking logic is intricate — the migration must preserve every semantic around node readiness, edge updates, and graph status transitions.

## Context

Target file set: `src/activations/lattice/` (activation.rs, mod.rs, storage.rs, types.rs).

Pattern pinned in `docs/architecture/<nanotime>_storage-trait-pattern.md` (STG-2). Read the migration checklist first.

**Cross-epic inputs:**

- **ST newtypes** relevant to Lattice: `GraphId`, `NodeId`. Where ST has landed them, `LatticeStore` signatures use them; otherwise follow STG-3's precedent.
- **Technical debt audit** flags `let _ = sqlx::query("ALTER TABLE ... ADD COLUMN ...")` error swallowing in Lattice schema migrations. That is RL's problem. Preserve current behavior; do not attempt to fix here.
- **`plans/README.md`** pins `LatticeStore` exactly.

## Required behavior

- Extract a public `LatticeStore` trait from the current `LatticeStorage` concrete struct.
- `SqliteLatticeStore` is the production default.
- `InMemoryLatticeStore` gated per STG-2's mechanism.
- Primary constructor: `Lattice::new(store: Arc<dyn LatticeStore>) -> Result<Self, LatticeError>`.
- Convenience constructors per STG-2 pattern.
- `builder.rs` updates to preserve pre-epic production behavior.

## Regression guarantees

| Surface | Post-ticket behavior |
|---|---|
| Lattice Plexus RPC methods | Unchanged. |
| Lattice on-disk SQLite schema | Unchanged. |
| DAG semantics (node ready events, edge updates, cycle detection if any) | Unchanged. |
| Graph status transitions | Unchanged. |
| All existing Lattice tests | Pass against `SqliteLatticeStore`. |

## Risks

| Risk | Mitigation |
|---|---|
| In-memory backend fails to reproduce SQLite's ordering guarantees for ready-node queries. | Port every Lattice test to both backends. Tighten the trait contract's docstring to specify ordering explicitly if tests depend on it. |
| Lattice's schema has had historical `ALTER TABLE` migrations whose current-version semantics are subtle. | In-memory backend matches the final (current) schema shape. Do not try to model historical migrations. |

## What must NOT change

- Any Plexus RPC method on Lattice.
- SQLite schema or file path.
- DAG semantics.
- Any file outside `src/activations/lattice/` other than `src/builder.rs` and possibly `src/activations/storage.rs`.

## Acceptance criteria

1. `cargo build -p plexus-substrate` succeeds.
2. `cargo test -p plexus-substrate` — all pre-epic tests pass.
3. `cargo test -p plexus-substrate --features test-doubles` — Lattice's test suite runs against both backends with identical assertions, all green.
4. `src/activations/lattice/` exposes `LatticeStore`, `SqliteLatticeStore`, `InMemoryLatticeStore` as public items.
5. `Lattice::with_sqlite(...)` produces behavior indistinguishable from pre-epic construction — verified by an integration test that exercises at least one DAG lifecycle (create graph, mark nodes ready, transition to complete).
6. `builder.rs`'s default startup produces identical on-disk state for Lattice's database.

## Completion

- PR against `plexus-substrate` landing the trait extraction, both backends, updated tests, and `builder.rs` call-site update.
- PR description includes all test commands and ST newtype integration status.
- Ticket status flipped from `Ready` → `Complete` in the same commit as the code.
