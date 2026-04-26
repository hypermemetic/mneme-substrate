---
id: IR-5
title: "plexus-macros: deprecation metadata capture across activations, methods, fields, and attribute args"
status: Complete
type: implementation
blocked_by: [IR-2]
unlocks: [IR-6, IR-7]
severity: Medium
target_repo: plexus-macros
---

## Problem

IR-3 captures `#[deprecated]` on individual `#[method]`s and emits `DeprecationInfo` on the corresponding `MethodSchema.deprecation` field. The rest of the surface is still silent:

- `#[deprecated]` on an `impl Activation` block (deprecating the whole activation) is parsed but not emitted onto the `PluginSchema`.
- `#[deprecated]` on fields of parameter input types is not scanned.
- Deprecated **attribute arguments** on `#[plexus_macros::activation]` and `#[plexus_macros::method]` — specifically `hub = true` and `children = [...]`, both slated for removal in favor of `#[child]` — emit no warning. Authors using them have no compile-time signal that they are using superseded syntax.

This ticket closes those gaps so synapse (IR-6) and synapse-cc (IR-7) have deprecation metadata to surface on every IR surface they render.

## Context

Target crate: `plexus-macros` at `/Users/shmendez/dev/controlflow/hypermemetic/plexus-macros`.

Depends on IR-2's public types: `DeprecationInfo` and the `PluginSchema` / `MethodSchema` extension surfaces.

**Superseded attribute arguments pinned in this ticket:**

| Attribute argument | Status | Replacement |
|---|---|---|
| `#[plexus_macros::activation(hub = true)]` | Deprecated (superseded by automatic hub inference — CHILD-8) | Omit — macro infers hub mode from presence of `#[child]`-tagged methods |
| `#[plexus_macros::activation(children = [...])]` | Deprecated (superseded by `#[child]` attribute on accessor methods — CHILD-3/4) | Use `#[plexus_macros::child]` on the accessor method(s) |

When the macro encounters one of these superseded attribute arguments, it emits a compile-time warning (not error — backward compat is preserved until a future plexus-macros major) pointing at the replacement syntax.

**Activation-level and field-level deprecation:**

| Source | Emitted `DeprecationInfo` surface |
|---|---|
| `#[deprecated(...)]` on the `impl Activation for Foo` block (or on the `Foo` type targeted by `#[activation]`) | New `PluginSchema.deprecation: Option<DeprecationInfo>` field — this ticket adds it to `plexus-core` as part of the IR extension (see Acceptance 1). |
| `#[deprecated(...)]` on a field of a `#[derive(...)]` input type used as a `#[method]` param | Propagated onto `ParamSchema.field_deprecations: Vec<(FieldName, DeprecationInfo)>` or similar structure. The exact shape on `ParamSchema` is pinned in Acceptance 4. |

`removed_in` is captured via the `#[plexus_macros::removed_in("...")]` companion attribute introduced in IR-3. If `#[deprecated]` appears without a companion `removed_in`, the macro uses the literal fallback `"unspecified"` (same convention IR-3 established).

## Required behavior

Extend `plexus-macros` to scan and propagate deprecation metadata across four scopes.

**Scope 1 — Activation-level deprecation:**

The macro reads `#[deprecated(...)]` on the target type or `impl` block and produces a `DeprecationInfo` on the emitted `PluginSchema`. This requires a new public field on `PluginSchema` in `plexus-core`:

| New field on `PluginSchema` | Type | Default |
|---|---|---|
| `deprecation` | `Option<DeprecationInfo>` | `None` |

