---
id: PROT-1
title: "PROT epic — unified schema protocol: every addressable node returns PluginSchema"
status: Epic
type: epic
blocked_by: []
unlocks: [PROT-S01, PROT-S02, PROT-S03, PROT-S04, PROT-2, PROT-3, PROT-4, PROT-5, PROT-6, PROT-7, PROT-8, PROT-9, PROT-10]
target_repo: multiple
---

## Goal

End state: every `.schema` fetch against a Plexus RPC node returns a `PluginSchema` — the same shape — regardless of whether the node is a top-level activation, a child activation, or a leaf method. Synapse and every other consumer navigate tree depth uniformly. The current asymmetry (activations return `PluginSchema`, methods return `MethodSchema` via a fragile strip-suffix shortcut) is removed.

Clean break: `SchemaResult::Method` wire variant removed. Synapse's content-type special-casing removed. plexus-core 0.6.0, plexus-macros 0.6.0, plexus-transport 0.3.0, plexus-protocol 0.6.0.0, synapse 4.0.0, plexus-substrate 0.6.0, hyperforge 5.0.0 ship together.

## Motivating bug (HF-AUDIT-3)

Post-HF-CLEAN, `synapse lforge hyperforge build` fails with "Fetch error at hyperforge.build: Protocol error: No schema in response".

Root cause (confirmed by wire trace):

- `lforge.call {method: "hyperforge.build.schema"}` returns content_type `hyperforge.method_schema` — a MethodSchema for the `build` child accessor, NOT BuildHub's PluginSchema.
- Synapse filters responses by content_type suffix `.schema`. `method_schema` doesn't match → filter returns empty → "No schema in response".
- The macro's `Activation::call` dispatch at `plexus-macros/src/codegen/activation.rs:688-702` has a `strip_suffix(".schema")` shortcut that matches `.<method>.schema` and returns `SchemaResult::Method` for any method found in `plugin_schema.methods`.
- Since CHILD-8 landed, `#[plexus_macros::child]` accessors are listed in `plugin_schema.methods` with `role: static_child`. The shortcut matches them and returns the wrong shape.
- Pre-HF-CLEAN (hand-written `plugin_children()`), child accessors were NOT in `plugin_schema.methods`. The `find` failed, fell through to `route_to_child` → `BuildHub.call("schema")` → PluginSchema. Worked.
- HF-CLEAN (commit `4dacf7c`) migrated HyperforgeHub/RepoHub off `hub = true` to `#[child]` accessors. The macro now lists them in `methods`. Bug triggers.

## Design — what unification looks like

Every `.schema` fetch at any path returns a `PluginSchema`. One shape. No special cases.

| Target | Response |
|---|---|
| Root (`<backend>.schema`) | DynamicHub's `PluginSchema` (activations tree). |
| Top-level activation (`<backend>.<ns>.schema`) | That activation's `PluginSchema`. |
| Child activation (`<backend>.<ns>.<child>.schema`) | Child's `PluginSchema`, via router. |
| Leaf method (`<backend>.<ns>.<method>.schema`) | **Synthetic** `PluginSchema::leaf_with_single_method(qualified_ns, method_schema)` — a 1-method PluginSchema wrapping the MethodSchema. |

Consumers always deserialize the same type. No content_type branch. Synapse tree navigation drills uniformly.

## Why clean break, not compat shim

- Wire format is under active development; no external consumers pinning 0.5.x.
- `SchemaResult::Method` is a live bug — keeping it deserializable risks future code accidentally emitting it.
- Deprecation dance (IR-4 style) adds code paths; clean removal is simpler.
- All consumers rebuild in this session; version-bump audit sweep catches any stale pins.

## Dependency DAG

```
        PROT-2 (plexus-core 0.6.0)
             │
    ┌────────┼────────┐
    ▼        ▼        ▼
  PROT-3   PROT-4   PROT-5
 (macros) (trans-  (protocol
          port)    Haskell)
    │        │        │
    │        │        ▼
    │        │     PROT-6
    │        │    (synapse
    │        │     4.0.0)
    └────────┴────┬───┘
         ┌────────┼────────┐
         ▼        ▼        ▼
       PROT-7  PROT-8   PROT-9
     (substr) (hyper-  (downstream
      0.6.0)   forge   audit)
              5.0.0)
         │        │        │
         └────────┼────────┘
                  ▼
             PROT-10
          (e2e verify +
           publish all)
```

## Phase breakdown

| Phase | Tickets | Notes |
|---|---|---|
| 0. Spikes | PROT-S01, PROT-S02, PROT-S03, PROT-S04 | Binary-pass. Ratify `SchemaResult` shape across 6 crates; audit `.hash` / `_info` for same bug; verify fix works with dynamic child gates; grep workspace for direct `SchemaResult::Method` usage. |
| 1. Core type change | PROT-2 | Add helper + remove `SchemaResult::Method`. Bumps plexus-core 0.5.0 → 0.6.0. |
| 2. Macro + transport + protocol | PROT-3, PROT-4, PROT-5 | Parallel. Each consumes plexus-core 0.6.0. |
| 3. Synapse adopt | PROT-6 | Depends on plexus-protocol 0.6.0.0. Major bump 3.x → 4.0. |
| 4. Consumer rebuilds | PROT-7, PROT-8, PROT-9 | Parallel. Substrate, hyperforge, downstream audit. |
| 5. Release + verify | PROT-10 | Publish to crates.io, push tags, end-to-end synapse drill-down test. |

