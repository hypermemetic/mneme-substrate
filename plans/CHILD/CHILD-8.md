---
id: CHILD-8
title: "Infer hub-mode from #[child] methods (hub flag + ChildRouter + plugin_children)"
status: Complete
type: implementation
blocked_by: []
unlocks: [CHILD-7]
severity: High
target_repo: plexus-macros
---

## Problem

CHILD-3 and CHILD-4 landed the `#[plexus_macros::child]` attribute and its opt-in `list`/`search` args, but only half-replaced the legacy `hub` flag + hand-written `ChildRouter` + hand-written `plugin_children()` pattern. When CHILD-7 attempted to migrate Solar off that pattern purely via `#[child]`, it hit three concrete gaps in the macro's codegen:

1. **`Activation::call` fallback** — without `hub = true`, the generated `call` returns `MethodNotFound` for unknown methods. Nested routing (`solar.mercury.info`) requires the hub-branch fallback `route_to_child(self, ...)`. `#[child]` does not trigger that branch today.
2. **`plugin_schema()` shape** — without `hub = true`, the macro emits `PluginSchema::leaf(...)`. Callers observing `schema.is_hub()` get `false`. `#[child]` does not flip the shape.
3. **`plugin_children()`** — the hub-shape `PluginSchema::hub(...)` constructor requires `self.plugin_children()` returning a `Vec<ChildSummary>`. Solar hand-writes this today. `#[child]` does not synthesize it.

Net effect: an activation using only `#[child]` (no `hub` flag, no hand-written router, no `plugin_children`) cannot be wire-equivalent to a legacy hub activation. CHILD-7's migration was blocked end-to-end by this gap.

## Context

The agent that attempted CHILD-7 verified the blocker empirically: draft migration of Solar built cleanly but failed 4 tests — `solar_is_hub_with_planets` (schema shape), `test_nested_routing_mercury`, `test_nested_routing_jupiter_io`, `test_nested_routing_earth_luna` (all `MethodNotFound` at `Activation::call`). A quick spike applying only the hub-inference fix (`hub = args.hub || !child_methods.is_empty()`) revealed the additional `plugin_children()` requirement in the hub-shape schema constructor — the fix is not one line.

Relevant source lines (snapshot — drift-tolerant):

| File | Concern |
|---|---|
| `plexus-macros/src/codegen/mod.rs:~242` | `args.hub` flows unchanged into `activation::generate`; needs to be OR-ed with `!child_methods.is_empty()`. |
| `plexus-macros/src/codegen/activation.rs:~55` | `call_fallback` branches on `hub`. |
| `plexus-macros/src/codegen/activation.rs:~129` | `plugin_schema_body` matches on `(hub, long_description)`. Hub-shape branches call `self.plugin_children()`. |
| `plexus-macros/src/codegen/activation.rs:~456` | `child_router_impl` skips codegen when `hub` is true — must differentiate "hub by explicit flag" from "hub by `#[child]` inference". |

## Required behavior

An activation that uses only `#[plexus_macros::child]` methods (no `hub` flag, no hand-written `impl ChildRouter`, no hand-written `plugin_children`) must produce wire-identical behavior to a legacy `hub` + hand-written-router activation for the same child set.

Specifically:

| Behavior | Expected |
|---|---|
| `Activation::call` with a known method name | Dispatches to that method (unchanged) |
| `Activation::call` with an unknown method name whose first path segment names a child | Routes to the child via `route_to_child(self, ...)` |
| `plugin_schema().is_hub()` | `true` |
| `plugin_schema().children` | Non-empty — contains one `ChildSummary` per static `#[child]` method (dynamic `#[child]` methods produce a summary describing the gate, not each possible name) |
| `ChildRouter::get_child(name)` | Unchanged from CHILD-3/CHILD-4 |

Codegen rules to add:

