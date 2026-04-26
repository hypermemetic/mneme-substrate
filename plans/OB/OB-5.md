---
id: OB-5
title: "Structured error context on the wire"
status: Pending
type: implementation
blocked_by: []
unlocks: []
severity: High
target_repo: plexus-substrate (+ possibly plexus-core for wire error shape)
---

## Problem

Substrate's per-activation error enums (`ConeError`, `OrchaError`, `ClaudeCodeError`, etc. — audit calls this out as a pattern that works) carry rich structured context internally. At the wire boundary, every error collapses to a `String` — `Result<T, String>` is the dispatch return type, and error variants are flattened via `Display` or `format!()`. An operator debugging a failed Orcha run sees `"graph execution failed: not found"` with no way to mechanically extract which graph, which node, which session. The structure is destroyed exactly where it's most needed.

This ticket surfaces the structured error context on the wire. Errors carry, at minimum:
- `activation`: the activation namespace (e.g., `"orcha"`).
- `method`: the RPC method that failed (e.g., `"run_graph"`).
- `kind`: a stable machine-readable error kind (e.g., `"not_found"`, `"invalid_arg"`, `"internal"`, `"timeout"`, activation-specific kinds like `"graph_cycle_detected"`).
- `message`: the human-readable string (the current flat-string contents, preserved for backward compatibility).
- Domain-specific structured fields where relevant — `graph_id`, `session_id`, `stream_id`, `approval_id`, `node_id`, `ticket_id`, and others depending on the activation.

## Context

**Affected components:**

| Component | Role |
|---|---|
| `plexus-core` (possibly) | Defines the wire error shape. If `PlexusError` or similar exists, OB-5 extends it; if the shape is `String`-only today, OB-5 introduces a new structured variant. |
| `plexus-substrate` dispatch boundary | The place where per-activation errors convert to wire errors. OB-5 rewrites the conversion to preserve structure. |
| Each activation's error enum | Every variant gains (or confirms) a stable `kind()` method and exposes its structured fields. |

**Cross-epic coordination.**

This ticket overlaps with **RL epic's error-typing work**. Contract:
- RL ensures errors are **typed** inside the activation (no silent `String::from` collapses, no `.ok()` error swallowing).
- OB-5 ensures the typed errors **surface on the wire** with structured fields.

OB-5 and RL land per-activation. For each activation:
- If RL has not yet migrated that activation, OB-5 inherits whatever shape exists. If it's already an enum (as the audit says most are), OB-5 wires it to the wire without asking RL to re-shape. If it's still string-heavy, OB-5 does a minimal enum migration for that activation scoped to the errors it surfaces.
- If RL has migrated the activation, OB-5 consumes the typed errors directly.

The two epics avoid re-flattening each other's work by owning disjoint concerns: RL owns "errors exist as types", OB-5 owns "types survive the wire".

**Cross-epic coordination with ST.** ST's newtypes (`GraphId`, `SessionId`, `StreamId`, `ApprovalId`, `ToolUseId`, `TicketId`, `NodeId`) appear in OB-5's structured-error context fields. If ST has shipped for an activation, OB-5 uses the newtypes. If ST has not, OB-5 uses the current bare types (`String`, `Uuid`); ST's migration later updates the fields to newtypes without changing the wire shape (the `serde(transparent)` derive preserves JSON compatibility).

**Wire shape (pinned for this ticket):**

The wire error becomes a structured JSON object instead of a bare string. Existing JSON-RPC or Plexus RPC response shape accommodates this — the exact placement depends on the current envelope (re-verify at implementation). Expected shape:

```json
{
  "error": {
    "activation": "orcha",
    "method": "run_graph",
    "kind": "graph_not_found",
    "message": "graph 3f2a... not found",
    "context": {
      "graph_id": "3f2a1c...",
      "session_id": "..."
    }
  }
}
```

**Backward compatibility.** Old clients that read `error` as a string see a structured object instead. Two compatibility strategies (pick during implementation, document in the PR):

- **Option A:** the wire envelope splits `error: string` (legacy) from `error_context: object` (new). Old clients read `error` unchanged; new clients read `error_context`.
- **Option B:** the wire envelope replaces `error: string` with `error: object`; old clients break (breaking change).

**A is preferred** unless the current envelope makes it awkward. The ticket documents the chosen strategy in the PR description.

**Error `kind()` taxonomy (pinned at the substrate-wide level):**

