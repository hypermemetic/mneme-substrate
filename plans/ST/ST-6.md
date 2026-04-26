---
id: ST-6
title: "Migrate ClaudeCode to use typed IDs"
status: Pending
type: implementation
blocked_by: [ST-2]
unlocks: [ST-10]
severity: High
target_repo: plexus-substrate
---

## Problem

ClaudeCode defines local identifier aliases and participates in several cross-activation correlation pairs. From the audit:

- `pub type StreamId = Uuid;` in `claudecode/types.rs:78` — crosses Chat lifecycle ↔ Orcha polling.
- `session_id: String` fields throughout — the audit calls out `claude_session_id` vs `loopback_session_id` as a top-swap hazard.
- `tool_use_id: String` — paired with `tool_name: String`; swapping them silently corrupts the permit logic.
- `model_id: String` in some places vs. the typed `Model` enum in others — no canonical boundary type.
- `working_dir: String` — should be `WorkingDir` (PathBuf).

The local `pub type ClaudeCodeId = Uuid;` at the top of `claudecode/types.rs:12` is a distinct concept (an activation-internal configuration ID) and can be kept as a private newtype — or, if it has no cross-boundary use, left as a type alias.

## Context

ClaudeCode lives under `src/activations/claudecode/`. Current aliases from `types.rs`:

```rust
pub type ClaudeCodeId = Uuid;   // line 12
pub type StreamId = Uuid;        // line 78
pub type MessageId = Uuid;       // line 81
```

The existing typed `Model` enum stays — it's used for validated model selection. `ModelId` from ST-2 is the **unvalidated string** crossing activation boundaries; internal code converts `ModelId` → `Model` via a parser method on `Model` (already exists as `Model::from_str` or similar).

Files owned by this ticket (exclusive write):

- `src/activations/claudecode/activation.rs`
- `src/activations/claudecode/types.rs`
- `src/activations/claudecode/storage.rs`
- `src/activations/claudecode/mod.rs`
- `src/activations/claudecode/render.rs`
- any other `src/activations/claudecode/*.rs`

ClaudeCode imports `arbor::{NodeId, TreeId}` — unchanged.
ClaudeCode currently imports `orcha::SessionId` (alias `String`) — change to `crate::types::SessionId`.

## Required behavior

Input/output table:

| Current signature | New signature |
|---|---|
| `pub type StreamId = Uuid;` in `types.rs` | Removed; re-exported from `crate::types::StreamId` (with `#[deprecated]` re-export for merge-order safety) |
| `pub type ClaudeCodeId = Uuid;` | Kept as a local alias OR promoted to a private newtype `ClaudeCodeConfigId(Uuid)`. Default: keep alias (not a cross-boundary ID). Decision documented in commit. |
| `pub type MessageId = Uuid;` | Kept as a local alias (activation-internal; not cross-boundary) |
| Any `session_id: String` on a public struct | `session_id: SessionId` |
| Any `stream_id: String` or `stream_id: Uuid` on a public struct | `stream_id: StreamId` |
| Any `tool_use_id: String` on a public struct | `tool_use_id: ToolUseId` |
| Any `model_id: String` on a public struct | `model_id: ModelId` |
| Any `model: String` field that represents a loose model identifier (not the validated `Model` enum) | `model: ModelId` |
| The existing `Model` enum | Unchanged — continues to be used for validated internal selection |
| `fn start_stream(session_id: String, model: ..., working_dir: String, ...) -> ...` | `fn start_stream(session_id: SessionId, model: ModelId, working_dir: WorkingDir, ...) -> ...` (or `Model` for the parsed form; depends on where parsing lives) |
| Any `HashMap<StreamId, ActiveStreamBuffer>` | Keyed by `StreamId` newtype |
| Handle types (`ClaudeCodeHandle`) internal `String` fields | Unchanged — handles carry free-form data |

**Canonicalization decision for Claude-originated vs generic SessionIds:** call sites that manage a Claude Code subprocess session (distinct from Orcha's logical session) may need distinguishing. Options:

- (a) Both use `SessionId` — distinguish by field name at call sites. Recommended default.
- (b) Introduce a ClaudeCode-private newtype `ClaudeProcessSessionId(SessionId)` for the subprocess ID specifically.

Default: (a). If (b) is chosen it stays private to ClaudeCode; ST-2 / `crate::types` is NOT modified.

Storage: SQLite columns stay the same. Reads wrap raw strings/UUIDs into newtypes at the storage boundary.

## Risks

- **`StreamId` is used in Orcha's stream-polling state machine.** Orcha must import `crate::types::StreamId` after ST-4. A `#[deprecated]` re-export at `claudecode::StreamId` eases merge-order.
- **`ModelId` vs `Model` enum.** ClaudeCode already has a validated `Model` enum. Cross-boundary is `ModelId` (unvalidated string); internal validated form is `Model`. Parsing happens at the boundary. If a new `impl From<ModelId> for Result<Model, Err>` is needed, add it; do NOT change the `Model` enum's serde form.
- **`ClaudeCodeId` is not in the pinned newtype list.** Keep as local alias unless audit shows it crosses an activation boundary. If it does, document and add a private newtype.
- **`HandleEnum`-generated code.** The `#[derive(HandleEnum)]` macro generates resolution code from the enum fields. Fields are `String` by design (free-form handle data); do NOT wrap them in newtypes.

## What must NOT change

- Wire format for every ClaudeCode RPC method — byte-identity, including handle strings.
- SQLite schema.
- The `Model` enum's shape or serde rename rules.
- The `ClaudeCodeHandle` enum variants or their field types (handles carry free-form data).
- The activation's namespace (`"claudecode"`), method names, or children.

## Acceptance criteria

1. `cargo build -p plexus-substrate` succeeds.
2. `cargo test -p plexus-substrate` succeeds.
3. `pub type StreamId = Uuid;` no longer exists in `claudecode/types.rs` (replaced by a `#[deprecated]` re-export).
4. Grep audit: no bare `String` parameter in any public function in `src/activations/claudecode/` represents a `SessionId`, `StreamId`, `ToolUseId`, `ModelId`, `WorkingDir`, `ApprovalId`, or `TicketId`.
5. `tool_name: String` and `tool_use_id: ToolUseId` on the same struct/function can no longer be silently swapped (the compiler rejects passing a `ToolUseId` into a `&str` parameter intended for `tool_name`, and vice versa).
6. The `Model` enum is unchanged; a new (or existing) `ModelId → Model` parser is demonstrable in tests.
7. A unit test constructs the main ClaudeCode stream/message request/response types and compares serde JSON byte-identity against a committed pre-migration fixture.

## Completion

Implementor delivers:

- A commit modifying only files under `src/activations/claudecode/`.
- A committed JSON fixture `tests/fixtures/claudecode_stream_wire.json`.
- `cargo build -p plexus-substrate` and `cargo test -p plexus-substrate` green.
- Ticket status flipped from `Ready` → `Complete`.
- ST-10 notified.
