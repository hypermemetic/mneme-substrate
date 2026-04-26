---
id: ST-8
title: "Migrate Cone + remaining activations to use typed IDs"
status: Pending
type: implementation
blocked_by: [ST-2]
unlocks: [ST-10]
severity: Medium
target_repo: plexus-substrate
---

## Problem

Cone uses `model_id: String` in multiple places (`cone/activation.rs:181`, `cone/storage.rs:101`) — a bare string passed through `create_cone`, stored in SQLite, and round-tripped to display. Swapping `model_id` with any other bare `String` parameter in the same function (e.g. `name`, `system_prompt`) silently compiles. Remaining activations (bash, changelog, chaos, echo, health, interactive, solar) are largely stateless or use typed IDs already, but any residual stringly-typed cross-boundary parameters should be swept here in one pass.

## Context

Cone lives under `src/activations/cone/`. Current usage:

```rust
fn cone_create(..., model_id: String, ...)
fn llm_registry.from_id(&model_id)
let cone.model_id: String
```

The ST-2 foundation provides `ModelId`.

Remaining activations to audit in this ticket:

- `src/activations/bash/` — BashOutput uses `String` for stdout/stderr; no cross-boundary IDs expected.
- `src/activations/changelog/` — hash and entry IDs; audit for cross-boundary IDs.
- `src/activations/chaos/` — uses `spec_type: String` (audit notes should be an enum — out of scope for ST; leave).
- `src/activations/echo/` — stateless.
- `src/activations/health/` — stateless.
- `src/activations/interactive/` — audit for session/stream ID usage.
- `src/activations/solar/` — hub activation; audit for child identifier parameters.

Files owned by this ticket (exclusive write):

- `src/activations/cone/*` (all files)
- `src/activations/bash/*`
- `src/activations/changelog/*`
- `src/activations/chaos/*`
- `src/activations/echo/*`
- `src/activations/health/*`
- `src/activations/interactive/*`
- `src/activations/solar/*`

Registry and Mustache are NOT in this ticket — ST-9 owns them.

## Required behavior

Cone-specific input/output table:

| Current signature | New signature |
|---|---|
| `fn cone_create(..., model_id: String, ...)` | `fn cone_create(..., model_id: ModelId, ...)` |
| `pub struct Cone { ..., pub model_id: String, ... }` | `model_id: ModelId` |
| `llm_registry.from_id(&model_id)` where `model_id: String` | `llm_registry.from_id(model_id.as_str())` or `.from_id(&model_id)` if `from_id` accepts `&ModelId` — author decides based on `from_id` signature. If `from_id` is cross-crate (cllient), adapt at the call site. |
| Cone SQLite storage `bind(&model_id)` | `bind(model_id.as_str())` |

For each remaining activation, the audit yields one of three outcomes:

| Outcome | Action |
|---|---|
| Activation has no cross-boundary stringly-typed IDs | No edit; document in commit |
| Activation has one or more such IDs | Apply the ST-2 newtype to the public surface and storage boundary |
| Activation has IDs that are activation-internal only | Leave as local aliases; document |

Solar (hub activation) exposes children via `#[plexus_macros::child]` — child names remain `String` (they are routing labels, not identifiers of a single domain concept).

Enum migration for `spec_type` (chaos/types.rs) — OUT of scope. The audit flags it as "should be an enum" but leaving it as `String` is intentional for this epic; a follow-up ticket can handle it.

## Risks

- **Cross-crate interaction with cllient.** `llm_registry.from_id(...)` may live in the `cllient` crate (see `.cargo/config.toml` patch in README). If `from_id` takes `&str`, adapt with `.as_str()`. If it takes `&String`, convert explicitly. Do NOT modify cllient in this ticket — adapt at the call site only.
- **Seven-activation audit may find nothing.** Acceptable outcome; document.
- **`BashOutput` variants** include stdout/stderr/exit-code strings. These are content, not IDs — do NOT wrap.
- **Changelog hashes.** The changelog uses SHA-style hash strings. If a `ChangelogHash` newtype seems warranted, it's out of scope for this ticket (not in the ST-2 pinned list). Document the finding and file a follow-up ticket if useful.

## What must NOT change

- Wire format for every affected RPC method — byte-identity.
- SQLite schemas.
- Enum shapes (`SpecType` / `spec_type` stays `String` deliberately).
- Cone's `Model` / `MessageRole` enums.
- Hub behavior in Solar — child lookup signatures are unchanged.

## Acceptance criteria

1. `cargo build -p plexus-substrate` succeeds.
2. `cargo test -p plexus-substrate` succeeds.
3. Grep audit inside `src/activations/cone/`: no `model_id: String` on a public function parameter or struct field. All occurrences are `ModelId`.
4. Grep audit across all activations covered by this ticket: no bare `String` parameter on any public function signature represents a `SessionId`, `GraphId`, `NodeId`, `StreamId`, `ApprovalId`, `ToolUseId`, `TicketId`, `WorkingDir`, or `ModelId`.
5. Commit message lists each of the seven non-Cone activations and states the audit outcome (no-op, IDs found and migrated, or IDs found and deemed activation-internal).
6. A unit test in Cone constructs a `Cone` struct with a typed `ModelId`, round-trips through serde, and compares byte-identity against a committed pre-migration fixture.

## Completion

Implementor delivers:

- A commit modifying only files under `src/activations/cone/`, `src/activations/bash/`, `src/activations/changelog/`, `src/activations/chaos/`, `src/activations/echo/`, `src/activations/health/`, `src/activations/interactive/`, `src/activations/solar/`.
- Committed JSON fixture `tests/fixtures/cone_wire.json`.
- `cargo build -p plexus-substrate` and `cargo test -p plexus-substrate` green.
- Ticket status flipped from `Ready` → `Complete`.
- ST-10 notified.
