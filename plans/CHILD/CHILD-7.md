---
id: CHILD-7
title: "Migrate substrate Solar activation to #[child] method attribute"
status: Complete
type: implementation
blocked_by: [CHILD-3, CHILD-4, CHILD-5, CHILD-6]
unlocks: []
severity: Medium
target_repo: plexus-substrate
---

## Problem

Solar is the marquee hub activation in substrate — it exists specifically to demonstrate nested plugin hierarchy with dynamic children (planets). It was built before the CHILD epic's ergonomic improvements, so it uses the legacy `hub` flag on its activation macro plus a hand-written `impl ChildRouter for Solar` block (~25 lines) that performs case-insensitive lookup against `self.system.children`. The CHILD epic has since introduced the `#[plexus_macros::child]` method attribute, opt-in list/search capabilities, doc-comment extraction, and a fixed `crate_path` default. This ticket migrates Solar onto those new facilities, validates the epic end-to-end on its most representative consumer, and proves the manual boilerplate collapses to one or two method declarations.

## Context

Solar lives at `src/activations/solar/` in plexus-substrate. Its current header is:

```
#[plexus_macros::activation(
    namespace = "solar",
    version = "1.0.0",
    description = "Solar system model - demonstrates nested plugin hierarchy",
    hub,
    crate_path = "plexus_core"
)]
```

Its current `ChildRouter` is hand-written: `router_namespace()` returns `"solar"`, and `get_child(name)` lowercases `name`, searches `self.system.children` for a matching lowercased name, and wraps hits in `Box::new(CelestialBodyActivation::new(c.clone()))`. Children are dynamic — they come from the runtime `build_solar_system()` config, not a static list.

The CHILD epic has delivered these primitives that this ticket consumes:
- CHILD-2: `ChildRouter` trait extended with default-`None` `capabilities()`, `list_children()`, `search_children()`.
- CHILD-3: `#[plexus_macros::child]` method attribute for child lookup.
- CHILD-4: `list = "..."` / `search = "..."` args on `#[child]`, auto-generating capability bitflags.
- CHILD-5: doc-comment extraction into activation/method descriptions.
- CHILD-6: `crate_path` default fix so the attr arg is no longer required.

## Required behavior

| Behavior | Before migration | After migration |
|---|---|---|
| `solar.info` returns system overview | Works | Identical response |
| `solar.mercury.info` (nested call via `DynamicHub` routing) | Works | Identical response |
| `solar.get_child("Mercury")` resolves to Mercury | Works | Identical |
| `solar.get_child("mercury")` resolves to Mercury (case-insensitive) | Works | Identical |
| `solar.list_children` | Unimplemented (`None`) | Returns `Some(stream)` yielding planet names |
| Solar's namespace | `"solar"` | `"solar"` (unchanged) |
| Set of Plexus RPC methods exposed by Solar | Baseline set | Baseline set plus the new `list_children` capability; no methods removed or renamed |

Migration rules that must be applied to `src/activations/solar/activation.rs`:

| Item | Action |
|---|---|
| `hub` flag on activation macro | Remove (macro infers hub-ness from presence of `#[child]` methods) |
| `crate_path = "plexus_core"` on activation macro | Remove (CHILD-6 fixed the default) |
| `description = "..."` on activation and on methods | Remove and replace with `///` doc comments; CHILD-5 extracts them |
| Hand-written `impl ChildRouter for Solar` block | Delete; macro generates the impl from the `#[child]` method |
| Case-insensitive name normalization that currently lives in `get_child` | Preserve inside the body of the new `#[child]` method |
| Dynamic child source (`self.system.children` from runtime config) | Preserve — the `#[child]` method body iterates the same source |

`list = "..."` is required (Solar acquires a new `list_children` capability). `search = "..."` is optional — add only if a natural search semantic over planet names is obvious; otherwise skip (opt-in by design).

## Risks

- A `#[child]` method that returns `impl Stream<Item = String>` over `&self` may hit lifetime/GAT issues. Fallback: return `BoxStream<'_, String>` explicitly — per CHILD-4, the macro accepts both.
- Case-insensitive lookup must remain byte-exact for compatibility; `synapse solar Mercury` and `synapse solar mercury` must both resolve. If the macro imposes a normalization the user-supplied method body cannot override, migration stalls and the risk is reported — don't paper over it.
- If any substrate test uses Solar as a fixture, it must still pass without modification. If a test is coupled to the old `hub` flag or the hand-written impl, that is a regression and must be fixed in-ticket.
- If CHILD-5 doc-comment extraction does not in practice surface the activation-level `///` on the `impl` block in the substrate manifest, the migration still lands (descriptions are a Plexus RPC nicety, not wire-visible), but flag the gap.

## What must NOT change

- Solar's namespace (`"solar"`).
- The set of Plexus RPC method names exposed by Solar, apart from the newly available `list_children` capability.
- Response shapes for `solar.info` and for nested `solar.{planet}.*` calls.
- Case-insensitive planet name lookup.
- The runtime source of Solar's children (still `build_solar_system()` / `self.system.children`).

## Acceptance criteria

1. `cargo build -p plexus-substrate` succeeds.
2. `cargo test -p plexus-substrate` succeeds; all pre-existing Solar-related tests pass without modification.
3. `grep -c 'impl ChildRouter for Solar' src/activations/solar/activation.rs` returns `0`.
4. `grep -c 'hub,' src/activations/solar/activation.rs` returns `0` (the `hub` attr-arg flag has been removed from Solar's activation header).
5. `grep -c 'crate_path = "plexus_core"' src/activations/solar/activation.rs` returns `0`.
6. With substrate running (`./target/debug/plexus-substrate -p 4444`), `synapse solar` lists the `info` method and a body/child dispatcher; `synapse solar Mercury` and `synapse solar mercury` both resolve to Mercury and return its info.
7. Against the same running substrate, invoking Solar's `list_children` capability returns a non-empty stream whose items are the planet names configured by `build_solar_system()`.
8. An integration test — new or existing — exercises `list_children` against Solar and asserts the returned stream is non-empty and matches the configured planet set.

## Completion

PR against plexus-substrate; CI green; the PR description includes a short synapse transcript showing `solar`, `solar mercury`, and `solar list_children` all responding. Ticket status flipped to `Complete` in the same commit that lands the migration.
