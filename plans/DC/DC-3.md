---
id: DC-3
title: "Decouple Orcha from LoopbackStorage via LoopbackClient"
status: Pending
type: implementation
blocked_by: [DC-2]
unlocks: [DC-7]
severity: High
target_repo: plexus-substrate
---

## Problem

Orcha reaches directly into Loopback's storage layer. `orcha/graph_runner.rs` imports `LoopbackStorage` (a storage struct that owns the SQLite connection pool for approvals), holds an `Arc<LoopbackStorage>` on multiple runner structs, and queries approval state as if Orcha owned the approval table. Loopback's storage layer is the single place that should know about the approval schema, the pool, the migrations, and the row shapes — but because `LoopbackStorage` is `pub` and re-exported from `claudecode_loopback/mod.rs`, Orcha has grown a reach-in that couples it to Loopback's implementation.

A schema change in Loopback's approval table (e.g., adding a column, renaming a row type, swapping the pool implementation) breaks Orcha. This violates the library-API convention pinned in DC-2: storage is internal.

## Context

**The specific coupling (re-verify against HEAD before starting; audit drift caveat applies):**

- `src/activations/orcha/graph_runner.rs:3` — `use crate::activations::claudecode_loopback::LoopbackStorage;`
- `src/activations/orcha/graph_runner.rs:33, 179, 233, 297, 528, 583, 627, 840, 893, 960` — `loopback_storage: Arc<LoopbackStorage>` field on multiple runner structs.
- `src/activations/orcha/graph_runner.rs:269` — `use crate::activations::claudecode_loopback::ApprovalStatus;` (this is domain type — acceptable in library API).
- `src/activations/orcha/activation.rs:2411` — `loopback_storage: Arc<crate::activations::claudecode_loopback::LoopbackStorage>`.
- `src/activations/claudecode_loopback/mod.rs:6` — `pub use storage::{LoopbackStorage, LoopbackStorageConfig};` — this re-export is the leak DC-3 closes.

**What Orcha actually does with `LoopbackStorage`.** Orcha queries approval status, resolves approvals, and waits on approval outcomes during graph execution. All of these are library-level operations that could live behind a curated API on Loopback's activation struct itself. `LoopbackStorage`'s storage methods are what Orcha calls; the library-API alternative is a client handle (`LoopbackClient`) that exposes **just the approval operations Orcha legitimately needs** and keeps the storage pool, schema, and row types hidden.

**Library-API shape pinned by DC-2.** Loopback's entry point re-exports: the activation struct, constructor, `LoopbackError`, approval domain types (`ApprovalId`, `ApprovalStatus`, etc.). It does NOT re-export `LoopbackStorage`.

**`LoopbackClient` shape (required behavior below).** A cheap handle (likely `#[derive(Clone)]`, holds an `Arc` internally) obtained from the Loopback activation struct via a method like `fn client(&self) -> LoopbackClient`. Exposes just the approval operations Orcha needs: query approval, resolve approval, wait/subscribe for approval outcome. Each method returns domain types (`ApprovalId`, `ApprovalStatus`, etc.) — not storage rows.

## Required behavior

**Loopback side:**

| Operation | Current shape (via LoopbackStorage) | New shape (via LoopbackClient) |
|---|---|---|
| Read approval status by ID | `storage.get_approval(id)` returning a row type | `client.approval_status(id)` returning `Option<ApprovalStatus>` or `Option<ApprovalRecord>` (domain type) |
| Resolve approval (approve/deny) | `storage.resolve_approval(id, status)` | `client.resolve_approval(id, decision)` |
| Wait for approval outcome | direct storage query in a poll loop | `client.wait_for_approval(id)` returning a future or stream of the outcome |
| List approvals for a session | direct storage query | `client.list_approvals(session_id)` |

The exact set of operations is determined by what `orcha/graph_runner.rs` actually calls against `LoopbackStorage` — the implementor enumerates those call sites during implementation and maps each to a `LoopbackClient` method.

**Orcha side:**

