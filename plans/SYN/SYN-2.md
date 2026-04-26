---
id: SYN-2
title: "Synapse consumes capabilities / list_children / search_children"
status: Pending
type: implementation
blocked_by: [CHILD-7]
unlocks: [SYN-3]
severity: High
target_repo: synapse (+ possibly plexus-core/substrate for wire exposure)
---

## Problem

The CHILD epic gave Rust-side activations opt-in listing and searching for their child sets. Synapse (the CLI users actually touch) doesn't yet consume these. A user running `synapse solar` cannot tab-complete planet names, cannot search planets, cannot distinguish a hub activation's dynamic child gate from a static child. Until synapse reads `capabilities()` and conditionally calls `list_children` / `search_children`, the CHILD epic is invisible to end users.

Additionally, whether these three methods are callable at the Plexus RPC **wire level** today is unverified. The Rust trait exists, but RPC exposure may require explicit surfacing. This ticket resolves that question as its first step.

## Context

Affected components:

| Component | Role |
|---|---|
| `plexus-core` (`~/dev/controlflow/hypermemetic/plexus-core`) | Defines `ChildRouter` trait — unchanged, but wire exposure may need adding here or in substrate |
| `plexus-substrate` (`~/dev/controlflow/hypermemetic/plexus-substrate`) | Hosts activations; may need to expose the methods at the wire as part of `DynamicHub` routing |
| `synapse` (`~/dev/controlflow/hypermemetic/synapse`) | Haskell CLI consuming the wire protocol |

`ChildCapabilities` bitflags:
- `LIST` — `list_children()` returns `Some(stream)` yielding child names
- `SEARCH` — `search_children(query)` returns `Some(stream)` yielding matching names

Expected wire-call shape (to be confirmed in step 1):

| Method | Params | Result |
|---|---|---|
| `<namespace>.capabilities` | none | Bitflags-like value (JSON number or `{"list": bool, "search": bool}`) |
| `<namespace>.list_children` | none | Stream of strings (or `null` if capability not set) |
| `<namespace>.search_children` | `{"query": "..."}` | Stream of strings (or `null` if capability not set) |

## Required behavior

### Step 1: wire-exposure check

Before synapse changes, verify the three methods are callable over Plexus RPC for any `#[plexus_macros::child]`-based hub. Test by connecting to a running substrate (post-CHILD-7) and calling `solar.capabilities` via raw JSON-RPC. Three outcomes:

| Outcome | Next step |
|---|---|
| All three methods respond with structured data | Proceed to step 2. |
| One or more methods return "method not found" | Add wire exposure in plexus-core or plexus-macros. This becomes a prerequisite sub-ticket (SYN-S01 — create and land first). |
| Methods respond but with unclear shape | Pin the response shape in this ticket before moving on. |

### Step 2: synapse integration

For every activation synapse discovers:

| Signal | Synapse behavior |
|---|---|
| `plugin_schema.is_hub()` is `true` | Recurse into children |
| `plugin_children` contains a static summary | Render as a named node in the tree |
| Dynamic-child gate exists (inferred from schema or from `list_children` capability) | Render as `<namespace> {name}` placeholder entry |
| `capabilities` includes `LIST` | Tab-completion on the child-name slot calls `list_children` and offers the returned names |
| `capabilities` includes `SEARCH` | Typed-prefix completion calls `search_children(prefix)` for filtering |
| `capabilities` neither | Child names not enumerable — placeholder stays visible, tab returns nothing |
| `plugin_schema.description` non-empty | Surface as help text for that node |

### Step 3: error handling

If `list_children` or `search_children` responds with `null` (capability not set despite advertisement), synapse logs a warning and treats the capability as absent for that invocation. Synapse does not crash.

## Risks

| Risk | Mitigation |
|---|---|
| Wire exposure turns out to be nonexistent and requires substantial plexus-core work | SYN-S01 spike resolves before SYN-2 implementation. If the fix is nontrivial, surface the scope inflation and stop for planner review. |
| Streaming wire format for `list_children` differs from scalar-method convention | Synapse already handles Plexus RPC streaming responses (PlexusStreamItem). Reuse that infrastructure; don't invent a new path. |
| Tab-completion latency if `list_children` is slow | Synapse caches per-session per-activation list results with a short TTL (e.g., 5s). Stale lists are acceptable for tab-completion. |
| A busy or remote hub hangs on `list_children` | Timeout the call (e.g., 2s) and fall back to "no completions available" — do not block the user. |

## What must NOT change

- Existing synapse behavior for non-hub activations and for hubs using the legacy `hub` + hand-written `ChildRouter` pattern (e.g., Solar pre-CHILD-7). They render and dispatch exactly as before.
- Wire protocol for any existing RPC method. Additions only.
- Command-line invocation patterns (`synapse <namespace> <method> <args>`). Unchanged.
- Synapse's cycle-detection behavior during traversal.

## Acceptance criteria

1. Step 1 completed and its outcome recorded in the PR description (methods wire-exposed already, or SYN-S01 filed and landed as prerequisite).
2. Synapse connects to a substrate running migrated Solar (CHILD-7 landed) and:
   - `synapse solar` output includes an entry rendered as `body {name}` (the dynamic child gate), not hidden.
   - Tab-completion on `synapse solar body <TAB>` offers planet names matching what `solar.list_children` returns. Verified against the known planet list from `build_solar_system()`.
   - `synapse solar body mercury` resolves and the body's methods are reachable (`synapse solar body mercury info` returns mercury's info).
   - `synapse solar body Mercury` (capitalized) also resolves (case-insensitive preserved by CHILD-7's method body).
3. On an activation with only static `#[child]` methods (e.g., a test fixture), synapse renders each static child as a named node in the tree.
4. On an activation with a dynamic `#[child]` method but NO `list = "..."` opt-in, synapse renders the gate `{name}` entry and tab-completion at that slot yields no completions (no crash, no error message popup).
5. Help text shown for any node comes from the `///` doc comment that produced its `plugin_schema.description`.
6. Synapse unit or integration tests (Haskell side) cover: capability-matrix rendering, `list_children` tab-completion, `search_children` prefix-filter completion, graceful handling when capability is absent.
7. Existing non-hub and legacy-hub activations (Arbor, Cone, ClaudeCode, etc.) render unchanged — verified by a synapse smoke test against unmigrated substrate.

## Completion

PR(s) against `synapse` (and `plexus-core` / `plexus-substrate` if SYN-S01 was needed). CI green on synapse. Demo transcript captured in PR description showing `synapse solar<TAB>` expanding to the planet list. Ticket status flipped from `Ready` to `Complete` in the same commit as the synapse code change.
