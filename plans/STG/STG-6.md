---
id: STG-6
title: "Migrate ClaudeCode to ClaudeCodeStore trait"
status: Pending
type: implementation
blocked_by: [STG-2]
unlocks: [STG-10]
severity: High
target_repo: plexus-substrate
---

## Problem

ClaudeCode currently owns a concrete `ClaudeCodeStorage` struct with a `SqlitePool` hand-wired in its constructor (`src/activations/claudecode/storage.rs`, ~1487 lines — the largest storage surface in substrate). It also has a sibling `claudecode_loopback` activation with its own storage (`src/activations/claudecode_loopback/storage.rs`, ~328 lines). Migrate both to the pattern established by STG-2: a `ClaudeCodeStore` trait for the main activation and a `LoopbackStore` (or similar) for the sibling, each with SQLite and in-memory backends.

ClaudeCode owns the streaming buffer (`streams: RwLock<HashMap<StreamId, ActiveStreamBuffer>>` per the technical debt audit — this is in-memory already but is part of the activation state, not the `Storage` struct; it stays where it is).

## Context

Target file set: `src/activations/claudecode/` (activation.rs, executor.rs, mod.rs, render.rs, sessions.rs, storage.rs, types.rs) plus `src/activations/claudecode_loopback/` (activation.rs, storage.rs, etc.).

Pattern pinned in `docs/architecture/<nanotime>_storage-trait-pattern.md` (STG-2). Read the migration checklist first.

**Cross-epic inputs:**

- **ST newtypes** relevant to ClaudeCode: `SessionId`, `StreamId`, `ToolUseId`, `ApprovalId`, `ModelId`. Loopback already has `ApprovalId(Uuid)` per the technical debt audit — ST's `ApprovalId` canonicalizes on that.
- **Technical debt audit** notes the streams map leak (no eviction on client disconnect) — that's RL's problem. Preserve current behavior; do not try to fix here.
- **`plans/README.md`** pins `ClaudeCodeStore` exactly. Loopback's trait name is NOT pinned in README — this ticket chooses it (`LoopbackStore` is the natural name). Add the chosen name to the README cross-epic contracts section in the same PR.

## Required behavior

- Extract `ClaudeCodeStore` trait from `ClaudeCodeStorage`.
- Extract a `LoopbackStore` (or chosen name) trait from `LoopbackStorage` in `claudecode_loopback`.
- Both activations get: SQLite default impl (`SqliteClaudeCodeStore`, `SqliteLoopbackStore`), in-memory impl gated per STG-2.
- Primary constructors: `ClaudeCode::new(store: Arc<dyn ClaudeCodeStore>, ...) -> Result<Self, Error>` and analogous for Loopback.
- Convenience constructors per STG-2 pattern.
- `builder.rs` updated to preserve pre-epic production behavior for both.

## Regression guarantees

| Surface | Post-ticket behavior |
|---|---|
| ClaudeCode + Loopback Plexus RPC methods | Unchanged. |
| On-disk SQLite schemas | Unchanged. |
| Session lifecycle | Unchanged. |
| Streaming buffer behavior (in-memory, separate concern) | Unchanged. |
| Approval resolution semantics | Unchanged. |
| All existing ClaudeCode and Loopback tests | Pass against SQLite backends. |

## Risks

| Risk | Mitigation |
|---|---|
| 1487 lines is the largest storage surface; trait method count may be large. | Acceptable — the point of the trait is shape, not method count. If the trait exceeds ~30 methods, consider splitting along natural boundaries (sessions vs. messages vs. tool-use records) but document the split. |
| Orcha's direct reach into Loopback storage (technical debt audit) means Orcha may break when Loopback's storage type changes. | That coupling is DC's problem. For this ticket, preserve Loopback's existing `pub` surface where Orcha consumes it. Flag in the PR any new DC-visible coupling introduced. |
| Streams map (RwLock HashMap) is activation state, not storage — confusing boundary. | Do not touch it. It stays on the activation struct, not on the store trait. Document the boundary clearly in the PR. |

## What must NOT change

- Any Plexus RPC method on ClaudeCode or Loopback.
- SQLite schemas or file paths.
- Session, stream, or approval semantics.
- Any file outside `src/activations/claudecode/` and `src/activations/claudecode_loopback/` other than `src/builder.rs` and possibly `src/activations/storage.rs`.

## Acceptance criteria

1. `cargo build -p plexus-substrate` succeeds.
2. `cargo test -p plexus-substrate` — all pre-epic tests pass.
3. `cargo test -p plexus-substrate --features test-doubles` — ClaudeCode and Loopback test suites run against both backends with identical assertions, all green.
4. `src/activations/claudecode/` exposes `ClaudeCodeStore`, `SqliteClaudeCodeStore`, `InMemoryClaudeCodeStore`. `src/activations/claudecode_loopback/` exposes the analogous three items.
5. `ClaudeCode::with_sqlite(...)` and `Loopback::with_sqlite(...)` produce behavior indistinguishable from pre-epic — verified by an integration test exercising at least one session with a tool-use and an approval round trip.
6. `builder.rs`'s default startup produces identical on-disk state.
7. `plans/README.md`'s trait-surfaces table is updated with the Loopback trait name chosen by this ticket.

## Completion

- PR against `plexus-substrate` landing both trait extractions, four backends, updated tests, `builder.rs` call-site updates, and the `plans/README.md` update.
- PR description includes all test commands, the chosen Loopback trait name, and ST newtype integration status.
- Ticket status flipped from `Ready` → `Complete` in the same commit as the code.
