---
id: RL-S01
title: "Spike: cancellation-token propagation mechanism through hub and stream loops"
status: Pending
type: spike
blocked_by: []
unlocks: [RL-9, RL-10]
severity: High
target_repo: plexus-substrate
---

## Question

Does the hub own and inject a single `CancellationToken` per request that flows through `PlexusStreamItem` poll loops into activation work, so that a cancelled request terminates the activation's work within **500 ms** of cancellation being signalled?

(N = 500 ms chosen as a reasonable upper bound for an in-process cancellation round-trip against a cooperative poll loop running on tokio. The budget is the single tunable — if the passing approach exceeds 500 ms, treat as FAIL and escalate to the fallback spike variant.)

## Setup

1. In a throwaway branch off `main`, pick one existing activation that already runs a multi-step stream path. Candidates, in order of preference: Echo (simplest; no storage side effects), Cone (has a real poll loop against an LLM-shaped producer), Orcha (most realistic but has its own `cancel_registry` — avoid unless Echo and Cone are insufficient).
2. Extend `DynamicHub` (or its equivalent dispatch entry point) to construct one `tokio_util::sync::CancellationToken` per incoming request.
3. Thread the token to the chosen activation's method handler via whatever parameter shape the spike picks. Two plausible shapes — try option A first:
   - **Option A.** Add a `CancellationToken` parameter to the activation trait's method signature, alongside existing parameters.
   - **Option B.** Store the token in a request-scoped context (e.g., a `tokio::task_local!` or a new field on the activation's per-call context struct) and retrieve it inside the handler.
4. Inside the chosen activation's poll loop, observe the token via `select! { _ = token.cancelled() => break, item = next_item() => yield item }` (or the equivalent for Option B).
5. Build a small harness (test binary or `cargo test`) that:
   a. Starts a substrate with only the chosen activation registered.
   b. Issues a streaming call that is designed to emit a new `PlexusStreamItem` every 50 ms indefinitely.
   c. At t = 250 ms after stream start, cancels the client side (or calls `token.cancel()` directly on the hub-side token through a test hook).
   d. Measures wall-clock time from cancel-signal to the activation's poll loop returning (i.e., the spawned handler future completing or being dropped).
6. Record the measurement. Repeat 10 times. Take the max.

## Pass condition

Across 10 runs, the **maximum** measured cancel-signal-to-handler-return time is **≤ 500 ms**, AND the test harness observes no lingering spawned tasks via `tokio-console` (or equivalent — check for live tasks from the chosen activation after the measurement window).

Binary: max ≤ 500 ms AND no lingering task → **PASS**. Either condition violated → **FAIL**.

## Fail → next

If Option A fails (e.g., the trait signature change is too invasive, or the activation cannot be modified to honour the token within the budget), rerun the spike with Option B (task-local / request-scoped context). Repeat the measurement procedure. Same pass condition.

If both A and B fail: rerun with a coarser mechanism — an `AbortHandle` on the spawned handler task, abandoning in-flight work mid-step rather than cooperative cancellation. Document that recovery of partial state becomes the caller's responsibility and loses the cleanliness of cooperative cancellation.

## Fail → fallback

If all three variants exceed the 500 ms budget on at least one of the 10 runs: accept cancellation-as-best-effort. RL-9 still wires the token through the code but the "within N ms" acceptance criterion on RL-9 becomes "within 2 s" and a separate follow-up ticket is opened for tightening the budget. This is a real degradation — surface it in the spike's completion report and in RL-9's Context section.

## Time budget

Four focused hours. If the spike exceeds this, stop and report regardless of pass/fail state — the budget overrun itself is signal, and it means RL-9's implementation is riskier than assumed.

## Out of scope

- Cancellation propagation across **networked** transport (e.g., websocket teardown on disconnect triggering hub-side cancel). This spike measures the in-process path. Transport integration is RL-9's scope — the spike only confirms the in-process mechanism works end-to-end once the transport side has delivered a cancel signal.
- Per-activation customization of cancellation semantics (priority, partial cancel, resume). The spike answers only "does a single token mechanism reach the poll loop in time?"
- Replacing Orcha's `cancel_registry`. RL-9 decides how to reconcile — the spike does not.
- Graceful shutdown. RL-10's scope.

## Completion

Spike delivers: a single commit to a throwaway branch (not merged) with:

- The hub-side token construction + injection patch.
- The activation-side poll loop modification.
- The measurement harness.
- A one-paragraph result block pinning which variant (A / B / coarser) passed, the max measured latency across 10 runs, the lingering-task check result, and the time spent.

The result block is copy-pasted into RL-9's Context section before RL-9 is promoted to Ready. If the spike lands in the fallback regime (≤ 2 s rather than ≤ 500 ms), the degradation is recorded in RL-1 ("Open coordination questions") and in RL-9's acceptance criteria.
