---
id: STG-9
title: "MCP session storage: McpSessionStore trait"
status: Pending
type: implementation
blocked_by: [STG-2]
unlocks: [STG-10]
severity: Medium
target_repo: plexus-substrate
---

## Problem

`src/mcp_session.rs` (412 lines) persists MCP bridge session state via SQLite — but it is NOT an activation. It lives outside `src/activations/` and has a concrete SQLite pool hand-wired in its constructor. It has the same backend-lock-in as the activations but doesn't fit the per-activation trait pattern by namespace.

Apply the same treatment: extract an `McpSessionStore` trait, provide a `SqliteMcpSessionStore` default, and an `InMemoryMcpSessionStore` for tests. Thread through `mcp_bridge.rs` wherever the MCP session store is constructed.

Note: `McpSessionStore` is NOT pinned in `plans/README.md`'s cross-epic contracts table (only activation traits are). Pin it in the same PR as this ticket by adding the trait name to the trait-surfaces table.

## Context

Target file set: `src/mcp_session.rs`, `src/mcp_bridge.rs` (for the constructor call-site update), and possibly a new `src/mcp_session_memory.rs` or similar for the in-memory backend (layout per STG-2's pattern doc — inline in `mcp_session.rs` is also acceptable).

Pattern pinned in `docs/architecture/<nanotime>_storage-trait-pattern.md` (STG-2). Read the migration checklist first.

**Cross-epic inputs:**

- **ST newtypes** relevant to MCP session: `SessionId` at minimum. Possibly others if MCP session tracks arbor handles, stream IDs, etc.
- **Technical debt audit** notes `.ok()` error swallowing on multiple MCP session DB ops. That's RL's problem. Preserve current behavior.
- **`plans/README.md`** trait-surfaces table needs updating in the same PR.

## Required behavior

- Extract a public `McpSessionStore` trait from the current concrete type in `mcp_session.rs`.
- `SqliteMcpSessionStore` is the production default.
- `InMemoryMcpSessionStore` gated per STG-2's mechanism.
- Primary constructor for the MCP session manager / bridge component: accepts `Arc<dyn McpSessionStore>`.
- Convenience constructors per STG-2 pattern.
- `mcp_bridge.rs` (or wherever the MCP session is instantiated at startup) updated to preserve pre-epic production behavior.
- `plans/README.md`'s trait-surfaces table gains a row for `McpSessionStore`.

## Regression guarantees

| Surface | Post-ticket behavior |
|---|---|
| MCP bridge external behavior | Unchanged. |
| On-disk SQLite schema for MCP sessions | Unchanged. |
| Session lifecycle, cleanup, expiration semantics | Unchanged. |
| All existing MCP tests | Pass against `SqliteMcpSessionStore`. |

## Risks

| Risk | Mitigation |
|---|---|
| MCP session is structurally different from activations (not a `plexus_core::Activation` impl). | That's fine — STG's pattern is about storage-trait seams, not about activation trait conformance. The pattern applies to any code with a hand-wired SQLite pool. |
| MCP session tests may be sparse. | The test pass condition is best-effort: every existing test passes against both backends. If there are no tests, the PR adds at least two: one round-trip per backend. |
| `.ok()` error swallowing obscures real bugs. | Out of scope. Preserve behavior. |

## What must NOT change

- MCP bridge's external behavior.
- SQLite schema or file path for MCP session DB.
- Session lifecycle semantics.
- Any file outside `src/mcp_session.rs`, `src/mcp_bridge.rs`, and (possibly) `src/activations/storage.rs` or new shared helpers.

## Acceptance criteria

1. `cargo build -p plexus-substrate` succeeds.
2. `cargo test -p plexus-substrate` — all pre-epic tests pass.
3. `cargo test -p plexus-substrate --features test-doubles` — any MCP session tests run against both backends, all green. If no tests exist, at least two round-trip tests are added (one per backend).
4. `src/mcp_session.rs` exposes `McpSessionStore`, `SqliteMcpSessionStore`, `InMemoryMcpSessionStore` as public items.
5. The MCP session manager's production constructor produces behavior indistinguishable from pre-epic — verified by an integration test that exercises at least one session creation + retrieval.
6. `mcp_bridge.rs`'s default startup produces identical on-disk state.
7. `plans/README.md`'s trait-surfaces table includes `McpSessionStore` with STG as the owner epic.

## Completion

- PR against `plexus-substrate` landing the trait extraction, both backends, any required tests, `mcp_bridge.rs` call-site update, and the `plans/README.md` update.
- PR description includes all test commands and ST newtype integration status.
- Ticket status flipped from `Ready` → `Complete` in the same commit as the code.
