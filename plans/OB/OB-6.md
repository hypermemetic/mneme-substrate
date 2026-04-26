---
id: OB-6
title: "Streaming protocol versioning — implementation"
status: Pending
type: implementation
blocked_by: [OB-S01]
unlocks: []
severity: High
target_repo: plexus-substrate (+ plexus-core for wire-format surface)
---

## Problem

`PlexusStreamItem` is JSON-serialized at the wire boundary with no version discriminator. Adding a new variant silently breaks older clients that deserialize the union with strict patterns (`deny_unknown_fields`, exhaustive `match` on an enum tag). The audit names this under "Streaming protocol has no versioning". Until a versioning scheme is wired, the streaming protocol is frozen by omission — every new variant is a potential silent client break.

This ticket implements the versioning strategy chosen by OB-S01. It makes `PlexusStreamItem` wire-format versioned end-to-end: producer emits version-tagged items, consumer handles unknown versions gracefully, and the interop matrix from OB-S01 (old client / new server, new client / new server, new client / old server) passes in production substrate and `plexus-core` — not just the throwaway spike branch.

## Context

This ticket is **deferred-content** with respect to strategy. The strategy is pinned by OB-S01:

- **If OB-S01 picked Strategy 1 (`v:` field):** OB-6 adds `v: u32` to the serialization shape of `PlexusStreamItem`. Each variant declares its version. The serializer always emits `v`; the deserializer tolerates unknown versions (skip-and-log).
- **If OB-S01 picked Strategy 2 (feature handshake):** OB-6 adds a capability-negotiation handshake on connection. Server remembers the client's capability set. Stream producers consult it before emitting version-gated variants.
- **If OB-S01 picked Strategy 3 (per-method negotiation):** OB-6 adds a `version` parameter (optional, defaulting to v1) to every streaming method. Server emits items matching the requested version.
- **If OB-S01's fallback was taken (frozen protocol):** OB-6 lands as a **documentation-only** ticket: CHANGELOG convention, deprecation policy alignment, and explicit text that new variants require a `plexus-protocol` major-version bump with coordinated client/server rollout.

The ticket's concrete scope cannot be finalized until OB-S01 completes. This ticket template is written to be refactored in-place once OB-S01 pins the strategy.

**Placeholder assumption (pending OB-S01):** Strategy 1 is the most likely outcome per the spike's ordering. The acceptance criteria below are written for Strategy 1 and will be rewritten if OB-S01 picks another strategy.

## Required behavior (Strategy 1 — placeholder until OB-S01 confirms)

### Serialization

Every `PlexusStreamItem` emitted over the wire carries a `v: u32` field at the top level of its JSON representation.

| Variant | Declared version |
|---|---|
| All existing variants at time of OB-6's landing | `v = 1` |
| New variants added after OB-6 lands | `v = 2` or higher, per the variant's introduction version |

Version numbers are monotonic — they never shrink. A version mapping is committed alongside the code (e.g., `plexus-core/docs/stream-versions.md`) listing which variants belong to which version.

### Deserialization

| Consumer receives | Consumer behavior |
|---|---|
| Item with known `v` and known variant | Parse normally. |
| Item with known `v` but unknown variant | Skip the item; log `tracing::warn!` with the raw variant tag. |
| Item with unknown `v` | Skip the item; log `tracing::warn!` with the observed `v`. |
| Item with no `v` field (pre-versioned server) | Treat as `v = 1`; parse existing variants; unknown variants skip-and-log. |

Skip-and-log never terminates the stream. The consumer continues receiving subsequent items.

### Producer behavior

The producer (substrate's stream emission path) always stamps `v` on every item. When a variant is introduced at `v = 2`, substrate still emits older variants at `v = 1` and new variants at `v = 2` — the version is per-variant, not per-session.

### Backward-compat matrix

| Direction | Expected |
|---|---|
| Old client → new server (OB-6-landed substrate) | Stream continues working. Pre-OB-6 client sees `v` field it may not explicitly handle — tolerated because the client extracts variant by tag, not by `v`. New variants the old client doesn't know about surface as unknown-variant deserialization errors **for that item only**, not the stream as a whole (mitigation depends on the client's existing unknown-tag handling; if synapse or cllient crashes on unknown tags today, a coordinated client-side fix ships alongside). |
| New client → new server | Stream produces v1 and v2 items. Client parses both. |
| New client → old server (pre-OB-6 substrate) | Stream produces items without `v`. Client treats missing `v` as `v = 1`. Client does not expect v2-only variants. No crash. |

### `plexus-protocol` / `plexus-core` version coordination

