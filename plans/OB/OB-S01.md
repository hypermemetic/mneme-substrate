---
id: OB-S01
title: "Spike: streaming protocol versioning strategy"
status: Pending
type: spike
blocked_by: []
unlocks: [OB-6]
severity: High
target_repo: plexus-substrate (+ possibly plexus-core for wire-format surface)
---

## Question

Which of the three candidate versioning strategies for `PlexusStreamItem` produces a working old-client / new-server interop (old client unaffected) **and** a working new-client / new-server capability negotiation (new client can detect and use a new variant) under a single, minimal prototype? The passing strategy is the one OB-6 implements.

Binary pass: one strategy is prototyped end-to-end against a running substrate; the interop matrix (below) behaves as specified; the other two strategies are not attempted unless this one fails.

## Context

`PlexusStreamItem` is serialized as JSON at the wire boundary. Today it has no version discriminator — adding a new variant (e.g., `ToolUseStarted`) silently breaks older clients that deserialize the union with `deny_unknown_fields` or similar strict patterns. The audit calls this out under "Streaming protocol has no versioning".

Three candidate strategies, to be compared one at a time:

**Strategy 1 — `v:` field on every stream item.**

Every serialized `PlexusStreamItem` gains a top-level `"v": N` field. Clients read the field, dispatch on it. Adding a new variant bumps `v` for items that use new fields; old clients see the new `v` and skip (or log-and-skip) items they don't understand. Simplest. Most JSON-RPC-shaped.

**Strategy 2 — Feature handshake at connection.**

On connection (or first method call), client and server exchange a capability set. Server knows which stream variants the client supports. Server down-converts / omits stream items the client won't understand. Adding a new variant adds a capability string; old clients omit the string, old servers ignore unknown capability strings. Richer but requires state on both ends.

**Strategy 3 — Per-method negotiation.**

Each streaming method declares its accepted item-version in its schema. Client reads the schema, picks the highest version it understands, passes it as a parameter. Server emits items matching that version. Most fine-grained but invasive — every streaming method's signature changes.

**Test bed.** A throwaway streaming method on a test activation (or a dummy variant injected into an existing stream) emits one item under the new scheme. Two clients drive it:

| Client | Expected behavior |
|---|---|
| **Old client** (compiled against pre-versioning substrate) | Connects to versioned server; existing stream calls continue to work; unknown new variants are either skipped silently or logged-without-crash — never panic, never corrupt subsequent items in the stream. |
| **New client** (compiled against versioned substrate) | Connects to versioned server; observes the new variant; parses it; exposes it to application code. |

A third matrix cell — **new client against old server** — tests graceful degradation: new client detects (via `v` absence, missing capability in handshake, or missing method annotation) that the server does not support the new variant, and continues working with the older shape.

## Setup

Pick **Strategy 1** (`v:` field) first — it's the lowest-LOC prototype and matches how most JSON-RPC-shaped protocols version. If it passes, stop.

**Strategy 1 prototype:**

