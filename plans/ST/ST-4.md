---
id: ST-4
title: "Migrate Orcha to use typed IDs"
status: Pending
type: implementation
blocked_by: [ST-2]
unlocks: [ST-10]
severity: High
target_repo: plexus-substrate
---

## Problem

Orcha is the biggest swap-compiles hazard in the codebase. `SessionId = String` is defined locally; Orcha also passes bare `String` values for `GraphId`, `ApprovalId`, `TicketId`, `WorkingDir`, and `ModelId`. Two fields of the same raw type in the same struct (`claude_session_id` and `loopback_session_id` — both `String`) are trivially swappable; the Orcha cancel registry keys on `GraphId = String` while the Lattice module that produces those IDs uses its own `type GraphId = String`. The compiler cannot catch cross-boundary confusion today.

## Context

Orcha lives under `src/activations/orcha/`. Current stringly-typed surface from the audit:

| Concept | Current | Boundary |
|---|---|---|
| `SessionId` | `type SessionId = String;` in `orcha/types.rs:53` | Orcha ↔ ClaudeCode ↔ Loopback |
| `GraphId` | bare `String` field in graph/cancel types | Orcha ↔ Lattice |
| `ApprovalId` | bare `String` in Orcha — Loopback uses `ApprovalId(Uuid)` | Orcha ↔ Loopback |
| `TicketId` | `Option<String>` in event types | Orcha ↔ PM |
| `claude_session_id` / `loopback_session_id` | both bare `String` in the same struct | Orcha internal, trivially swappable |
| `working_dir` | bare `String` | Orcha ↔ ClaudeCode |
| `model_id` | bare `String` in Cone; typed enum `Model` in ClaudeCode; stringly in Orcha | Orcha ↔ Cone ↔ ClaudeCode |

Files owned by this ticket (exclusive write):

- `src/activations/orcha/activation.rs`
- `src/activations/orcha/types.rs`
- `src/activations/orcha/storage.rs`
- `src/activations/orcha/orchestrator.rs`
- `src/activations/orcha/context.rs`
- `src/activations/orcha/graph_runner.rs`
- `src/activations/orcha/ticket_compiler.rs`
- any other `src/activations/orcha/*.rs` file

All ST-2 newtypes are imported from `crate::types`.

The existing `pub type SessionId = String;` in `orcha/types.rs:53` is **removed** by this ticket. Downstream activations (ClaudeCode, Loopback) currently import Orcha's alias — those imports change to `crate::types::SessionId` in their own tickets (ST-6, ST-7). This ticket must not leave Orcha re-exporting the old alias.

## Required behavior

Input/output table for every changed public signature in Orcha:

| Current signature | New signature |
|---|---|
| `pub type SessionId = String;` in `types.rs` | Removed; `SessionId` re-exported from `crate::types` |
| `pub struct SessionInfo { pub session_id: SessionId (= String), ... }` | `pub struct SessionInfo { pub session_id: SessionId, ... }` (now wrapping newtype) |
| `fn start_session(session_id: String, working_dir: String, model: String, ...) -> ...` | `fn start_session(session_id: SessionId, working_dir: WorkingDir, model: ModelId, ...) -> ...` |
| `fn cancel_graph(graph_id: String) -> ...` | `fn cancel_graph(graph_id: GraphId) -> ...` |
| Cancel registry `HashMap<String, ...>` keyed by graph id | `HashMap<GraphId, ...>` |
| Ticket-correlation events with `ticket_id: Option<String>` | `ticket_id: Option<TicketId>` |
| Approval-related structs with `approval_id: String` | `approval_id: ApprovalId` |
| Any field `claude_session_id: String` and sibling `loopback_session_id: String` in the same struct | Both become `SessionId` — but they MUST be distinguished. Keep distinct field names; the compiler no longer catches swap between them, so add a debug-only assertion or comment pointing out that the two SessionId values are intentionally distinct instances. If distinguishing by type is essential, introduce `ClaudeSessionId` and `LoopbackSessionId` as private wrappers around `SessionId` in Orcha's types. Decide once and document in the ticket's commit message. |
| Any `model_id: String` field | `model_id: ModelId` |
| Error variants `OrchaError::SessionNotFound { session_id: String }` | `OrchaError::SessionNotFound { session_id: SessionId }` (same for other variants carrying IDs) |

