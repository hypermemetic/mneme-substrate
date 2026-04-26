---
id: RL-9
title: "Cancellation token end-to-end (transport → hub → activation)"
status: Pending
type: implementation
blocked_by: [RL-S01]
unlocks: [RL-10]
severity: High
target_repo: plexus-substrate
---

## Problem

Cancellation is inconsistent across substrate. Orcha has an ad-hoc `cancel_registry` on watch channels. Echo, Cone, ClaudeCode, and every other streaming activation ignore client disconnects and run to completion regardless of whether the caller is still listening. There is no unified `CancellationToken` flowing from transport → hub → activation, which means:

- A disconnected client leaks server-side work.
- There is no consistent contract activations can honour: some observe their own ad-hoc mechanism, most observe nothing.
- Graceful shutdown (RL-10) has no handle to stop in-flight work.

## Context

**Depends on RL-S01.** The spike pins the exact mechanism — whether the token flows via a trait parameter (Option A), a request-scoped context / task-local (Option B), or a coarser `AbortHandle` fallback. RL-9 implements whatever the spike picked. This ticket is written before the spike resolves because the *scope and shape of work* is stable across outcomes; only the *concrete mechanism* varies.

**README pin.** Cross-epic contracts table in `plans/README.md` names `CancellationToken` with owner = RL. After RL-S01 lands, update the README row with the mechanism (A / B / coarser) and the latency budget (500 ms from the spike, or 2 s if the fallback regime applied).

**Scope of this ticket:**

1. Transport integration — on the transport edge (cllient or the websocket handler), a client disconnect triggers cancellation of the associated hub-side token. Mechanism is transport-specific; implementor handles both the JSON-RPC call-response disconnect and the streaming disconnect.
2. Hub integration — `DynamicHub` constructs a token per incoming request, passes it to the activation's handler per the spike's shape (A / B / coarser), and cancels it on transport-side signal or hub-owned timeout (if a request timeout is configured; RL-9 does not add new timeouts).
3. Activation integration — every streaming activation (Echo, Cone, ClaudeCode, Orcha, Health's stream methods, Bash if it streams) observes the token in its poll loop and terminates within the spike-pinned latency budget.
4. Orcha's `cancel_registry` reconciliation — either migrated to the unified token (preferred) or retained as a specialization with the unified token as the outer cancel (fallback). RL-9 picks per what is least invasive; if the migration is > 1 day of work, retain the registry and have the unified token trigger a registry-level cancel.

**`src/builder.rs` edit.** RL-9 threads the token into the builder. RL-2 lands first to replace `.expect()` — RL-9 rebases.

## Required behavior

| Event | Current observable behavior | Required observable behavior |
|---|---|---|
| Client connects, issues a streaming call, disconnects mid-stream | Server continues running the stream to completion; no cancellation | Server detects the disconnect, cancels the per-request token, activation's poll loop exits within the spike's latency budget. |
| Client issues a unary call, closes the connection before the response is sent | Response is sent to nobody; activation still ran to completion | Same as above — token cancelled; activation exits early if the call has a cancellation-observation point. |
| Activation's handler explicitly checks the token and exits on cancel | n/a (no token to check) | Handler exits the poll loop on `token.cancelled()` within the latency budget. |
| A non-streaming, fast unary call that completes before cancellation could fire | Normal path | Unchanged. |
| Orcha's existing `cancel_registry` users (RPC methods that call `orcha.cancel_graph`) | Work via the registry | Still work — either the registry is migrated to the unified token and users are unaffected, or the registry stays and is outer-driven by the unified token. |

## Risks

- **Transport-side disconnect detection.** On websocket, detecting a graceful close is easy; detecting a TCP-level drop requires either a heartbeat (out of scope) or relying on tokio's `tungstenite` error propagation on the next read. Implementor relies on the latter — acceptable because the activation will eventually observe cancellation on the next message boundary. Dead-client detection under no-activity is not in scope.
- **Per-activation adoption cost.** Every streaming activation needs a `select!` or equivalent on the token. The spike's Option A and Option B differ in how invasive this is. If the spike picked Option B (task-local), the per-activation change is smaller. If Option A (trait parameter), more files change. RL-9's diff size scales with the spike's outcome.
- **Orcha `cancel_registry` reconciliation.** If migrating the registry exceeds one focused day, fall back to layered cancellation (unified token cancels the registry's outer watch). This is an acceptable fallback documented in the spike's Context paragraph in RL-9's implementation PR.

## What must NOT change

- The set of RPC method names or request/response shapes on any activation. The token is a new mechanism, not a new parameter visible on the wire.
- Activation startup order in `src/builder.rs`.
- SQLite-per-activation layout.
- Existing `cargo test` pass rate.
- Semantics of successful (non-cancelled) calls at any activation.
- Orcha's observable cancellation semantics via `orcha.cancel_graph` — the method still cancels the graph; only the underlying plumbing may change.

## Acceptance criteria

1. A new integration test spawns a substrate with Echo (or Cone) registered, opens a streaming call, receives two items, closes the client connection, and asserts (via `JoinHandle::is_finished()` or server-side tracing events) that the server-side handler exited within the spike-pinned latency budget.
2. Every streaming activation's handler contains at least one `token.cancelled()` observation point (grep-verifiable).
3. Orcha's existing `orcha.cancel_graph` RPC method continues to cancel a running graph (existing behaviour; regression-tested by any existing Orcha test that exercises cancellation, or a new test if none exists).
4. `DynamicHub`'s per-request token construction and cancellation is covered by at least one unit test.
5. The `CancellationToken` row in `plans/README.md`'s cross-epic contracts table is updated with the chosen mechanism (A / B / coarser) and the latency budget (500 ms or 2 s) in the same commit.
6. All existing `cargo test` targets pass.

## Completion

Implementor delivers:

- Transport-side disconnect → token.cancel wiring.
- `DynamicHub` per-request token plumbing.
- Per-activation `select!` / `cancelled()` observation points.
- Orcha `cancel_registry` reconciliation (migration or layering — decision recorded in the PR body).
- README update per criterion 5.
- Integration test per criterion 1.
- `cargo test` output confirming criterion 6.
- Status flip to `Complete` in the same commit that lands the code.