1. In a throwaway branch of `plexus-core` (or wherever `PlexusStreamItem` is defined), add `v: u32` to the serialization shape of `PlexusStreamItem`. Pin the current set of variants at `v = 1`. Add one synthetic new variant (e.g., `VersioningProbe { note: String }`) at `v = 2`.
2. Update the serializer so every emitted item includes `"v": N` where N is that variant's declared version.
3. Update the deserializer so unknown `v` values produce a "skip this item" result rather than a hard parse error. Logged via `tracing::warn!`.
4. Build substrate against the patched `plexus-core`.
5. Write a minimal **old client** simulator: a short Rust or Python script that connects to substrate, calls a streaming method (e.g., `echo.stream` or a test stream), and asserts it receives the existing variants without error. Do **not** rebuild the client against the new `plexus-core` — link it against the unpatched shape (simulate by stripping `v` handling from the client's deserializer, or by using raw `serde_json::Value` and ignoring `v`).
6. Write a minimal **new client** simulator: connects, drives a stream that emits the new `VersioningProbe` item, receives and asserts it.
7. Run both clients against the same substrate. Record outcomes.

**Old-server matrix cell:**

8. Run the old-client simulator against **unpatched** substrate (the HEAD substrate without the spike's changes). Confirm the old client still works — this is the control.
9. Run the new-client simulator against unpatched substrate. Expected: the `VersioningProbe` variant is never emitted, and the new client observes this (no crash, no hang). If the new client crashes because it demands `v` be present on every item, it's not graceful-degradation ready — the spike fails that cell.

**If Strategy 1 passes all three cells, the spike passes and OB-6 implements Strategy 1.**

**If Strategy 1 fails a cell,** attempt **Strategy 2** (feature handshake):

1. Add a `server_capabilities` method to the Plexus RPC protocol (or extend an existing handshake mechanism). Server responds with a set of capability strings (e.g., `"stream.variant.tooluse_started"`).
2. Client sends its own supported-capability set at connection time (or on first call).
3. When emitting stream items, server skips (or down-converts) items the client hasn't declared support for.
4. Rerun the interop matrix.

**If both 1 and 2 fail,** attempt **Strategy 3** (per-method negotiation):

1. Add a `version` parameter to one streaming method's signature.
2. Server reads the parameter and emits items matching that version's schema.
3. Rerun the interop matrix.

## Pass condition

**Binary — the spike passes when one strategy satisfies all three interop matrix cells:**

| Cell | Pass means |
|---|---|
| Old client / new server | Stream call completes. Existing variants received and parsed. Unknown new variant is skipped (or logged) without crashing the stream. |
| New client / new server | Stream call completes. New variant is received, parsed, and available to application code. |
| New client / old server | Stream call completes. Client observes the absence of the new variant (via missing `v` field, missing capability, or missing method annotation — depending on strategy). No crash. No hang. |

The first strategy to pass all three cells wins. OB-6 implements that strategy.

**If a strategy passes cells 1 and 2 but fails cell 3,** it is a **partial pass** — viable only if substrate never needs to serve clients newer than itself. Partial passes are documented and the planner decides whether to accept (noting the limitation in OB-6) or escalate to the next strategy.

## Fail → next

**Strategy 1 fails:** move to Strategy 2 (feature handshake). Document why 1 failed (e.g., "old client's JSON deserializer has `deny_unknown_fields` set and panics on `v`; updating that is a breaking change").

**Strategy 2 fails:** move to Strategy 3 (per-method negotiation).

**Strategy 3 fails:** the spike **fails** overall. Document the specific blocker and escalate to planner review. OB-6 is blocked until the blocker is resolved or the strategy set is expanded.

## Fail → fallback

If all three strategies fail under budget: the fallback is **"freeze the protocol and require synchronized rollouts"** — explicitly document that adding a new `PlexusStreamItem` variant is a breaking wire change, bump `plexus-protocol` major version when it happens, coordinate client and server updates together. This is the status quo made explicit, not a new capability. OB-6 then lands as a documentation-only ticket: a CHANGELOG convention, not a wire feature.

## Time budget

4 hours total. Strategy 1 prototype should be 30–90 minutes; Strategy 2 roughly 2 hours if Strategy 1 fails; Strategy 3 is out of scope under budget unless both 1 and 2 fail fast with informative failures.

Stop and report regardless of state at 4 hours.

## Out of scope

- **Implementing the chosen strategy in production code.** OB-6 does that. OB-S01 only prototypes and decides.
- **Versioning non-streaming methods.** Request/response shapes for scalar methods are unchanged; this spike concerns `PlexusStreamItem` only.
- **Designing a full capability-negotiation protocol.** Strategy 2, if chosen, ships with a minimal shape. Broader capability negotiation (auth, feature gates, etc.) is a future concern.
- **Cross-language client compatibility.** The spike drives substrate from two Rust/Python-shaped simulators. Real-world clients in other languages inherit the chosen strategy's JSON shape; their parser behavior is out of scope here.
- **Backward compatibility for `plexus-protocol` below its current major.** The spike assumes the current `plexus-protocol` version as baseline.

## Completion

Spike delivers:

- Throwaway branch(es) with the prototype code.
- A one-page report (added to this ticket body or linked from it) stating: which strategy passed, the three interop matrix cell outcomes, measured wall-clock time, any blockers encountered.
- A pinned decision: "OB-6 implements Strategy {1|2|3}" OR "OB-6 documents the frozen-protocol fallback".
- No merge to main. OB-6 inherits the findings and references them in its Context section before being promoted to Ready.
