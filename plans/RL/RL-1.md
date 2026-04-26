---
id: RL-1
title: "Resilience — kill load-bearing panics, error swallowing, and missing shutdown paths"
status: Epic
type: epic
blocked_by: []
unlocks: []
target_repo: plexus-substrate
---

## Goal

End state: substrate no longer panics on startup-time storage failures, no longer swallows errors at persistence boundaries, and cleanly propagates cancellation + shutdown from transport → hub → activation. The audit's "Load-bearing panics and error swallowing" section becomes empty against HEAD:

- `src/builder.rs` returns typed startup errors with actionable messages instead of `.expect()` on every storage init.
- Every `panic!` / `unreachable!` in production paths (Orcha `graph_runner`, Orcha `ticket_compiler`, Bash `executor`) is replaced by a typed error variant returned through the activation's error enum.
- Every `let _ = ...` at a persistence or state-resolution boundary becomes a `Result` with logged, structured context (graph id, session id, approval id — using ST newtypes when they land).
- Every `tokio::spawn` either runs under a `CancellationToken` (wired per RL-9) or lives on the owning activation's task set, and is reaped on shutdown.
- A SIGINT / SIGTERM handler drains in-flight work, checkpoints Orcha graphs, closes storage pools, and exits cleanly without leaked tasks.

This epic fixes the audit's "Load-bearing panics and error swallowing" inventory and the "Cancellation is inconsistent" / "No graceful shutdown" gaps. It does not add metrics, config loading, or pagination — those are OB's scope.

## Context

**Cancellation-token shape is the pinned cross-epic unknown.** The audit flags that only Orcha has ad-hoc cancellation (a `cancel_registry` on watch channels); Echo, Cone, ClaudeCode streams ignore client disconnects. RL-9 and RL-10 need a unified mechanism, but the exact shape — whether the hub owns a `CancellationToken` per request, how it threads through `PlexusStreamItem` poll loops, and how activations observe it — is not yet decided. **RL-S01 is the spike that decides.** Its pass condition is binary: a cancelled request terminates the activation's work within N ms (pinned in the spike). The passing spike's approach becomes the implementation in RL-9.

**ST and STG relationships.** RL's error enums gain structured context fields. Those fields should use ST's newtypes (`GraphId`, `SessionId`, `ApprovalId`, `StreamId`, `ToolUseId`) once ST lands. If ST has not shipped when RL implementation starts, RL-5..7 use the bare types present in HEAD and are updated in ST's per-activation migrations — no blocking dependency. STG's storage traits (`ArborStore`, `OrchaStore`, `LatticeStore`, etc.) will carry the structured-context error types RL introduces for persistence failures; the concrete error *shape* is RL's contribution, the *trait* is STG's.

**Audit drift note.** The technical-debt audit (2026-04-16) lists specific file:line references. These may drift. Every implementation ticket (RL-2..10) re-verifies its specific sites against HEAD before proceeding. The **categories** of debt are durable; the line numbers are not.

**Scope of "error swallowing."** Any `let _ = ...` or `.ok()` / `.ok()?` / `.await.ok().flatten()` on a fallible operation at a persistence, state-resolution, or cross-activation boundary. Swallowing in pure-local paths (e.g., a best-effort log write inside a single function with no downstream consumer) is not in scope.

## Dependency DAG

```
                       RL-S01 (spike: cancellation propagation)
                         │
                         ▼
                       RL-9 (cancellation end-to-end)
                         ▲
                         │
                       RL-8 (task lifecycle cleanup) ─┐
                         ▲                             │
                         │                             ▼
                       RL-10 (graceful shutdown) ◄─────┘

   (parallel, file-disjoint, independent of the spike)

     RL-2         RL-3         RL-4         RL-5         RL-6         RL-7
   (builder    (Orcha        (Bash        (Orcha pm   (Loopback    (misc
   expects)    panics)       panic)       save swallow) resolve_    swallows:
                                                        approval)   mcp, lattice,
                                                                    loopback reads,
                                                                    changelog,
                                                                    bash stderr)
```