**Canonicalization decision for `claude_session_id` vs `loopback_session_id`:** the author of this ticket decides between (a) both are `SessionId` with distinct field names, relying on named-field call sites for safety, or (b) introducing two Orcha-private newtypes `ClaudeSessionId(SessionId)` and `LoopbackSessionId(SessionId)`. Default choice: (a), because field names at construction sites provide sufficient safety and (b) creates taxonomy drift with ST-2. If (b) is chosen, the two wrappers remain private to `orcha/types.rs` and do NOT cross into ST-2 / `crate::types`.

Storage boundary: SQLite columns stay `TEXT`. Rust reads `String` and wraps via `SessionId::new(...)` before returning from the storage layer.

## Risks

- **Cross-activation imports break.** ClaudeCode and Loopback currently reach for `orcha::SessionId`. After this ticket, that alias is gone. Those imports must switch to `crate::types::SessionId` in their respective migration tickets (ST-6, ST-7). Since ST-4, ST-6, ST-7 run in parallel after ST-2, coordination is via a build-breaks-compiler: any ticket landing first that removes `orcha::SessionId` breaks the others until they update. Mitigation: order-of-merge coordination by the human running the epic, OR keep a deprecated `pub use crate::types::SessionId;` re-export at `orcha::SessionId` for one version — this ticket chooses the deprecated re-export path to avoid merge-order dependencies. Record the `#[deprecated]` attribute on the re-export.
- **`Option<TicketId>` vs `Option<String>` in serde.** `#[serde(transparent)]` on `TicketId` preserves wire format; `Option<TicketId>` serializes as either `null` or a bare JSON string, identical to `Option<String>`. Verify in ST-10's roundtrip fixture.
- **`ApprovalId` canonicalization timing.** Loopback's `pub type ApprovalId = Uuid;` is removed by ST-7. Orcha's bare `String` for approval IDs becomes `ApprovalId` in this ticket. Wire-format continuity: on the wire `ApprovalId` is a JSON string (UUID canonical form); both old bare-string-UUID and new `ApprovalId(Uuid)` serialize to the same JSON. No client observes a change.
- **`graph_runner.rs` has `unreachable!()` and `panic!` sites (audit notes this).** Not in scope for this ticket; RL epic owns resilience. Leave as-is.

## What must NOT change

- Wire format for every Orcha RPC method — byte-identity.
- SQLite schema for Orcha's DB.
- Error-variant names (internal shape changes; field types change).
- Public RPC method names.
- Orcha's `Activation` impl, namespace, or children.
- Existing behavior around ticket persistence, graph cancellation, approval round-tripping.

## Acceptance criteria

1. `cargo build -p plexus-substrate` succeeds.
2. `cargo test -p plexus-substrate` succeeds.
3. `pub type SessionId = String;` no longer exists in `orcha/types.rs` (replaced by a `#[deprecated]` re-export of `crate::types::SessionId`).
4. Grep audit: no bare `String` parameter in any public function in `src/activations/orcha/` represents a `SessionId`, `GraphId`, `StreamId`, `ApprovalId`, `ToolUseId`, `TicketId`, `WorkingDir`, or `ModelId`.
5. The cancel registry in Orcha is keyed by `GraphId` (not `String`).
6. Every `OrchaError` variant that previously carried an `_id: String` field now carries the corresponding newtype.
7. The `claude_session_id` / `loopback_session_id` handling decision is documented in the commit message with rationale.
8. A unit test in `orcha/tests.rs` (or inline `#[cfg(test)]`) constructs a `SessionInfo` via typed constructors and serializes/deserializes it; the resulting JSON is compared against the JSON produced by the pre-migration shape (committed fixture).

## Completion

Implementor delivers:

- A commit modifying only files under `src/activations/orcha/`.
- A committed JSON fixture `tests/fixtures/orcha_session_info_wire.json` used in the serialization test.
- `cargo build -p plexus-substrate` and `cargo test -p plexus-substrate` green.
- Commit message documents the canonicalization decision for `claude_session_id` / `loopback_session_id`.
- Ticket status flipped from `Ready` → `Complete`.
- ST-10 notified that Orcha's wire-format fixture is ready.