## Cross-cutting contracts pinned here

- **New helper:** `plexus_core::plexus::PluginSchema::leaf_with_single_method(ns: &str, method: MethodSchema) -> PluginSchema` — constructs a leaf-shape PluginSchema with `methods = vec![method]`, description derived from the method's description, `is_hub = false`, `children = vec![]`.
- **Removed:** `plexus_core::plexus::SchemaResult::Method` variant. The enum becomes a single-variant marker or is flattened to `PluginSchema` directly. PROT-2 decides.
- **Macro invariant:** generated `Activation::call` never emits `SchemaResult::Method`. The `_` arm's strip-suffix logic rewrites to:
  1. Try `get_child(child_name)` first. If Some, route `schema` into it.
  2. Else try `find` in `plugin_schema.methods`. If Some (and NOT a child-role — belt-and-suspenders), synthesize a 1-method PluginSchema and return.
  3. Else fall through to `call_fallback`.
- **Deprecated surfaces removed:** `#[plexus_macros::activation(... hub)]`, `#[plexus_macros::hub_method]`, `#[plexus_macros::hub_methods]`, `ChildCapabilities` (if applicable per IR precedent). PROT-3 removes; consumers already migrated.
- **Content_type filter:** Haskell `Plexus.Transport.fetchSchemaAt` filter already accepts any suffix `.schema`. Synthetic leaves use content_type `<qualified_ns>.schema` — passes the filter. PROT-5 updates the ADT; PROT-6 removes the parser's `method_schema` branch.

## Version matrix

| Crate | Before | After | Bump reason |
|---|---|---|---|
| plexus-core | 0.5.0 | 0.6.0 | Remove `SchemaResult::Method` variant (breaking). |
| plexus-macros | 0.5.1 | 0.6.0 | Drop `hub` / `hub_method` / `hub_methods` deprecated surfaces; new codegen. |
| plexus-transport | 0.2.1 | 0.3.0 | Core 0.6 dep bump. |
| plexus-protocol (Haskell) | 0.5.0.0 | 0.6.0.0 | Remove `Method` variant from `SchemaResult`. |
| synapse | 3.13.0 | 4.0.0 | Wire-compat-breaking parse change. |
| plexus-substrate | 0.5.0 | 0.6.0 | Deps bump + rebuild. |
| hyperforge | 4.1.2 | 5.0.0 | Deps bump + `#[allow(deprecated)]` TODOs retire cleanly. |

## What must NOT change

- Existing activation surface for users. `#[plexus_macros::activation]`, `#[plexus_macros::method]`, `#[plexus_macros::child]` all keep their current author-facing APIs.
- Existing method invocation behavior. Only `.schema` fetch responses change shape.
- Method semantics. A method's params, return type, streaming behavior — all unchanged.
- Children's addressability. `hyperforge.build.dirty` still invokes `BuildHub::dirty`.

## Out of scope

- Deeper "methods ARE activations" type-system unification. Reasonable future direction, separate epic if ever pursued.
- Changing synapse tree rendering beyond protocol conformance.
- Repositioning dynamic-child gates (`#[child(list = "...")]`) in the schema. HF-IR territory.
- Rewriting plexus-protocol's Haskell ADT beyond the `SchemaResult` update.
- Republishing all crates outside the PROT chain. Only PROT-affected crates bump in this epic.

## Risks

| Risk | Mitigation |
|---|---|
| Synapse CLI installed binary (3.10.1) is stale — won't benefit from this until reinstalled. | PROT-10 includes `cabal install` step + verification `synapse --version` reports 4.0.0. |
| Any downstream workspace crate (plexus-listen, plexus-locus, plexus-mono, mono-provider, plexus-music-royalty-free) has stale plexus-core 0.4 or 0.5 pins and will break with 0.6. | PROT-9 audits every sibling `Cargo.toml`; each drift gets filed as its own audit ticket. Already have HF-AUDIT-1 (mono-provider family) and HF-AUDIT-2 (plexus-locus). |
| HF-AUDIT-3's manifestation (`synapse lforge hyperforge build` fails) must actually be fixed by PROT — not masked by some other change. | PROT-10 explicitly re-runs the reproducer and asserts success. |
| `SchemaResult` is publicly re-exported from plexus-core and used by consumers directly (not via macro). | Grep workspace for `SchemaResult::Method` before PROT-2 lands. Substitute any direct uses with PluginSchema-returning helpers. |
| Hyperforge local binary (PID 18804) must be restarted to pick up new plexus-core/macros codegen. | PROT-10 includes explicit restart step. |

## Completion

Epic is Complete when:
- PROT-2 through PROT-10 are all Complete.
- `synapse lforge hyperforge build` (fresh synapse 4.0.0 against rebuilt hyperforge 5.0.0) renders BuildHub's schema tree without error.
- `synapse lforge hyperforge build dirty path=... all_git=true` streams dirty-repo events correctly.
- All PROT crates published to crates.io at their new versions. Tags pushed.
- HF-AUDIT-3 flipped Complete (bug fixed).
- HF-AUDIT-1 and HF-AUDIT-2 updated to reference plexus-core 0.6 deps in their fix plans (or remain Pending — their scope is broader than PROT).
