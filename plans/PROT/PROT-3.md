---
id: PROT-3
title: "plexus-macros 0.6.0: rewrite schema dispatch to child-first; drop deprecated hub/hub_method/hub_methods"
status: Pending
type: implementation
blocked_by: [PROT-2, PROT-S02, PROT-S03]
unlocks: [PROT-7, PROT-8]
severity: Critical
target_repo: plexus-macros
---

## Problem

The macro-generated `Activation::call` has a broken `strip_suffix(".schema")` branch at `src/codegen/activation.rs:688-702` that incorrectly returns a `SchemaResult::Method` for `#[plexus_macros::child]` accessors. This is HF-AUDIT-3's root cause.

Additionally, several deprecated surfaces remain in the macro's public API, blocking HF-0's `#[allow(deprecated)]` TODO markers from being cleaned:
- `#[plexus_macros::activation(... hub)]` — hub mode should be inferred from `#[child]` (CHILD-8); the flag itself is deprecated since 0.5 per HF-0's TODO markers.
- `#[plexus_macros::hub_method]` — replaced by `#[plexus_macros::method]`.
- `#[plexus_macros::hub_methods]` — replaced by `#[plexus_macros::activation]`.

PROT-3 fixes both: rewrites schema dispatch to route children first, and removes the deprecated macro surfaces at the 0.6 break.

## Context

- Repo: `/Users/shmendez/dev/controlflow/hypermemetic/plexus-macros/`
- Version: 0.5.1 → 0.6.0 (breaking: macro surface removals + consumer-visible behavior change on schema dispatch).
- Files to edit:
  - `src/codegen/activation.rs` — the `Activation::call` dispatch body and the schema-routing branches.
  - `src/lib.rs` — remove the `hub_method` and `hub_methods` public macros.
  - `src/codegen/mod.rs` — remove the `hub` attribute parsing (reject it with a clear error, or just drop the arg and keep compatibility for one deprecation cycle — see risks).

## Required behavior

### 1. Fix schema dispatch (the HF-AUDIT-3 fix)

Rewrite the generated `Activation::call` body's `_` arm to:

```rust
_ => {
    // First: if method is "<child>.schema" and <child> is a registered child,
    // route into the child. The child's own call() will handle "schema" arm.
    if let Some(child_name) = method.strip_suffix(".schema") {
        if let Some(child) = self.get_child(child_name).await {
            return child.router_call("schema", params, auth, raw_ctx).await;
        }
        // Not a child — synthesize a single-method PluginSchema for leaf methods.
        let plugin_schema = self.plugin_schema();
        if let Some(m) = plugin_schema.methods.iter()
            .find(|m| m.name == child_name)
        {
            let synthetic = #crate_path::plexus::PluginSchema::leaf_with_single_method(
                concat!(#namespace, ".", child_name),
                m.clone(),
            );
            return Ok(#crate_path::plexus::wrap_stream(
                futures::stream::once(async move { synthetic }),
                concat!(#namespace, ".", child_name, ".schema"),
                vec![#namespace.into()]
            ));
        }
        // Neither child nor local method — fall through to call_fallback.
    }

    #call_fallback
}
```

Key changes from current behavior:
- Child lookup happens BEFORE local-method find.
- Child lookup routes `schema` into the child; child returns its own PluginSchema.
- Leaf method fallback uses `PluginSchema::leaf_with_single_method` (new in PROT-2) — returns a PluginSchema, not a bare MethodSchema.
- `SchemaResult::Method` is never emitted.

### 2. Drop deprecated surfaces

Remove entirely:
- `#[plexus_macros::hub_method]` public macro.
- `#[plexus_macros::hub_methods]` public macro.
- The `hub` attribute arg on `#[plexus_macros::activation]`. It's already inferable from `#[child]` methods (CHILD-8). Reject it with a proc-macro error: `"the 'hub' argument is removed — hub mode is inferred from #[child] methods. See PROT-3."`

Also drop the `_PLEXUS_MACROS_DEPRECATED_HUB_FLAG_*` const emitted solely to surface the deprecation warning.

### 3. Remove ChildCapabilities emission (if still present)

