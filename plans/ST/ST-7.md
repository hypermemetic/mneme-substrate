---
id: ST-7
title: "Migrate Loopback to use typed IDs"
status: Pending
type: implementation
blocked_by: [ST-2]
unlocks: [ST-10]
severity: High
target_repo: plexus-substrate
---

## Problem

Loopback already defines `pub type ApprovalId = Uuid;` — closer to typed than most activations but still a type alias, not a newtype. Orcha uses bare `String` for the same concept on its side; the ST-2 foundation canonicalizes `ApprovalId` as a newtype around `Uuid`. This ticket removes Loopback's alias and adopts the canonical type.

Loopback also uses bare `String` for `session_id`, `tool_name`, `tool_use_id`. `tool_name` is arguably a bounded enum (there are a finite set of tool names Claude Code can request permits for), but Loopback-side doesn't enumerate them — leave as `String` since it's a name not an ID. `tool_use_id` IS an ID correlating permit round-trips and must be typed.

## Context

Loopback lives under `src/activations/claudecode_loopback/`. Current aliases from `types.rs`:

```rust
pub type ApprovalId = Uuid;   // line 8
```

Struct `ApprovalRequest` fields of interest:

```rust
pub struct ApprovalRequest {
    pub id: ApprovalId,                // OK — already typed
    pub session_id: String,            // → SessionId
    pub tool_name: String,             // stays String (name, not ID)
    pub tool_use_id: String,           // → ToolUseId
    pub input: Value,                  // stays Value
    pub status: ApprovalStatus,        // OK — already enum
    pub response_message: Option<String>,  // stays String (free-form)
    pub created_at: i64,               // timestamps stay i64
    pub resolved_at: Option<i64>,
}
```

Files owned by this ticket (exclusive write):

- `src/activations/claudecode_loopback/activation.rs`
- `src/activations/claudecode_loopback/types.rs`
- `src/activations/claudecode_loopback/storage.rs`
- `src/activations/claudecode_loopback/mod.rs`

## Required behavior

Input/output table:

| Current signature | New signature |
|---|---|
| `pub type ApprovalId = Uuid;` in `types.rs` | Removed; re-exported from `crate::types::ApprovalId` (`#[deprecated]` re-export at `claudecode_loopback::ApprovalId` for merge-order safety) |
| `pub struct ApprovalRequest { pub session_id: String, pub tool_use_id: String, ... }` | `session_id: SessionId`, `tool_use_id: ToolUseId`; `tool_name: String` unchanged |
| `pub struct PermitRequest { pub tool_name: String, pub tool_use_id: String, pub input: Value }` | `tool_use_id: ToolUseId` (others unchanged) |
| `LoopbackError::ApprovalNotFound { id: String }` | `LoopbackError::ApprovalNotFound { id: ApprovalId }` |
| `LoopbackConfig` fields with `session_id: String` (if present) | `SessionId` |
| Any public function taking `approval_id: Uuid` | `approval_id: ApprovalId` |
| Any public function taking `session_id: String` | `session_id: SessionId` |
| Any public function taking `tool_use_id: String` | `tool_use_id: ToolUseId` |

`#[serde(transparent)]` on the `ApprovalId` newtype guarantees the wire JSON remains a bare UUID string — identical to the current `type ApprovalId = Uuid;` shape.

Storage boundary: SQLite columns stay `TEXT` (already storing UUID string form). Reads wrap via `ApprovalId::new(...)`.

## Risks

- **Orcha currently uses `String` for approval IDs** on its end of the round-trip (the audit's "Orcha-side ApprovalId" row). ST-4 migrates Orcha's side to `ApprovalId`. Both sides converge on the ST-2 canonical newtype; neither side observes a wire-format change.
- **The loopback approval resolution error-swallowing sites (`let _ = storage.resolve_approval(...)`)** are out of scope (RL epic).
- **`tool_name` stays `String` deliberately.** Bounded-enum of tool names is a larger, separate decision — not in ST.

## What must NOT change

- Wire format for every Loopback RPC method — byte-identity, especially the UUID-string shape of `ApprovalId` serde output.
- SQLite schema.
- The `ApprovalStatus` enum.
- The permit-response `PermitResponse` enum shape (`Allow` / `Deny`).

## Acceptance criteria

1. `cargo build -p plexus-substrate` succeeds.
2. `cargo test -p plexus-substrate` succeeds.
3. `pub type ApprovalId = Uuid;` no longer exists in `claudecode_loopback/types.rs` (replaced by a `#[deprecated]` re-export of `crate::types::ApprovalId`).
4. Grep audit: no bare `String` parameter in any public function in `src/activations/claudecode_loopback/` represents a `SessionId`, `ApprovalId`, or `ToolUseId`.
5. A unit test constructs an `ApprovalRequest`, `PermitRequest`, and `PermitResponse`, round-trips each through serde, and compares byte-identity against committed pre-migration fixtures.
6. A cross-activation compile check: a function accepting `ApprovalId` cannot silently accept a bare `Uuid` without explicit `.into()` or `ApprovalId::new(...)` — demonstrate with a doc-test or unit test.

## Completion

Implementor delivers:

- A commit modifying only files under `src/activations/claudecode_loopback/`.
- Committed JSON fixtures `tests/fixtures/loopback_approval_request_wire.json`, `tests/fixtures/loopback_permit_request_wire.json`, `tests/fixtures/loopback_permit_response_wire.json`.
- `cargo build -p plexus-substrate` and `cargo test -p plexus-substrate` green.
- Ticket status flipped from `Ready` → `Complete`.
- ST-10 notified.