- **RL-S01** gates RL-9 and RL-10. Its binary pass/fail picks the cancellation mechanism.
- **RL-2 through RL-7** run in parallel. Each owns disjoint file scope (see per-ticket "What must NOT change" and the file-boundary table below). They do not depend on the spike and do not block each other.
- **RL-8** (task lifecycle cleanup) runs in parallel with RL-2..7. It touches `plugin_system/conversion.rs` and `health/activation.rs` and the owning activations' task sets. It does *not* depend on the spike — the fix is "every spawn has an owner" — but its tasks will later be cancelled via RL-9's mechanism.
- **RL-9** lands after RL-S01 resolves. It wires the chosen mechanism from transport → hub → activation.
- **RL-10** lands after RL-8 and RL-9. It uses RL-9's cancellation to drain work and RL-8's task ownership to reap tasks on shutdown.

**File-boundary check.**

| Ticket | Files touched |
|---|---|
| RL-2 | `src/builder.rs`, new `src/error.rs` (or equivalent) |
| RL-3 | `orcha/graph_runner.rs`, `orcha/ticket_compiler.rs`, `orcha/error.rs` |
| RL-4 | `bash/executor/mod.rs`, `bash/error.rs` |
| RL-5 | `orcha/activation.rs`, `orcha/error.rs` |
| RL-6 | `claudecode_loopback/activation.rs`, `claudecode_loopback/error.rs` |
| RL-7 | `mcp/mcp_session.rs`, `lattice/storage.rs`, `claudecode_loopback/storage.rs`, `changelog/activation.rs`, `bash/executor/mod.rs` (stderr only) |
| RL-8 | `plugin_system/conversion.rs`, `health/activation.rs`, plus per-activation task sets as needed |
| RL-9 | Transport integration (cllient edge), `hub-core` (if outside substrate), `src/builder.rs`, every activation's method entry points |
| RL-10 | `src/main.rs` / `src/bin/*.rs`, `src/builder.rs`, every activation's `Drop` or `shutdown` hook |

- **RL-3 and RL-5 both touch `orcha/error.rs`.** File-collision concurrent — land RL-3 first (it introduces the variant shape), then RL-5 adds the persistence-failure variants.
- **RL-4 and RL-7 both touch `bash/executor/mod.rs`.** File-collision concurrent — land RL-4 first (panic replacement is a bigger surface), then RL-7 (stderr truncation) against the resulting file.
- **RL-2 and RL-9 both touch `src/builder.rs`.** RL-2 first (replace `.expect()`), RL-9 second (wire cancellation into startup paths).

## Phase Breakdown

| Phase | Tickets | Notes |
|---|---|---|
| 0. Decision | RL-S01 | Binary spike: cancellation-token propagation mechanism. Result pins RL-9 and RL-10 shape. |
| 1. Kill panics + swallowing (parallel) | RL-2, RL-3, RL-4, RL-5, RL-6, RL-7, RL-8 | Seven tickets. File-disjoint per the boundary table (with the two serialization rules noted above). Each targets a specific audit-flagged category. |
| 2. Wire cancellation | RL-9 | Depends on RL-S01. Threads the chosen mechanism through transport → hub → activation. |
| 3. Graceful shutdown | RL-10 | Depends on RL-8 and RL-9. Drains in-flight work on SIGINT / SIGTERM. |

## Tickets

| ID | Summary | Status |
|---|---|---|
| RL-1 | This epic overview | Epic |
| RL-S01 | Spike: cancellation-token propagation mechanism through hub and stream loops | Pending |
| RL-2 | Replace `src/builder.rs` `.expect()` chains with typed startup errors | Pending |
| RL-3 | Replace load-bearing panics in Orcha (`graph_runner::unreachable!`, `ticket_compiler` × 3) | Pending |
| RL-4 | Replace load-bearing panic in Bash executor (`panic!("Expected stdout/exit")`) | Pending |
| RL-5 | Fix Orcha `pm.save_*` error swallowing (7 sites in `orcha/activation.rs`) | Pending |
| RL-6 | Fix Loopback approval-resolution error swallowing (`resolve_approval`) | Pending |
| RL-7 | Fix remaining error swallowing (mcp_session, lattice migrations, loopback reads, changelog, bash stderr) | Pending |
| RL-8 | Task lifecycle cleanup (no orphan `tokio::spawn`; every task has an owner) | Pending |
| RL-9 | Cancellation token end-to-end (transport → hub → activation) | Pending |
| RL-10 | Graceful shutdown (SIGINT / SIGTERM drain, checkpoint, close) | Pending |

## Out of scope

