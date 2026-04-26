---
id: PROT-2
title: "plexus-core 0.6.0: add PluginSchema::leaf_with_single_method, remove SchemaResult::Method"
status: Pending
type: implementation
blocked_by: [PROT-S01, PROT-S04]
unlocks: [PROT-3, PROT-4, PROT-5]
severity: Critical
target_repo: plexus-core
---

## Problem

`SchemaResult::Method` exists as a wire-level variant that carries a single `MethodSchema` instead of a full `PluginSchema`. The plexus-macros generated `Activation::call` emits this variant via a `strip_suffix(".schema")` shortcut for leaf methods. Synapse's response parser has no uniform way to handle it — it filters by content_type suffix `.schema` and the emitted `method_schema` content_type fails that filter.

PROT-1 pins the unified protocol: every `.schema` response is a `PluginSchema`. Leaf methods get wrapped in a synthetic single-method `PluginSchema`. `SchemaResult::Method` goes away.

## Context

- Repo: `/Users/shmendez/dev/controlflow/hypermemetic/plexus-core/`
- Version: 0.5.0 → 0.6.0 (breaking change: wire ADT removed).
- Files to edit (verify before editing):
  - `src/plexus/plexus.rs` — defines `SchemaResult`, `PluginSchema`, schema RPC handlers.
  - `src/plexus/mod.rs` — re-exports.
  - `src/plexus/types.rs` — `MethodSchema` (unchanged, but audit for any implicit dependency).

## Required behavior

1. **Add** `PluginSchema::leaf_with_single_method`:
   ```rust
   impl PluginSchema {
       /// Construct a leaf-shape PluginSchema containing a single method.
       /// Used by child-routing `.schema` fallback when the target is a
       /// leaf method: callers get a PluginSchema at every path, uniform.
       pub fn leaf_with_single_method(namespace: &str, method: MethodSchema) -> Self {
           Self {
               namespace: namespace.to_string(),
               description: method.description.clone(),
               version: env!("CARGO_PKG_VERSION").to_string(),  // or method-level version if tracked
               methods: vec![method],
               children: Vec::new(),
               is_hub: false,
               ..Default::default()
           }
       }
   }
   ```
   Fields exactly match existing `PluginSchema::leaf` constructor shape. Implementer reads the current `leaf` constructor to match field defaults.

2. **Remove** `SchemaResult::Method` variant. Either:
   - (a) Flatten: `SchemaResult` becomes `PluginSchema` directly (wire-level ADT disappears).
   - (b) Keep `SchemaResult` as a struct alias: `type SchemaResult = PluginSchema` (minimal churn for the rename).
   - (c) Collapse to single-variant enum: `enum SchemaResult { Plugin(PluginSchema) }` (least change but pointless).
   
   **Pin (a) or (b) here — implementer chooses the one with fewer call-site changes.** Report in the commit body which was chosen.

3. **Update the DynamicHub's `lforge.schema` handler** (`plexus.rs:953-981`): the existing code returns `SchemaResult::Plugin(plugin_schema)` by default and `SchemaResult::Method(m.clone())` when `method` param is set. After this ticket: always return `PluginSchema` — either the full DynamicHub schema (default) or `PluginSchema::leaf_with_single_method(ns, m)` when `method` param matches.

4. **Audit** the workspace for any direct consumer of `SchemaResult::Method` and migrate them to the synthetic leaf form. Targets: plexus-macros (PROT-3 handles), hub-codegen, any other Rust consumer.

5. **Version bump** plexus-core: 0.5.0 → 0.6.0 in `Cargo.toml`.

6. **Tag** `plexus-core-v0.6.0` locally (not pushed — PROT-10 handles publish).

## Risks

| Risk | Mitigation |
|---|---|
| `SchemaResult` is pub-re-exported and has external Rust consumers outside the workspace. | Grep workspace for `SchemaResult::Method` before editing. If matches exist outside PROT scope (plexus-macros / hub-codegen), file audit tickets. |
| `PluginSchema` default fields have drifted since the `leaf` constructor was written. | Read `PluginSchema` struct + `leaf` constructor side-by-side before writing `leaf_with_single_method`. Match the exact field initialization. |
| Clippy-deny may flag the new helper as `missing_docs` or similar. | Doc-comment the fn per plexus-core convention. Follow hyperforge's HF-CLEAN pattern — no `#[allow]` hacks. |
| `SchemaResult` flattening breaks derive macros that pattern-match on the variant. | Before flattening, grep for `match.*SchemaResult`. If matches exist, migrate them in the same commit. |

## What must NOT change

- `MethodSchema`'s shape (fields, serde representation).
- `PluginSchema`'s existing constructors (`leaf`, `hub`, `leaf_with_long_description`, etc.) — strictly additive.
- `Activation` trait — strictly additive (no new required methods).
- `ChildRouter` trait — strictly additive (no new required methods).
- Wire format for any non-schema response (`.call`, `.hash`, `_info`).
- plexus-core's re-exports other than `SchemaResult::Method`.

## Acceptance criteria

1. `cargo build -p plexus-core` and `cargo test -p plexus-core` pass green.
2. `PluginSchema::leaf_with_single_method("foo", method)` returns a PluginSchema with `namespace == "foo"`, `methods.len() == 1`, `is_hub == false`, `children.is_empty()`.
3. `grep -rn 'SchemaResult::Method' /Users/shmendez/dev/controlflow/hypermemetic/plexus-core/src/` returns zero results.
4. plexus-core's `Cargo.toml` version is `0.6.0`. Annotated tag `plexus-core-v0.6.0` exists locally.
5. Pre-existing deprecation warnings are unchanged (HF-IR's `#[allow(deprecated)]` scope already validated them).

## Completion

PR against plexus-core. Status flipped to Complete when PROT-3, PROT-4, PROT-5 can compile against this ticket's artifact. Not published yet — PROT-10 ships everything together.
