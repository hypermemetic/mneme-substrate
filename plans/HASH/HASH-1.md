---
id: HASH-1
title: "Runtime hash aggregation — remove child hashes from schemas"
status: Epic
type: epic
blocked_by: []
unlocks: []
---

## Goal

End state: schemas do not carry child hashes. Hashes are computed at runtime by walking the activation graph, asking each node for its current hash, and aggregating tolerantly — any child that fails to respond (remote, offline, timeout, error) is treated as absent and skipped. The aggregate is deterministic over the present-and-responding set.

This aligns hash semantics with the "graph, not tree" mental model: remote nodes come and go, child sets may be uncountable or policy-gated, and a static snapshot hash baked into a schema is a lie about a fundamentally dynamic property.

## Context

**Today:** `plexus_core::plexus::ChildSummary` has a required `hash: String` field. Hub activations (Solar, and any future hub) populate `hash` at `plugin_children()` time with a deterministic digest of the child's own sub-schema. The schema is a static snapshot. Tests assert `!hash.is_empty()`.

**The problem:** embedding hashes in schemas assumes:
- Children are always present when the schema is read.
- Children are local (or at least synchronously reachable).
- The hash set is fixed at schema-read time.

None of these hold for remote, dynamic, or policy-gated children. A schema's bake-time hashes become stale the moment a remote child changes. A schema whose hash generation requires walking all children synchronously can't be returned promptly when one child is timing out.

**Proposed model:**

- `ChildSummary.hash` becomes optional (or is removed — see HASH-S01).
- Schemas carry names, descriptions, and structural metadata — not content hashes.
- Each activation exposes `async fn plugin_hash(&self) -> Option<String>` returning its current content hash (or `None` if it can't compute one).
- Parents aggregate: walk children via `ChildRouter`, call `plugin_hash()` on each, collect successful responses, sort deterministically (by name), aggregate (e.g., hash the sorted `(name, hash)` pairs).
- Failing child call (timeout, error, `None`, not-reachable) → that child is omitted from the aggregate. The parent's aggregate still commits.

**Relationship to IR epic:** The [IR epic (IR-1)](../IR/IR-1.md) supersedes the schema surgery portion of HASH — reshaping `PluginSchema` / `ChildSummary` and introducing deprecation metadata now lives in IR. What remains in HASH is the runtime `plugin_hash()` aggregation story: trait method on activations, tolerant aggregator over `ChildRouter`, and the demo that proves missing children don't stall the aggregate. HASH-S01's decision narrows accordingly — the "option vs remove" framing is absorbed into IR's `DeprecationInfo { since, removed_in }` convention; HASH-S01 now only pins the runtime contract for `plugin_hash()`.

## Dependency DAG

```
         IR-1 (handles ChildSummary schema surgery + deprecation)
                 │
                 ▼
         HASH-S01 (spike: runtime plugin_hash contract only;
                    schema-field deprecation inherited from IR)
                 │
                 ▼
            HASH-2 (plexus-core: add plugin_hash + aggregate_hash)
                 │
    ┌────────────┼────────────┐
    ▼            ▼            ▼
 HASH-3       HASH-4        HASH-5
 (macros     (substrate    (synapse
  synthesis   test update,  consumption —
  emits       drop Solar    only needed if
  plugin_     hand-written  hashes are
  hash)       plugin_       wire-exposed)
              children)
                 │
                 ▼
             HASH-6
         (retire schema-
          baked hashes —
          follows IR's
          DeprecationInfo
          removed_in plan)
```

HASH-S01 originally owned the "option vs remove" decision for `ChildSummary.hash`. That decision is now inherited from IR's uniform deprecation convention (`DeprecationInfo { since, removed_in }` — see IR-1). HASH-S01's narrowed scope: pin the runtime contract for `plugin_hash()` and tolerant aggregation. HASH-6's retirement schedule follows the `removed_in` plan IR encodes per field; it is no longer an independent version-bump decision.

## Phase Breakdown

| Phase | Tickets | Notes |
|---|---|---|
| 1. Decision | HASH-S01 | Binary: Option vs remove. Pass: one option survives migration review with fewer downstream patches. |
| 2. Foundation | HASH-2 | Add runtime hash mechanism to plexus-core. Tolerant aggregator. |
| 3. Integrations | HASH-3, HASH-4, HASH-5 (parallel) | Macros synthesize `plugin_hash`, substrate tests update, optional synapse wire exposure. |
| 4. Cleanup | HASH-6 | Retire schema-baked hashes on the agreed timeline. |

## Tickets

| ID | Summary | Target repo | Status |
|---|---|---|---|
| HASH-1 | This epic overview | — | Epic |
| HASH-S01 | Spike: backward-compat vs breaking migration for `ChildSummary.hash` | plexus-core | Pending |
| HASH-2 | plexus-core: `plugin_hash()` trait method + tolerant `aggregate_hash()` helper | plexus-core | Pending |
| HASH-3 | plexus-macros: synthesize `plugin_hash()` for `#[plexus_macros::activation]` | plexus-macros | Pending |
| HASH-4 | substrate: drop Solar's hand-written `plugin_children` hash logic; update tests to read runtime aggregate | plexus-substrate | Pending |
| HASH-5 | synapse (if scope grows): surface `aggregate_hash` in the CLI | synapse | Pending |
| HASH-6 | Retire schema-baked hashes — flip `ChildSummary.hash` semantics per HASH-S01's decision | plexus-core | Pending |

## Out of scope

- Cryptographic identity (PKE-strong signing). That's the deferred IDY epic. Hashes here are content/schema digests, not identity.
- Caching and invalidation strategies on the aggregator. HASH-2 keeps it pure — caller caches if they want to.
- Synapse client-side hash verification. If HASH-5 lands, it's read-only display; no consistency checking.

## What must NOT change

- Existing `plugin_schema()` shape during the epic except for the `hash` field semantics — no other schema fields are renamed or reshaped.
- Wire protocol compatibility for non-hash fields. Hash field's serialization shape may change per HASH-S01.
- All activations continue to compile and pass non-hash-related tests throughout the epic.
- Solar's external wire behavior (observe, info, nested routing, list_children) stays identical.

## Completion

Epic is Complete when:
- `plugin_hash()` is implemented on every plexus-macros-generated activation.
- `aggregate_hash` is callable on any hub and handles missing children gracefully.
- Solar's tests verify the new aggregation model; the hand-written `plugin_children` loses its hash-preservation responsibility.
- `ChildSummary.hash` is either deprecated-with-docstring (Option path) or removed with a major version bump (breaking path) per HASH-S01.

A demo transcript captured in HASH-6's PR shows: kill a remote child mid-query, observe aggregate hash recomputing without it, restore it, observe aggregate returning to prior value.