- **Config file / TOML loader.** OB's scope.
- **Metrics / Prometheus / OpenTelemetry counters.** OB's scope. RL adds structured error context via `tracing` events, but no new metric surface.
- **Pagination.** OB's scope.
- **Streaming protocol versioning.** OB's scope. RL is type-safety and lifecycle hygiene, not wire-format evolution.
- **Strong-typed IDs.** ST's scope. RL consumes ST's newtypes *if they exist at execution time*; if not, RL uses bare types and ST updates signatures later.
- **Storage trait abstraction.** STG's scope. RL does not move any activation from concrete SQLite to a trait; it only hardens error propagation against the current concrete storage.
- **Activation decoupling.** DC's scope. RL does not untangle cross-activation imports.
- **New cancellation semantics beyond "stop this work".** RL-S01 picks a mechanism whose binary pass condition is "a cancelled request terminates the activation's work within N ms". It does *not* introduce partial cancellation, priority cancellation, or resumable cancellation.
- **Property tests / fuzzing.** Each ticket adds targeted unit or integration tests where mechanical verification requires them. Adding a proptest suite is out of scope.

## Cross-epic references

- **Audit document** (`docs/architecture/16670380887168786687_substrate-technical-debt-audit.md`). Section "Load-bearing panics and error swallowing" is RL's requirements inventory. Sections "Cancellation is inconsistent" and "No graceful shutdown" scope RL-8..10.
- **ST epic.** ST's newtypes (`SessionId`, `GraphId`, `NodeId`, `StreamId`, `ApprovalId`, `ToolUseId`, `TicketId`, `WorkingDir`, `ModelId`) appear in RL's structured error variants when ST has shipped. Example: `OrchaError::GraphNotFound(GraphId)` instead of `OrchaError::GraphNotFound(String)`. If ST has not shipped, RL uses bare types and ST later migrates.
- **STG epic.** STG's per-activation storage traits (`ArborStore`, `OrchaStore`, `LatticeStore`, etc.) carry the structured error types RL introduces. The error *shape* is RL's contribution; the *trait signature* is STG's.
- **README pinned decision.** RL-S01's cancellation-token mechanism resolution pins a new row in README's cross-epic contracts table (`CancellationToken`, owner = RL). Update README in the same commit that lands RL-S01's resolution.
- **DC epic.** RL's error-enum changes (e.g., `OrchaError::PersistenceFailed { ... }`) become part of Orcha's library-API surface once DC-2 lands. If DC lands first, RL's error variants are added to the already-narrow API. If RL lands first, DC's DC-2 re-exports whatever RL has pinned.

## What must NOT change

- Wire-level RPC behavior. Every `#[plexus_macros::method]` continues to serve the same request/response shape. RL may add new error-response variants, but existing success responses are unchanged.
- SQLite-per-activation layout. RL does not touch `~/.plexus/substrate/activations/{name}/` paths, migrations, or schema.
- Activation namespace strings, method names, schema hashes.
- Existing `cargo test` pass rate. All currently-passing tests pass after every RL ticket lands. New tests may be added; none are removed.
- Orcha's `cancel_registry` semantics during the window between RL-S01 and RL-9. The ad-hoc registry keeps working until RL-9 replaces it; RL-9's integration preserves the observable behavior of existing cancellation points.
- Startup ordering of activations in `builder.rs`. Cyclic-parent injection via `OnceLock<Weak<DynamicHub>>` is preserved. RL-2 changes the *failure mode* (typed error instead of panic), not the *order*.

## Completion

Epic is Complete when RL-S01 is Complete or Superseded, RL-2 through RL-10 are all Complete, and the audit's "Load-bearing panics and error swallowing" section is empty against HEAD (verified by re-running the audit's categories against the tree). Deliverables:

- Zero `.expect()` in `src/builder.rs` storage init paths (grep check).
- Zero `panic!` / `unreachable!` in `orcha/graph_runner.rs`, `orcha/ticket_compiler.rs`, `bash/executor/mod.rs` (grep check).
- Zero `let _ = pm.save_*` / `let _ = storage.resolve_approval` in `orcha/activation.rs` and `claudecode_loopback/activation.rs` (grep check).
- Zero `.ok()?` / `.await.ok().flatten()` at the audit-flagged sites (grep check per ticket).
- Every `tokio::spawn` in the substrate source tree is reachable from an owner: either a `CancellationToken` guard or a task set on a specific activation (mechanical check per RL-8's acceptance criteria).
- A SIGINT handler exists at the binary entry point; a manual integration test (ticket RL-10) shows that sending SIGINT to a running substrate with an in-flight Orcha graph checkpoints the graph and exits within a bounded window.
- README's "Open coordination questions" gains a resolved entry for cancellation-token mechanism (from RL-S01).