Every activation's error enum exposes a `kind(&self) -> &'static str` method. The top-level taxonomy (stable, substrate-wide) is:

| Kind | Meaning |
|---|---|
| `"not_found"` | Identifier does not resolve. |
| `"invalid_arg"` | Request parameter was malformed or out of range. |
| `"permission_denied"` | Caller not authorized. |
| `"precondition_failed"` | Resource in the wrong state for the requested operation. |
| `"timeout"` | Operation exceeded its deadline. |
| `"cancelled"` | Client or server cancelled the operation. |
| `"internal"` | Unexpected server-side failure; bug or infrastructure fault. |
| `"unavailable"` | Sibling activation or storage temporarily unavailable. |

Activation-specific kinds extend this set with prefixed names (e.g., `"orcha.graph_cycle_detected"`, `"cone.template_parse_error"`). The top-level taxonomy acts as a fallback classifier — callers can match on the prefix.

**Context fields (domain identifiers):**

Each error variant declares which context fields it populates. Examples:

| Error variant | Populated context fields |
|---|---|
| `OrchaError::GraphNotFound { id }` | `graph_id` |
| `OrchaError::NodeFailed { graph_id, node_id, reason }` | `graph_id`, `node_id` |
| `ClaudeCodeError::StreamEnded { stream_id }` | `stream_id` |
| `LoopbackError::ApprovalExpired { id, graph_id }` | `approval_id`, `graph_id` |
| `ConeError::TemplateNotFound { template_id }` | `template_id` |

The full mapping per activation is pinned in the PR — each activation's error variants get a `context(&self) -> ErrorContext` impl (or equivalent) enumerating the fields.

## Required behavior

### Wire error envelope

For every RPC method (non-streaming and streaming alike), errors surface on the wire with:

| Field | Required | Source |
|---|---|---|
| `activation` | yes | The activation namespace handling the call. |
| `method` | yes | The method name (e.g., `"run_graph"`). |
| `kind` | yes | From the activation error's `kind()` method. |
| `message` | yes | The `Display` output (human-readable; current flat-string content). |
| `context` | yes; may be empty object | Structured domain fields per the variant's declaration. |

### Instrumentation integration

The structured error shape flows into OB-3's metrics: the `outcome` label on `substrate_rpc_calls_total` gains a sub-label `error_kind` for errored calls — `outcome="err", error_kind="not_found"`. This integration is optional if OB-3 hasn't landed yet; OB-5 pins the field name so OB-3 consumes it cleanly when both ship.

### Logging integration

Errors logged via `tracing::error!` include the structured fields as span attributes (e.g., `error_kind=%e.kind(), graph_id=%ctx.graph_id`). This is how structured-error context feeds observability beyond the wire: operators searching logs for `graph_id=3f2a...` see every error tagged with that graph.

### Per-activation error coverage

Every stateful activation's error enum is audited in this ticket:

| Activation | Action |
|---|---|
| Orcha, ClaudeCode, Cone, Loopback, PM, Lattice, Arbor, Mustache, Registry, MCP | Existing error enums' variants gain `kind()` and `context()`. |
| Echo, Health, Chaos, Bash, Interactive | Minimal error surface; audit to confirm each variant has a stable `kind`. Add the two methods. |

The ticket does **not** introduce new error variants — it documents and surfaces the existing ones.

## Risks

| Risk | Mitigation |
|---|---|
| The wire envelope's current shape doesn't easily accommodate a structured error object. | Pick Option A or B per context. If both are awkward, escalate — the envelope shape may need a spike. OB-5 scope extends to propose the envelope change; implementation may span plexus-core too. |
| Clients (synapse, cllient) break because they deserialize errors as strings. | Option A preserves backward compat. If Option B is chosen, coordinate with synapse/cllient via follow-up tickets filed in the same PR. |
| Cross-activation errors flow through (activation A errors to activation B's call site). The wire surfaces B's wrapper, not A's inner. | The error envelope supports a `cause` field (nested structured error) for chain propagation. Implementation decides whether to flatten, chain, or both. Document the choice in the PR. |
| Activation error enums have variants with unclear semantics (e.g., `OrchaError::Other(String)`). | Each such variant maps to `kind = "internal"` with the string preserved in `message`. Document as a known limitation; follow-up tickets per activation can refine. |
| RL's error-typing work across activations is not synchronized with this ticket. | Per-activation sequencing: land OB-5 for activations RL hasn't touched yet; re-audit after RL migrates each activation. |
| Some activations' errors currently include sensitive information in strings (e.g., raw DB paths, partial API keys). | Audit during implementation. `message` is allowed to contain details; the `context` object is the structured view. If sensitive data leaks via `message`, flag the variant for redaction as a follow-up — OB-5 does not introduce redaction infrastructure. |
| File collision with OB-2 on `builder.rs`. | OB-5's edits to dispatch-boundary code are in dispatch glue, not startup. If they converge on `builder.rs`, coordinate per the epic's file-boundary note — land OB-2 first, rebase OB-5. |

## What must NOT change

- Success response shapes for any RPC method. Only the error shape changes.
- Activation namespace strings or method names.
- The per-activation error enum types' names and public variants (ticket adds methods; it does not rename or re-shape variants).
- Tracing behavior for non-error paths.
- Wire compatibility for streaming method **success** items. OB-6 handles streaming versioning; OB-5 stays scoped to error payloads.
- Schema hashes for unchanged methods.

## Acceptance criteria

1. An RPC call that errors returns a structured error envelope with `activation`, `method`, `kind`, `message`, and `context` fields. Verifiable by driving a known-failing call (e.g., `orcha.run_graph` with an invalid id) and parsing the response.
2. `context` for a `GraphNotFound` error includes a `graph_id` field matching the requested id.
3. `context` for a `StreamEnded` error (ClaudeCode) includes `stream_id`.
4. Every activation's error enum exposes `kind() -> &'static str` and `context() -> ErrorContext`. Verifiable by grep in the activation source tree.
5. `kind` values conform to the pinned taxonomy or use an activation-prefixed extension. No ad-hoc kinds.
6. Logging errors via `tracing::error!` at dispatch includes `error_kind` and relevant domain ids as span attributes. Verifiable by driving a failing call and inspecting stderr output.
7. Backward-compatibility strategy (Option A or B) is chosen, pinned in the PR description, and documented in CHANGELOG.
8. Synapse / cllient continue to render errors (either via the preserved `message` field in Option A, or via an updated renderer landed alongside in Option B).
9. `cargo test --workspace` passes unchanged.
10. At least one end-to-end test per activation asserts the structured error shape for a representative failure mode. Tests are committed in the activation's test module.
11. If OB-3 has landed: `substrate_rpc_calls_total` metric includes an `error_kind` sub-label on `outcome="err"` calls.

## Completion

PR against `plexus-substrate` (+ possibly `plexus-core` for the wire envelope). CI green. Status flipped from `Ready` to `Complete` in the same commit. When OB-5 lands, every error on the wire carries actionable structure; pairing with OB-3's `error_kind` label and OB-2's structured logging config, operators can mechanically correlate wire errors, log entries, and metric time-series.
