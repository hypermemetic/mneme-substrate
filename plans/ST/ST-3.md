---
id: ST-3
title: "Migrate Arbor to use typed IDs"
status: Pending
type: implementation
blocked_by: [ST-2]
unlocks: [ST-10]
severity: Medium
target_repo: plexus-substrate
---

## Problem

Arbor is already the most typed activation — `ArborId(Uuid)`, `NodeId = ArborId`, `TreeId = ArborId` are in place. But any `&str` / `String` parameter on Arbor's public method surface that represents a handle, session correlation, or other cross-boundary concept is still stringly typed. Arbor is the lowest-risk migration target and serves as a proof-of-pattern before the larger Orcha/ClaudeCode migrations land in parallel.

## Context

Arbor lives under `src/activations/arbor/`. Existing typed identifiers:

| Name | Shape | Keep |
|---|---|---|
| `ArborId` | `struct ArborId(Uuid)` in `types.rs` | Yes — already canonical |
| `NodeId` | `type NodeId = ArborId;` | Yes — alias is load-bearing in other activations' imports |
| `TreeId` | `type TreeId = ArborId;` | Yes — same |

Files owned by this ticket (exclusive write):

- `src/activations/arbor/activation.rs`
- `src/activations/arbor/methods.rs`
- `src/activations/arbor/storage.rs`
- `src/activations/arbor/types.rs`
- `src/activations/arbor/views.rs`
- `src/activations/arbor/mod.rs`

No other ticket writes these files. Coordination with ST-6 (ClaudeCode) and ST-8 (Cone) is read-only: they import `arbor::NodeId` and `arbor::TreeId` — the alias names stay identical.

The ST-2 foundation module provides `crate::types::{SessionId, TicketId, ...}`. This ticket imports as needed.

## Required behavior

Audit every `pub fn`, `pub async fn`, public struct field, and public enum variant under `src/activations/arbor/`. For each parameter or field of type `String` / `&str` / `Uuid` / `PathBuf`, ask:

| Concept | Replace with |
|---|---|
| Represents a generic arbor node ID | `ArborId` or `NodeId` (unchanged) |
| Represents a handle string crossing activation boundaries | Keep as `String` — handle strings are free-form data, not domain IDs |
| Represents a `SessionId`, `WorkingDir`, `TicketId`, etc. | Use ST-2 newtype |
| Represents a human-readable name / description | Keep as `String` |

Input/output table for Arbor public surface (showing before → after for any parameter that changes):

| Current signature | New signature |
|---|---|
| Existing `ArborId` / `NodeId` / `TreeId` uses | Unchanged |
| Any `fn` with a `session_id: String` parameter | `session_id: SessionId` |
| Any public struct field named `session_id: String` | `session_id: SessionId` |
| Any public field named `working_dir: String` (if present) | `working_dir: WorkingDir` |
| Any handle-lookup function taking an opaque handle string | Unchanged (handles are free-form) |

If the audit finds Arbor has no cross-boundary stringly-typed parameters beyond what's already newtyped, this ticket's scope reduces to: document the audit result in the commit message, add an integration test that confirms `arbor::NodeId` is still the `ArborId` alias, and mark Complete.

Storage row reads: SQLite columns remain `TEXT`. Use `ArborId::parse_str(row.get::<String, _>("id"))` or equivalent at the storage boundary. No schema migration.

## Risks

- **Arbor has almost no stringly-typed cross-boundary IDs already.** Likely this ticket produces a small diff or confirms no-op. That is an acceptable outcome — record it in the commit.
- **Handle strings are `String` by design.** Handles encode `{plugin_id}@{version}::{method}:{entity}:{extra}` and are not identifiers of a single domain concept. Do NOT wrap handle strings.
- **Breaking `NodeId` / `TreeId` aliases would ripple into six other files.** Aliases stay as-is.

## What must NOT change

- Public wire format — every Arbor RPC method serializes identically before and after.
- SQLite schema for Arbor's shared tree DB.
- `ArborId`, `NodeId`, `TreeId` shapes and names.
- Arbor's `Activation` impl signature and namespace.

## Acceptance criteria

1. `cargo build -p plexus-substrate` succeeds.
2. `cargo test -p plexus-substrate` succeeds — no regressions.
3. Grep audit limited to `src/activations/arbor/`: no bare `String` parameter remains in a public function signature that represents a `SessionId`, `GraphId`, `StreamId`, `ApprovalId`, `ToolUseId`, `TicketId`, `WorkingDir`, `ModelId`, or `TemplateId`.
4. `arbor::NodeId` and `arbor::TreeId` remain aliases of `ArborId`; downstream imports `use crate::activations::arbor::{NodeId, TreeId};` in `claudecode/types.rs` and `cone/types.rs` still compile without edit.
5. Commit message explicitly states whether any signature changed; if no signature changed, states "Arbor audit confirmed no cross-boundary stringly-typed parameters; types.rs unchanged."

## Completion

Implementor delivers:

- A commit modifying only files under `src/activations/arbor/` (or zero files if the audit finds no migration needed, in which case the commit is a note in a CHANGELOG or directly in the next ticket's commit — the implementor's call).
- `cargo build -p plexus-substrate` and `cargo test -p plexus-substrate` green.
- Ticket status flipped from `Ready` → `Complete`.
- ST-10 notified that Arbor is ready for wire-format roundtrip testing.
