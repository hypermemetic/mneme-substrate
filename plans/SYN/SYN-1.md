---
id: SYN-1
title: "Synapse consumes the CHILD epic — capability-aware discovery and completion"
status: Epic
type: epic
blocked_by: [CHILD-2, CHILD-3, CHILD-4, CHILD-7]
unlocks: []
---

## Goal

End state: synapse (the Plexus RPC CLI at `~/dev/controlflow/hypermemetic/synapse/`) drives the `#[plexus_macros::child]` pattern end-to-end. When a user runs `synapse solar`, synapse introspects Solar's new hub-shape schema, sees the static children via `plugin_children`, sees the dynamic child gate (`solar body {name}`), offers tab-completion via `list_children` when the activation opts in via `ChildCapabilities::LIST`, and offers search via `search_children` when `SEARCH` is set. Help text flows from `///` doc comments surfaced in `plugin_schema.description`.

This epic is the "does the CHILD epic actually work for humans" gate. Until it ships, the CHILD epic is only verifiable via unit tests — not via the CLI users actually touch.

## Context

Synapse is a Haskell CLI (`cabal.project` at the repo root). It connects to any Plexus RPC server (WebSocket on port 4444 by default) and offers method discovery, invocation, help, and — the part that matters here — tab-completion over the activation tree.

CHILD epic delivered (Rust side):
- `ChildRouter::capabilities()` → `ChildCapabilities` bitflags (LIST, SEARCH)
- `ChildRouter::list_children()` / `search_children(query)` → `Option<BoxStream<String>>`
- `#[plexus_macros::child]` attribute generates the above and hub-mode inference
- `///` doc comments flow into `plugin_schema.description`

Open question resolved in SYN-2: are `capabilities`, `list_children`, `search_children` callable as wire-level RPC methods today, or do we need to expose them explicitly in `plexus-core` / substrate first?

## Dependency DAG

```
       CHILD-7 (Solar migration)
              │
              ▼
       SYN-2 (synapse consumes new wire methods)
              │
              ▼
       SYN-3 (integration smoke test against Solar)
```

SYN-2 may spawn a prerequisite ticket (call it SYN-S01) if the new ChildRouter methods aren't yet exposed at the wire level — that spike lands before SYN-2's implementation.

## Phase Breakdown

| Phase | Tickets | Notes |
|---|---|---|
| 1. Wire exposure check + impl | SYN-2 (possibly SYN-S01 spike first) | Surface `capabilities`, `list_children`, `search_children` as RPC methods if they're not already. |
| 2. Synapse consumer | SYN-2 (same ticket, synapse side) | Synapse calls the new methods during tree rendering and completion. |
| 3. Integration | SYN-3 | End-to-end smoke test with migrated Solar. |

## Tickets

| ID | Summary | Target repo | Status |
|---|---|---|---|
| SYN-1 | This epic overview | — | Epic |
| SYN-2 | Synapse consumes `capabilities` / `list_children` / `search_children` for capability-aware tree rendering and tab-completion | synapse (+ possibly plexus-core/substrate) | Pending |
| SYN-3 | End-to-end integration smoke test — migrated Solar drivable from synapse with tab-completion | synapse + substrate (integration) | Pending |

## Out of scope

- **Remote children traversal.** Cross-node hub → hub → hub traversal is orthogonal to capability consumption; tracked separately if it comes up.
- **Descriptions on dynamic-child gates.** The `{placeholder}` rendering is covered in SYN-2; richer gate annotations (e.g., showing the accepted-name type) are follow-ups.
- **Cycle detection enhancements.** Synapse already does cycle detection per prior conversation; this epic doesn't re-examine that.
- **Replacing synapse's current plugin_schema consumption.** Existing schema-driven rendering stays; SYN-2 adds the new capability-aware methods alongside.

## What must NOT change

- Existing synapse behavior for activations that don't use `#[child]` (all 15 substrate activations today). They continue to render and dispatch exactly as before.
- Wire-format compatibility: the new methods must be additive; no existing RPC call's request/response shape changes.
- Solar's pre-migration wire behavior was preserved by CHILD-7; post-SYN-2 synapse must observe identical behavior for everything except the new capability-driven features.

## Completion

Epic is Complete when SYN-2 and SYN-3 are both Complete, and a demo transcript showing `synapse solar<TAB>` auto-completing to planet names (from `list_children`) is captured in SYN-3's PR.
