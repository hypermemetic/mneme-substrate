---
id: RUSTGEN-S02
title: "Spike: WebSocket transport dependency — tokio-tungstenite vs jsonrpsee vs bundled"
status: Pending
type: spike
blocked_by: []
unlocks: [RUSTGEN-4]
severity: High
target_repo: hub-codegen
---

## Question

Which WebSocket + JSON-RPC dependency does the Rust runtime use, and does it roundtrip a real method call against a live substrate?

Current Rust backend (`hub-codegen/src/generator/rust/client.rs`) emits code depending on:

- `tokio-tungstenite = "0.21"` — WebSocket
- `serde_json = "1.0"` — JSON serialization
- `async-stream = "0.3"` — stream helpers
- `futures = "0.3"` — Stream / Sink
- `tokio = "1.0"` — async runtime

This works (generated code compiles) but nobody has verified it actually connects to a live substrate end-to-end. The method call must:

1. Establish a WebSocket connection to substrate.
2. Send a JSON-RPC framed request (id, method, params).
3. Read streamed `PlexusStreamItem` responses.
4. Terminate on `Done` or `Error`.

Three candidates:

| Option | Deps | Trade-off |
|---|---|---|
| **A. Status quo: tokio-tungstenite** | Current deps as above. | Already emitted; minimal change. Hand-rolled JSON-RPC framing may have bugs. |
| **B. jsonrpsee** | `jsonrpsee = "0.22"` with client + WebSocket features. | Mature JSON-RPC lib; handles framing, subscriptions, batching. Larger dep tree; possibly opinionated about transport-layer errors in ways that don't match substrate's stream semantics. |
| **C. Bundled — hand-roll on top of tokio-tungstenite** | Same deps as A, but with proper framing code vendored in. | Full control over framing; matches substrate's exact stream protocol. More surface area to maintain. |

## Setup

1. Start a local substrate (`cargo run --bin plexus-substrate` from the substrate repo).
2. Pick one method with a non-trivial return — e.g., `solar.list_bodies` or `arbor.list_docs`.
3. For each of options A, B, C, write a standalone spike program (`hub-codegen/spike/rustgen-s02/opt_<a|b|c>/src/main.rs`) that:
   - Connects to `ws://localhost:<substrate_port>`.
   - Sends a JSON-RPC request for the chosen method.
   - Collects all `PlexusStreamItem` responses until `Done` or `Error`.
   - Prints the result count (e.g., number of `Data` items received).
4. Run each spike program against the live substrate. Record the transcript.

## Pass condition

Binary: the first option satisfying ALL of these passes —

- [ ] `cargo run` on the spike program exits 0.
- [ ] The program prints a non-zero count of `PlexusStreamItem::Data` items (or `Done` immediately for a method that returns nothing — choose a method that returns at least one item for the spike).
- [ ] Total dep tree count (`cargo tree | wc -l`) is under 250 lines.
- [ ] Connection establishes and terminates cleanly — no hung futures, no `panic!`, no `unwrap` failures.

If option A passes: status quo stays, RUSTGEN-4 codifies the current dep set.

If option B passes and A fails: RUSTGEN-4 switches to `jsonrpsee`.

If option C passes and A/B fail: RUSTGEN-4 codifies hand-rolled framing vendored into the runtime.

## Fail → next

If option A (tokio-tungstenite status quo) fails, try option B (jsonrpsee). If B fails, try option C (hand-rolled bundled).

Specific failure modes to diagnose before declaring an option failed:

- Dep resolution error → deps issue, not transport issue. Fix deps, re-attempt.
- Connection-refused → substrate isn't running. Not a spike failure.
- Handshake failure → WebSocket protocol mismatch. This is a real failure.
- JSON-RPC request rejected by substrate → framing bug. Real failure.
- Stream never terminates → protocol mismatch in `Done`/`Error` handling. Real failure.

## Fail → fallback

If all three options fail to roundtrip against live substrate, the substrate-side transport is the problem, not the client. Escalate with a specific reproducer: "substrate does not respond to `<method>` called via `<option>` because `<observed behavior>`". The fallback is to block RUSTGEN-4 pending a substrate-side fix (separate ticket, separate epic).

## Time budget

Four focused hours. Includes substrate startup and fixture-method selection. If the spike exceeds this, stop and report.

## Out of scope

- TLS / `wss://` transport. Local `ws://` is sufficient for the spike.
- Authentication. Substrate currently accepts unauthenticated connections in dev mode.
- Reconnection / retry logic. The spike tests single-call roundtrip; reconnection is a follow-up concern.
- Streaming performance benchmarking. Correctness only.
- Bidirectional method roundtrip (substrate → client messages). Out of RUSTGEN's scope.

## Completion

Spike delivers:

- A spike directory `hub-codegen/spike/rustgen-s02/` containing `opt_a/`, `opt_b/`, `opt_c/` (only the ones attempted) as standalone Cargo workspaces or a small workspace with three bins.
- A `DECISION.md` in the spike dir documenting: which option passed, `cargo run` transcript for the passing option (showing the non-zero Data item count), dep tree summary (`cargo tree` output), and rationale for rejecting others.
- Pass/fail result captured in the decision doc.
- Time spent.

Report lands in RUSTGEN-4's Context section as a reference before RUSTGEN-4 is promoted to `Ready`. The decision pins the transport dep choice for RUSTGEN-4 and downstream.
