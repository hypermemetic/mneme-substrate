---
id: STG-4
title: "Migrate Orcha to OrchaStore trait"
status: Pending
type: implementation
blocked_by: [STG-2]
unlocks: [STG-10]
severity: High
target_repo: plexus-substrate
---

## Problem

Orcha currently owns a concrete `OrchaStorage` struct with a `SqlitePool` hand-wired in its constructor (`src/activations/orcha/storage.rs`, ~664 lines). Orcha also has a separate `pm` submodule with its own persistence. There is no seam for substituting either backend. Migrate Orcha to the pattern established by STG-2: an `OrchaStore` trait, a `SqliteOrchaStore` (default production path), and an `InMemoryOrchaStore` (for tests and STG-10's end-to-end integration).

The `pm` submodule's storage is also migrated here — it is logically part of Orcha's persistence. Either (a) fold pm methods into `OrchaStore`, or (b) add a sibling `OrchaPmStore` trait with its own SQLite/in-memory pair. The implementor chooses at implementation time based on how coupled the two stores are in the current code; document the choice.

## Context

Target file set: `src/activations/orcha/` (activation.rs, context.rs, graph_runner.rs, graph_runtime.rs, mod.rs, orchestrator.rs, pm/, storage.rs, tests.rs, ticket_compiler.rs, types.rs).

Pattern pinned in `docs/architecture/<nanotime>_storage-trait-pattern.md` (STG-2). Read the migration checklist first.

**Cross-epic inputs:**

- **ST newtypes** relevant to Orcha: `SessionId`, `GraphId`, `ApprovalId`, `ToolUseId`, `TicketId`, `WorkingDir`, `ModelId`. Where ST has landed the newtype, `OrchaStore` method signatures use it. Where ST has not landed the newtype, the ticket either waits on ST or accepts a bare-type interim per STG-3's precedent.

- **Technical debt audit** (`docs/architecture/16670380887168786687_substrate-technical-debt-audit.md`) notes Orcha reaches into `LoopbackStorage` directly in `graph_runner.rs`. That coupling is DC's problem, not STG's. Leave it alone here — migrate only Orcha's own storage.

- **`plans/README.md`** pins the trait name `OrchaStore` exactly.

## Required behavior

- Extract a public `OrchaStore` trait from the current `OrchaStorage` concrete struct.
- Decide on the `pm` submodule: fold into `OrchaStore` OR add a separate `OrchaPmStore` trait (both trait names are already used per convention; document the decision in the PR).
- `SqliteOrchaStore` is the production default.
- `InMemoryOrchaStore` gated per STG-2's mechanism.
- Primary constructor: `Orcha::new(store: Arc<dyn OrchaStore>, /* pm store if separate */) -> Result<Self, OrchaError>`.
- Convenience constructors per STG-2 pattern.
- `builder.rs` updates to preserve pre-epic production behavior.

## Regression guarantees

| Surface | Post-ticket behavior |
|---|---|
| Orcha Plexus RPC methods | Unchanged. |
| Orcha on-disk SQLite schemas (main + pm) | Unchanged. |
| Graph execution semantics | Unchanged. |
| Ticket persistence semantics | Unchanged. |
| All existing Orcha tests (including `tests.rs`) | Pass against `SqliteOrchaStore`. |

## Risks

| Risk | Mitigation |
|---|---|
| Orcha ↔ Loopback coupling requires in-memory Loopback too. | STG-10 owns end-to-end in-memory. This ticket's in-memory backend can rely on real Loopback for cross-activation scenarios, or stub Loopback via its STG-migrated trait once STG-6 lands. Document which path is used. |
| `pm` submodule storage is deeply coupled to the main storage (shared transactions, etc.). | Fold into one trait if tangled. Separate traits if not. |
| `let _ = pm.save_*()` error-swallowing sites (per technical debt audit) obscure whether tests exercise failure paths. | Out of scope — that's RL epic. Preserve the current behavior exactly. |

## What must NOT change

- Any Plexus RPC method on Orcha.
- SQLite schemas or file paths.
- Graph or ticket persistence behavior.
- Error-handling behavior (including the `let _ =` sites — those stay as-is, fixed in RL epic).
- Any file outside `src/activations/orcha/` other than `src/builder.rs` and possibly `src/activations/storage.rs`.

## Acceptance criteria

1. `cargo build -p plexus-substrate` succeeds.
2. `cargo test -p plexus-substrate` — all pre-epic tests pass.
3. `cargo test -p plexus-substrate --features test-doubles` — Orcha's test suite (including `tests.rs`) runs against both `SqliteOrchaStore` and `InMemoryOrchaStore` with identical assertions, all green.
4. `src/activations/orcha/` exposes `OrchaStore` (trait), `SqliteOrchaStore`, and `InMemoryOrchaStore` as public items. If `OrchaPmStore` is separate, it and its two impls are also public.
5. `Orcha::with_sqlite(...)` produces behavior indistinguishable from pre-epic `Orcha` construction — verified by an integration test that runs at least one graph end-to-end.
6. `builder.rs`'s default startup produces identical on-disk state for Orcha's databases.

## Completion

- PR against `plexus-substrate` landing the trait extraction(s), both (or four) backends, updated tests, and `builder.rs` call-site update.
- PR description includes all test commands and the pm-fold-or-separate decision with reasoning.
- PR description notes ST newtype integration status.
- Ticket status flipped from `Ready` → `Complete` in the same commit as the code.