| Before | After |
|---|---|
| `use crate::activations::claudecode_loopback::LoopbackStorage;` | `use crate::activations::claudecode_loopback::LoopbackClient;` |
| `loopback_storage: Arc<LoopbackStorage>` on runner structs | `loopback_client: LoopbackClient` (or `Arc<LoopbackClient>` — implementor picks based on Clone cost) |
| `self.loopback_storage.<method>(...)` | `self.loopback_client.<method>(...)` |

**Loopback's `mod.rs` after DC-3:**
- `pub use` removes `LoopbackStorage` and `LoopbackStorageConfig`.
- `pub use` adds `LoopbackClient` (new type).
- `LoopbackStorage` and `LoopbackStorageConfig` become `pub(crate)` or private.

## Risks

- **`LoopbackClient` creation pattern.** The handle needs access to the underlying `Arc<LoopbackStorage>`. If `LoopbackClient` wraps an `Arc<LoopbackStorage>` and is constructed by the Loopback activation struct, that's fine — the storage type stays internal. Risk: if `LoopbackClient` ends up with lifetime parameters that leak the internal type, the abstraction fails. **Mitigation:** require `LoopbackClient` to be `'static` (no lifetime parameters) and opaque (no method that returns a reference to internal storage).
- **Method count explosion.** If Orcha calls ten distinct storage methods, `LoopbackClient` gets ten methods, each of which is a thin wrapper. That's acceptable — curation is the point — but review each for whether Orcha's usage pattern can be expressed as a smaller higher-level operation (e.g., `wait_for_approval` instead of "poll + query + sleep"). Prefer higher-level operations where they match how Orcha already uses the API.
- **Concurrent DC-4.** DC-4 also modifies `orcha/graph_runner.rs` and `orcha/activation.rs`. DC-3 and DC-4 cannot land in parallel — file collision. Implementor checks HEAD and coordinates with DC-4 owner before starting. Recommended: DC-3 lands first (smaller diff, one coupling site), then DC-4.

## What must NOT change

- Loopback's wire-level RPC methods — request/response shapes identical.
- Loopback's SQLite schema, migrations, pool configuration.
- Orcha's graph-execution semantics — approval gates still block graph advance, still resolve on approval/denial, still time out the same way.
- Orcha's `#[plexus_macros::method]` surface — Orcha's wire API unchanged.
- `LoopbackStorage`'s internal API — DC-3 does not refactor storage methods; it hides the struct and puts a client in front.

## Acceptance criteria

1. `grep -rn "use crate::activations::claudecode_loopback::LoopbackStorage" src/activations/orcha/` returns zero results.
2. `grep -rn "use crate::activations::claudecode_loopback::LoopbackStorage" src/activations/` returns zero results **outside** `src/activations/claudecode_loopback/**`.
3. `claudecode_loopback/mod.rs` no longer contains `pub use storage::{LoopbackStorage, LoopbackStorageConfig};`. Instead, it re-exports `LoopbackClient` (and optionally `LoopbackClient`'s relevant result / error types).
4. Orcha holds `LoopbackClient` (or `Arc<LoopbackClient>`) on every runner struct that previously held `Arc<LoopbackStorage>`.
5. `cargo test --workspace` passes with zero test failures.
6. Orcha's existing approval-gating integration behavior (graph waits on approval, resumes on approve, fails on deny, times out on no-response) is unchanged — verified by whichever Orcha test already covers this, re-run and green.
7. A `cargo doc` pass shows `LoopbackStorage` and `LoopbackStorageConfig` no longer appear as `pub` items in Loopback's crate docs.

## Completion

Implementor delivers:

- Commit introducing `LoopbackClient` in `claudecode_loopback/`, with the method set matched to Orcha's actual call sites.
- Commit migrating Orcha's runners to use `LoopbackClient`, removing `LoopbackStorage` imports.
- Commit demoting `LoopbackStorage` and `LoopbackStorageConfig` to `pub(crate)` and removing their re-export from `mod.rs`.
- `cargo test` output showing green.
- Before/after `grep` output for the import-leak criteria.
- Status flip to `Complete` in the commit that lands the work.