Add this field in this ticket as part of the plexus-core IR extension (coordinate with IR-2's types; IR-2 does not have to re-open).

**Scope 2 — Method-level deprecation (already partially handled by IR-3):**

No behavior change here. IR-3 already captures method-level `#[deprecated]` and writes it to `MethodSchema.deprecation`.

**Scope 3 — Parameter-field deprecation:**

For each `#[method]`'s parameter list, the macro walks the parameter's type definition (if resolvable in the same crate — cross-crate type resolution is out of scope; emit no metadata when unresolvable). For each field of that type carrying `#[deprecated]`, the macro emits an entry on the corresponding `ParamSchema`:

| New field on `ParamSchema` | Type | Default |
|---|---|---|
| `field_deprecations` | `Vec<(String, DeprecationInfo)>` (field name → info) | Empty vec |

Cross-crate parameter types produce an empty `field_deprecations` — not an error. Acceptance 5 pins this.

**Scope 4 — Deprecated attribute-argument warnings:**

| Attribute argument | Action when macro encounters it |
|---|---|
| `#[plexus_macros::activation(hub = true)]` | Still compiles. Emits a compile-time warning: `"plexus-macros: the 'hub' argument is deprecated; hub mode is inferred automatically from #[child]-tagged methods. This argument will be removed in plexus-macros 0.6."` |
| `#[plexus_macros::activation(children = [...])]` | Still compiles. Emits a compile-time warning: `"plexus-macros: the 'children' attribute argument is deprecated; use #[plexus_macros::child] on the accessor method(s) instead. This argument will be removed in plexus-macros 0.6."` |

Warnings are emitted via `proc_macro::Diagnostic` (nightly) **or** via `compile_error!` only on opt-in — the default behavior on stable Rust is to emit a `#[deprecated]`-style warning through a dummy `#[allow(dead_code)] const _: () = { /* generated deprecated item */ };` trick, or through the established pattern already used in `plexus-macros` for similar warnings. The exact emission mechanism is an implementation detail; the observable contract (Acceptance 6, 7) is that **the build emits a warning whose text contains the specified migration hint**.

## Risks

| Risk | Mitigation |
|---|---|
| **File-boundary concurrency with IR-3.** Both tickets modify `plexus-macros` codegen files (likely `src/activation.rs` and attribute-parsing modules). Running them in parallel collides at commit time. | **Serialize: land IR-3 first, then IR-5.** IR-5 rebases onto IR-3's tip. This risk is called out here explicitly so the planning DAG reflects the constraint: IR-3 and IR-5 are NOT safe to run concurrently despite having distinct logical scopes. |
| Adding `deprecation` to `PluginSchema` is a plexus-core change that belongs with IR-2. | IR-2 pinned `MethodSchema.deprecation`. Extending `PluginSchema` is a small additive change consistent with IR-2's convention — rollable into IR-5's PR scope. If IR-2 is re-opened instead, this ticket only covers the macro side. Decision: this ticket owns the `PluginSchema.deprecation` field addition **if** IR-2 did not include it. Acceptance 1 verifies its presence regardless of which ticket adds it. |
| Cross-crate parameter types cannot have their fields scanned. | Documented behavior: emit empty `field_deprecations`. Not an error, not a warning. Acceptance 5 pins. |
| Attribute-argument warnings using `#[deprecated]` dummy-item trick cause unexpected lint escalation in consumer crates. | Use `#[allow(deprecated)]` on the dummy item to limit noise. Consumers with `-D warnings` policies see one expected warning per usage site. Document in the warning text. |
| Workspace activations in substrate currently use `hub = true` or `children = [...]` — they will begin emitting deprecation warnings after this ticket lands. | Those warnings are the intended outcome. Substrate's activations will migrate off the deprecated args in follow-up tickets (IR-8 covers Solar; other activations are out of scope for this epic). Substrate `cargo build` continues to succeed with warnings. |

## What must NOT change

- IR-3's behavior on method-level `#[deprecated]` and `#[plexus_macros::removed_in("...")]` — identical to what IR-3 landed.
- `#[plexus_macros::activation(hub = true)]` and `#[plexus_macros::activation(children = [...])]` continue to **compile** — only a warning is added. No hard removal.
- `#[plexus_macros::child]`, `#[plexus_macros::method]`, and non-deprecated attribute arguments behave exactly as they do after IR-3.
- All existing fixtures in `plexus-macros` pass.
- Substrate `cargo build --workspace` succeeds. Substrate activations that currently use `hub = true` or `children = [...]` will emit warnings but continue to build.

## Acceptance criteria

1. `cargo build -p plexus-core` succeeds. `PluginSchema.deprecation: Option<DeprecationInfo>` is present on the public `PluginSchema` type (regardless of whether IR-2 or IR-5 added it).
2. `cargo build -p plexus-macros` and `cargo test -p plexus-macros` succeed.
3. A trybuild fixture with `#[deprecated(since = "0.5", note = "migrate to Foo2")]` + `#[plexus_macros::removed_in("0.6")]` on an `impl Activation for Foo` block produces a `PluginSchema` with `deprecation == Some(DeprecationInfo { since: "0.5".into(), removed_in: "0.6".into(), message: "migrate to Foo2".into() })`.
4. A trybuild fixture with an `#[method]` whose parameter is a struct `MyReq` defined in the same crate, where `MyReq` has two fields — `a: String` (not deprecated) and `b: String` (`#[deprecated(since = "0.5", note = "use c")]`) — produces a `ParamSchema` whose `field_deprecations` contains exactly one entry: `("b".into(), DeprecationInfo { since: "0.5".into(), removed_in: "unspecified".into(), message: "use c".into() })`.
5. A trybuild fixture with an `#[method]` whose parameter type is imported from a different crate (e.g., `serde_json::Value` or a test helper crate) produces a `ParamSchema` with an empty `field_deprecations` vec — compilation succeeds without warning.
6. A trybuild fixture with `#[plexus_macros::activation(namespace = "t", hub = true)]` compiles and emits a build warning whose text contains the exact phrase `"'hub' argument is deprecated"` and the substring `"0.6"` (the removal target).
7. A trybuild fixture with `#[plexus_macros::activation(namespace = "t", children = [inner])]` compiles and emits a build warning whose text contains the exact phrase `"'children' attribute argument is deprecated"` and names `#[plexus_macros::child]` as the replacement.
8. Substrate `cargo build --workspace` succeeds. Any existing activation using `hub = true` or `children = [...]` builds but emits the deprecation warnings from criteria 6 / 7.

## Completion

- PR against `plexus-macros` extending the attribute scanner to capture deprecation on activations, parameter-type fields, and deprecated attribute args.
- PR may include a small additive change to `plexus-core` to add `PluginSchema.deprecation` and `ParamSchema.field_deprecations` if those fields weren't added by IR-2. These additions are minimal and follow IR-2's serde-default convention.
- PR description includes `cargo test -p plexus-macros` and `cargo build -p plexus-substrate` output — all green, with warnings from criteria 6/7 visible for any substrate activation still using the deprecated args.
- Ticket status flipped from `Ready` → `Complete` in the same commit.
- PR notes IR-6 and IR-7 are unblocked.