If the macro still emits `ChildCapabilities::empty` / `ChildCapabilities::LIST` / `ChildCapabilities::SEARCH` in generated code (see IR-16 history), replace with the `MethodRole::DynamicChild { list_method, search_method }` pattern that was IR-12's replacement. Verify first via grep.

### 4. Update trybuild fixtures

IR-11 reblessed fixtures to carry `ChildCapabilities` deprecation warnings. IR-16 re-reblessed them to drop those warnings. PROT-3 re-rebless once more: the generated output changes (new schema dispatch body, no deprecated flag). Run `TRYBUILD=overwrite cargo test -p plexus-macros` after the codegen change; verify the diff is bounded to the schema dispatch change.

### 5. Version bump

plexus-macros: 0.5.1 → 0.6.0. Annotated tag `plexus-macros-v0.6.0` local.

### 6. Dependency bump

`plexus-core = "0.6"` in `Cargo.toml` `[dependencies]` and `[dev-dependencies]`. PROT-2's helper (`PluginSchema::leaf_with_single_method`) is the new surface being consumed.

## Risks

| Risk | Mitigation |
|---|---|
| Some consumer still uses `hub = true` in its activation attr (post-HF-CLEAN, hyperforge already migrated). | Grep workspace for `hub = true\|hub,\|, hub)` before removing. File any stragglers as per-repo cleanup tickets. Known so far: none post-HF-CLEAN. |
| Some consumer still uses `#[hub_method]` or `#[hub_methods]`. | Same grep. Known so far: none. |
| The trybuild rebless creates a massive diff that's hard to verify. | Run pre-change trybuild + post-change trybuild; diff the diffs to confirm only schema-path-related lines changed. Commit body enumerates each .stderr file changed. |
| The `concat!(...)` path construction at macro-expansion time produces the wrong string for complex namespaces. | Test fixture: a 3-level-nested activation. Verify `.schema` fetch at every level returns the right PluginSchema content_type (ends in `.schema`). |
| A downstream consumer passes `hub` as a positional arg (e.g., `#[activation("foo", hub)]` rather than `#[activation(namespace = "foo", hub)]`). | The proc-macro error message should say explicitly "the 'hub' argument was removed". If the parser can't distinguish positional vs keyword, the error message directs to documentation. |

## What must NOT change

- `#[plexus_macros::activation]`, `#[plexus_macros::method]`, `#[plexus_macros::child]` author-facing attribute surface (remaining args).
- `#[plexus_macros::removed_in]`, `#[plexus_macros::child(list = ..., search = ...)]` — unchanged.
- The `HandleEnum`, `StreamEvent`, `PlexusRequest`, `JsonSchemaNoop` derive macros — unchanged.
- The dispatch for local user methods (`#[method]` names matched in the `match` arms) — unchanged.
- The `Activation::call` return type (`Result<PlexusStream, PlexusError>`) — unchanged.

## Acceptance criteria

1. `cargo build -p plexus-macros` and `cargo test -p plexus-macros` pass green. All 15+ test suites including trybuild.
2. `grep -rn 'hub_method\|hub_methods\|_PLEXUS_MACROS_DEPRECATED_HUB_FLAG' plexus-macros/src/` returns zero results.
3. `grep -rn 'SchemaResult::Method' plexus-macros/src/` returns zero results.
4. Trybuild fixtures re-reblessed. The diff per fixture is bounded to schema-dispatch-related changes (no unrelated output shifts).
5. A new test fixture (or extension of an existing one) validates: for a parent activation with a `#[child]` accessor, `parent.call("child.schema")` returns a PluginSchema with `namespace == "<parent_ns>.<child_name>"` and the child's methods.
6. For a leaf method, `parent.call("<method>.schema")` returns a PluginSchema with `methods.len() == 1` and `methods[0].name == <method>`.
7. `cargo build` green in: plexus-core (already PROT-2), plexus-substrate, hyperforge (via PROT-7, PROT-8). Integration gate per rule 12.
8. plexus-macros `Cargo.toml` version is `0.6.0`. Annotated tag `plexus-macros-v0.6.0` exists locally.

## Completion

PR against plexus-macros. Status flipped to Complete when PROT-7, PROT-8 (the downstream rebuilds) both build green and exercise the new dispatch.