| Rule | Generated output |
|---|---|
| `args.hub` is true OR `child_methods` is non-empty | `Activation::call` uses `route_to_child(...)` fallback; `plugin_schema_body` uses the hub-shape constructor |
| `child_methods` is non-empty AND the impl does NOT define `plugin_children` | Macro synthesizes a `plugin_children() -> Vec<ChildSummary>` method returning one entry per static `#[child]` method. Dynamic `#[child]` methods contribute a single generic gate entry (shape TBD — see Risks). |
| `args.hub` is true AND `child_methods` is empty AND impl DOES define `plugin_children` (Solar-pre-migration pattern) | Unchanged — do not generate a `ChildRouter` impl; do not synthesize `plugin_children`. Legacy path preserved. |
| `args.hub` is true AND `child_methods` is non-empty | Error. Mixing explicit `hub` with `#[child]` is ambiguous — force the user to pick one. |

## Risks

| Risk | Mitigation |
|---|---|
| Dynamic `#[child]` method has no compile-time child name, but the hub-shape schema expects a `Vec<ChildSummary>`. | Emit a single `ChildSummary` for the dynamic gate with a placeholder-name convention (e.g., `"{name}"` or the method identifier). Final convention pinned in implementation; the key contract is that `is_hub()` is true and the summary list is non-empty. |
| Impls that hand-write `plugin_children` AND use `#[child]` | Detect via `input_impl.items`; if `plugin_children` method is present, skip synthesis and use the user's. |
| Existing `children = [...]` attribute and `hub` flag behavior | Unchanged. Only `#[child]`-bearing impls get the new codegen paths. |

## What must NOT change

- Solar's current `hub` + hand-written `ChildRouter` + hand-written `plugin_children` pattern continues to compile and behave identically. Only impls that actually use `#[child]` methods get the new codegen.
- Legacy `children = [...]` attribute continues to work on non-`#[child]` impls.
- `ChildRouter::get_child` / `list_children` / `search_children` behavior from CHILD-3 and CHILD-4 is unchanged.
- Substrate's 16 activations compile without edits after this ticket lands.

## Acceptance criteria

1. `cargo build -p plexus-macros` and `cargo test -p plexus-macros` both succeed.
2. A committed fixture activation using only `#[plexus_macros::child]` (no `hub` flag, no hand-written router, no hand-written `plugin_children`) satisfies:

   | Observable | Expected |
   |---|---|
   | `plugin_schema().is_hub()` | `true` |
   | `plugin_schema().children.len()` | ≥ 1 |
   | `Activation::call(self, "child.method_name", ...)` for a static child | Returns the child's method output (nested routing works) |
   | `ChildRouter::get_child("child_name")` | Returns `Some` (CHILD-3 regression) |
   | `ChildRouter::list_children()` with `list = "..."` opt-in | Returns `Some(stream)` with expected names (CHILD-4 regression) |

3. A committed trybuild fixture with both explicit `hub` flag AND `#[child]` methods fails to compile with an error that names the ambiguity.
4. Solar can be migrated per CHILD-7's original spec (no `hub` flag, no hand-written `ChildRouter`, no hand-written `plugin_children`) and `cargo test -p plexus-substrate` passes — specifically `solar_is_hub_with_planets`, `test_nested_routing_mercury`, `test_nested_routing_jupiter_io`, `test_nested_routing_earth_luna` all pass without modification.
5. A regression fixture demonstrating the Solar-pre-migration pattern (`hub` flag + hand-written `impl ChildRouter` + hand-written `plugin_children`, no `#[child]` methods) still compiles and passes its existing tests. No changes required to that pattern.

## Completion

PR against `plexus-macros` landing the codegen changes plus fixtures. CI green. Status flipped from `Ready` to `Complete` in the same commit as the code. Once shipped, CHILD-7 (Solar migration) is retried — no changes to CHILD-7's own acceptance criteria are required; CHILD-8 is what makes those criteria satisfiable.