- Landing OB-6 bumps `plexus-protocol` minor version (additive — `v` field is opt-in-tolerated on consume; producer-side is backward-compatible via the "no `v` means v=1" rule).
- CHANGELOG entry names this as the versioning landing.
- Future variants that require a new `v` value bump only their activation's version, not the protocol — new variants are additive under the scheme.

## Risks

| Risk | Mitigation |
|---|---|
| OB-S01 picks a strategy that invalidates most of this ticket's scope. | This ticket is rewritten post-S01. The current text is a placeholder for Strategy 1; if another strategy wins, acceptance criteria and required-behavior sections are replaced. |
| Synapse (Haskell) and cllient (Rust) parse `PlexusStreamItem` in their own decoders. Updating every client in sync is a coordination tax. | Strategy 1 is chosen partly to minimize this — adding `v` as an ignored field requires no client changes. A follow-up per-client ticket adds the skip-and-log behavior for unknown variants, but this is not a blocker for OB-6. |
| The current JSON shape has `PlexusStreamItem` serialized as a **tagged enum** via `#[serde(tag = "type")]`; adding a `v` field at the top level may conflict with the tag. | `serde` tolerates both `tag` and arbitrary additional fields at the same level. Re-verify at implementation — if it doesn't, the shape wraps as `{"v": 1, "type": "...", ...rest}` via a custom `Serialize` / `Deserialize` impl. |
| Unknown-`v` skip-and-log is too lenient — a misbehaving producer could spew garbage that a strict consumer would want to surface. | Add a `strict: bool` config option (or per-consumer API) that flips unknown-`v` handling from skip-and-log to error-and-close. Default is lenient (skip-and-log). |
| Item-level versioning misses session-level versioning needs (e.g., the stream-envelope itself evolves). | Out of scope. OB-6 versions item-level payloads. Envelope changes are a separate future decision. |
| Test coverage requires driving actual streams. | Substrate's existing streaming tests cover `echo.stream`, ClaudeCode's chat, Orcha's run_graph. Extend those tests with a synthetic "injected v=999 variant" fixture that exercises the skip-and-log path. |

## What must NOT change

- Non-streaming RPC request/response shapes.
- `PlexusStreamItem`'s existing variant tags, payload shapes, or semantic meaning of each variant.
- The underlying transport (WebSocket JSON-RPC or whatever current shape).
- Client-observable behavior for streams that produce only existing (v1) variants. Such streams behave identically pre- and post-OB-6.
- Existing error envelope (OB-5 handles error shape; OB-6 does not touch it).

## Acceptance criteria

*(Acceptance criteria below assume Strategy 1. Rewrite with OB-S01's chosen strategy.)*

1. Every `PlexusStreamItem` serialized by substrate includes a `v: u32` field. Verifiable by driving any streaming method and inspecting the raw JSON on the wire.
2. A consumer sending a streaming request and receiving a synthetic item with `v: 999` (injected via test harness) logs a warning and continues receiving subsequent items. Stream does not terminate.
3. A consumer receiving an item with `v: 1` but an unknown variant tag logs a warning and continues. Verified via test harness injecting a fake variant.
4. A pre-OB-6 client (simulated by a test that strips `v` before processing) receives a stream from OB-6-landed substrate and processes existing variants without error.
5. A post-OB-6 client receiving a stream from pre-OB-6 substrate (simulated by a test that emits items without `v`) treats missing `v` as 1 and processes normally.
6. `plexus-core` declares `PlexusStreamItem`'s wire serialization (with `v` field) in a documented module; the variant-to-version mapping lives in a committed document (e.g., `plexus-core/docs/stream-versions.md`).
7. `cargo test --workspace` passes. Existing streaming tests unchanged; new tests cover the three interop matrix cells.
8. `plexus-protocol` minor version bumped; CHANGELOG entry landed.
9. Substrate's `/metrics` output (if OB-3 has landed) includes a `substrate_stream_unknown_version_total` counter for observability of skip-and-log events.
10. Synapse and cllient rendering of streams confirmed unaffected (no crashes, no visible rendering change) — verified by smoke test against a substrate emitting v1 items.

## Completion

PR against `plexus-substrate` (+ `plexus-core`). CI green. Status flipped from `Ready` to `Complete` in the same commit. OB-6's landing closes the epic — the full OB-1..OB-6 set is Complete and the audit's "Missing systems" section retires Config, Metrics, Pagination, Structured error context, and Streaming protocol versioning.

**Note to implementor:** re-read OB-S01 before starting. If S01 picked a strategy other than Strategy 1, rewrite the Required behavior and Acceptance criteria sections to match the chosen strategy. The Problem, Context, Risks (with strategy-specific adjustments), and What must NOT change sections remain mostly unchanged across strategies.
