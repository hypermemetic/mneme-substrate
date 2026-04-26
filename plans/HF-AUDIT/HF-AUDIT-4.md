---
id: HF-AUDIT-4
title: "plexus-macros 0.5.2: skip child-role methods in .schema strip-suffix shortcut (narrow fix for HF-AUDIT-3)"
status: Complete
type: implementation
blocked_by: []
unlocks: []
severity: Critical
target_repo: plexus-macros
---

## Problem

HF-AUDIT-3 diagnosed `synapse lforge hyperforge build` returning "No schema in response" after HF-CLEAN. Root cause: the macro's `.schema` strip-suffix shortcut (`plexus-macros/src/codegen/activation.rs:688-702`) matches `#[plexus_macros::child]` accessors because they're listed in `plugin_schema.methods` with role `static_child` since CHILD-8. The shortcut returns a `SchemaResult::Method` (method schema) when it should fall through to `route_to_child` so the child returns its own `PluginSchema`.

This is the **narrow fix**: exclude child-role methods from the shortcut's `find`. The shortcut continues to work for regular `#[method]`s; child accessors skip it and fall through to child routing, returning the child's full PluginSchema. Restores pre-HF-CLEAN behavior.

## Context

- Repo: `/Users/shmendez/dev/controlflow/hypermemetic/plexus-macros/`
- Version: 0.5.1 → 0.5.2 (patch — bug fix, no API change).
- File: `src/codegen/activation.rs`, lines 688-702 (the `_` arm's `strip_suffix(".schema")` branch).

Current code:

```rust
_ => {
    if let Some(method_name) = method.strip_suffix(".schema") {
        let plugin_schema = self.plugin_schema();
        if let Some(m) = plugin_schema.methods.iter().find(|m| m.name == method_name) {
            let result = #crate_path::plexus::SchemaResult::Method(m.clone());
            return Ok(...);
        }
    }
    #call_fallback
}
```

Issue: `find` matches ALL methods by name, including child accessors whose role is `MethodRole::StaticChild` or `MethodRole::DynamicChild { .. }`. Those should route to the child, not return method_schema.

## Required behavior

Change the `find` predicate to exclude child-role methods:

```rust
_ => {
    if let Some(method_name) = method.strip_suffix(".schema") {
        let plugin_schema = self.plugin_schema();
        if let Some(m) = plugin_schema.methods.iter().find(|m| {
            m.name == method_name
                && !matches!(
                    m.role,
                    #crate_path::plexus::MethodRole::StaticChild
                        | #crate_path::plexus::MethodRole::DynamicChild { .. }
                )
        }) {
            let result = #crate_path::plexus::SchemaResult::Method(m.clone());
            return Ok(...);
        }
        // Child-role method or no match → fall through to child routing.
    }
    #call_fallback
}
```

Call flow post-fix:
1. `hyperforge.build.schema` arrives at HyperforgeHub.
2. Activation::call: no local user-method match → `_` arm.
3. strip_suffix → `"build"`. find with the new predicate: `build` is in methods but its role is `StaticChild` → skipped.
4. Fall through to `call_fallback = route_to_child(self, "build.schema", ...)`.
5. `route_to_child` splits to `("build", "schema")` → `get_child("build")` → `BuildHub`.
6. `BuildHub.router_call("schema", ...)` → `BuildHub.call("schema")` → matches the `"schema"` arm → returns `SchemaResult::Plugin(plugin_schema)` for BuildHub.
7. Synapse's content_type filter accepts the response ending in `.schema`. Done.

## Risks

| Risk | Mitigation |
|---|---|
| A legitimate user method named the same as a child (shouldn't happen but possible). | The find match excludes child-role, so if a user explicitly named a `#[method]` `build` alongside a `#[child]` fn `build`, the method wins. This should already error at macro-compile for name collision; verify no existing consumer does this. |
| `MethodRole::DynamicChild { .. }` variant shape drift. | Use pattern `DynamicChild { .. }` to tolerate added fields without breaking. |
| Trybuild fixtures capture the old codegen output. | Re-rebless affected fixtures with `TRYBUILD=overwrite cargo test -p plexus-macros`. The diff is tiny — only the internal match arm changes, no user-visible code. |
| A downstream crate relies on the current (buggy) behavior where `<parent>.<child>.schema` returns method_schema. | No known consumer. plans/HF-AUDIT/HF-AUDIT-3.md documents the bug as causing "No schema in response" — no workaround that depends on the bug. |

## What must NOT change

- `#[plexus_macros::activation]`, `#[plexus_macros::method]`, `#[plexus_macros::child]` author-facing attribute surface.
- `Activation::call` return type.
- Any other dispatch arm in the `match method { ... }` block.
- Any other deprecated or non-deprecated macro surface. This is a one-predicate fix.

## Acceptance criteria

1. `cargo build -p plexus-macros` + `cargo test -p plexus-macros` green.
2. Trybuild fixtures re-reblessed. Diff is limited to the internal match arm comment/codegen; no user-visible stderr changes expected.
3. A new fixture test: an activation with one static child and one regular method. Verify `<activation>.call("<child>.schema")` returns a PluginSchema matching the child's plugin_schema, AND `<activation>.call("<method>.schema")` still returns a method_schema for the user method.
4. Downstream integration check: rebuild hyperforge against plexus-macros 0.5.2 (via path patch or post-publish). Start hyperforge. `synapse lforge hyperforge build` renders BuildHub's schema tree. `synapse lforge hyperforge build dirty path=... all_git=true` streams events correctly.
5. plexus-macros `Cargo.toml` version is `0.5.2`. Annotated tag `plexus-macros-v0.5.2` exists locally.
6. HF-AUDIT-3 flipped to Complete, citing this ticket.

## Completion

PR against plexus-macros. Status flipped Complete when hyperforge drill-down verification passes. Separate commit in hyperforge (pin bump to plexus-macros 0.5.2 + rebuild + restart) documented in this ticket's commit body.

## Relation to PROT epic

PROT is the architectural cleanup (unify all schema responses to PluginSchema, remove `SchemaResult::Method` variant entirely, major version bump across 6 crates). This ticket is the narrow fix that restores correct child-schema drill-down without the protocol rework.

After this ticket lands, PROT becomes **optional** architectural future work, not urgent bug fix. PROT-1's motivation text should be updated to reflect that HF-AUDIT-3 is addressable via either path.
