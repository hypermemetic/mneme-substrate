---
id: IR-3
title: "plexus-macros: emit MethodRole + deprecation on generated MethodSchema"
status: Complete
type: implementation
blocked_by: [IR-2]
unlocks: [IR-4, IR-8]
severity: High
target_repo: plexus-macros
---

## Problem

Once IR-2 lands, `MethodSchema` carries `role: MethodRole` and `deprecation: Option<DeprecationInfo>`. Today the proc-macro emits `MethodSchema` structs that know nothing about these fields — they silently default to `Rpc` and `None`, which is incorrect for any method annotated with `#[plexus_macros::child]`. Downstream tickets (IR-4 reads roles to populate the deprecated `children` / `is_hub` shim; IR-8 reads roles from Solar's schema to migrate tests) cannot proceed until the macro actually emits the role tags.

## Context

Target crate: `plexus-macros` at `/Users/shmendez/dev/controlflow/hypermemetic/plexus-macros`.

Relevant macro attribute shapes already supported (post-CHILD-3/4/8):

| Attribute | Effect in today's macro |
|---|---|
| `#[plexus_macros::method]` on `fn foo(&self, ...)` | Emits an RPC `MethodSchema` with name, params, return shape, description. |
| `#[plexus_macros::child]` on `fn name(&self)` (no name arg) | Emits a static child lookup; the method name is the child's name. |
| `#[plexus_macros::child(name = "...")]` on `fn gate(&self, key: &str)` | Emits a dynamic child lookup keyed by argument. |
| `#[plexus_macros::child(list = "list_fn")]` | Opts into `list_children` capability; names the sibling method that yields keys. |
| `#[plexus_macros::child(search = "search_fn")]` | Opts into `search_children` capability; names the sibling method that matches keys. |

Rust's built-in `#[deprecated]` attribute form:

```rust
#[deprecated(since = "0.5", note = "use MethodRole instead")]
```

`since` and `note` are supported by rustc. `removed_in` is **not** a rustc-recognized key. To capture a removal version, this ticket introduces a companion attribute on the `plexus-macros` side:

```rust
#[plexus_macros::removed_in("0.6")]
```

which the macro reads alongside `#[deprecated]` to populate `DeprecationInfo.removed_in`. Either attribute alone is permitted:

| Attributes present on a method | Resulting `DeprecationInfo` |
|---|---|
| None | `None` |
| `#[deprecated(since = "X", note = "Y")]` | `Some(DeprecationInfo { since: "X", removed_in: "unspecified", message: "Y" })` |
| `#[deprecated(since = "X", note = "Y")]` + `#[plexus_macros::removed_in("Z")]` | `Some(DeprecationInfo { since: "X", removed_in: "Z", message: "Y" })` |
| `#[deprecated(since = "X", note = "Y", removed_in = "Z")]` (if the author wrote this anyway) | `Some(DeprecationInfo { since: "X", removed_in: "Z", message: "Y" })` — macro accepts it even though rustc ignores the unknown key |
| `#[plexus_macros::removed_in("Z")]` alone, no `#[deprecated]` | Compile error — `removed_in` is only meaningful in combination with deprecation. |

"unspecified" (as a literal string) is the fallback value for `removed_in` when Rust's built-in `#[deprecated]` is used without the companion attribute.

## Required behavior

Extend `plexus-macros::activation::generate` so each emitted `MethodSchema` carries a `MethodRole` reflecting its source annotation:

| Annotation | Emitted `MethodRole` |
|---|---|
| `#[plexus_macros::method]` | `Rpc` |
| `#[plexus_macros::child]` with no `name` arg | `StaticChild` |
| `#[plexus_macros::child(name = "...")]` | `DynamicChild { list_method: None, search_method: None }` |
| `#[plexus_macros::child(name = "...", list = "list_fn")]` | `DynamicChild { list_method: Some("list_fn"), search_method: None }` |
| `#[plexus_macros::child(name = "...", list = "list_fn", search = "search_fn")]` | `DynamicChild { list_method: Some("list_fn"), search_method: Some("search_fn") }` |
| Hand-written `fn foo` with no macro attribute, in an `#[activation]` impl | Skipped — not emitted into `methods`. Unchanged from today. |

For each emitted `MethodSchema`, the macro additionally captures deprecation metadata from the surrounding attributes:

| Scope | Behavior |
|---|---|
| `#[deprecated]` on the method itself | Populates `MethodSchema.deprecation` per the table in Context. |
| `#[deprecated]` on the activation (`impl` block or `#[plexus_macros::activation]` target) | Populates a future activation-level `deprecation` surface — for this ticket, ensure the attribute is parsed without error; emission onto the activation's `PluginSchema` is IR-5's concern. Acceptance 4 below verifies parse-only. |
| `#[plexus_macros::removed_in("...")]` on a method | Combines with `#[deprecated]` as per the table. If `#[deprecated]` is absent, compile error with message naming `#[deprecated]` as the required companion. |

**Regression:** activations that use the legacy `children = [...]` attribute (which has not been deprecated through hard removal) continue to emit a correct schema. Method roles are additive: legacy `children = [...]` still produces the same `PluginSchema.children: Vec<ChildSummary>` it produces today. The emitted `methods` list in such a schema may or may not contain `StaticChild`-role entries depending on whether the author also annotated the corresponding accessor methods — either outcome is acceptable for this ticket (the shim in IR-4 reconciles).

## Risks

| Risk | Mitigation |
|---|---|
| File-boundary concurrency with IR-5. | IR-5 also modifies `plexus-macros` attribute-parsing and codegen code. Both tickets touch `src/activation.rs` (or the equivalent codegen entry point). **Serialize: land IR-3 first, then IR-5 rebases.** Do not attempt to run IR-3 and IR-5 concurrently. |
| Authors combining `#[deprecated]` on a method with legacy `children = [...]` on the activation. | Supported — deprecation metadata on a method is independent of the activation-level children list. Tested in acceptance criterion 5. |
| `#[plexus_macros::removed_in("X")]` placed on a non-method item (e.g., a struct). | Compile error. Out of scope for this ticket; IR-5 covers deprecation on structs/fields. |
| Author uses `#[deprecated(since = "...", removed_in = "...")]` with rustc's own `removed_in` key — rustc may emit an "unknown key" warning. | Document in the companion attribute's rustdoc: recommended path is the companion attribute. Macro accepts both shapes silently. |

## What must NOT change

- All existing CHILD-3 / CHILD-4 / CHILD-5 / CHILD-6 / CHILD-8 fixtures continue to pass.
- `PluginSchema.children` and `PluginSchema.is_hub` are still populated on emitted schemas (the shape of that population changes in IR-4; for this ticket, they remain populated the way they are today).
- `ChildCapabilities` bitflags on generated routers are unchanged.
- Hand-written `fn plugin_children(&self) -> Vec<ChildSummary>` continues to suppress synthesis (per CHILD-8's behavior). No reserved-name check.
- Legacy `children = [field_a, field_b]` attribute syntax continues to compile without warnings or errors.
- Substrate `cargo build --workspace` passes after this ticket lands, with no activation source edits required.

## Acceptance criteria

1. `cargo build -p plexus-macros` succeeds.
2. `cargo test -p plexus-macros` succeeds — all existing fixtures pass, and new fixtures (below) pass.
3. A trybuild fixture with an `#[activation]` impl containing one `#[method]`, one `#[child]` (no name), and one `#[child(name = "...", list = "list_fn", search = "search_fn")]` produces a `MethodSchema` list whose roles are, in order, `Rpc`, `StaticChild`, and `DynamicChild { list_method: Some("list_fn"), search_method: Some("search_fn") }`.
4. A trybuild fixture applying `#[deprecated(since = "0.5", note = "migrate")]` to an `impl Activation` block compiles (parse-only; emission onto the activation's schema is IR-5's concern).
5. A trybuild fixture with `#[deprecated(since = "0.5", note = "use bar")]` + `#[plexus_macros::removed_in("0.6")]` on a `#[method]` produces a `MethodSchema` whose `deprecation` field equals `Some(DeprecationInfo { since: "0.5".into(), removed_in: "0.6".into(), message: "use bar".into() })`.
6. A trybuild fixture with `#[plexus_macros::removed_in("0.6")]` on a method that is **not** also `#[deprecated]` fails to compile. The error message names `#[deprecated]` as the required companion attribute.
7. A trybuild fixture combining the legacy `#[activation(children = [body])]` syntax with role-tagged methods compiles and emits a `PluginSchema` with a populated `methods` list (roles reflecting the per-method attributes) and a populated `children: Vec<ChildSummary>`. Both surfaces coexist.
8. Substrate `cargo build --workspace` and `cargo test -p plexus-substrate` pass with no source edits to substrate activations.

## Completion

- PR against `plexus-macros` extending codegen to emit `MethodRole` and `DeprecationInfo` on each `MethodSchema`, adding the `#[plexus_macros::removed_in]` companion attribute, and adding the trybuild fixtures above.
- PR description includes `cargo test -p plexus-macros` and `cargo build -p plexus-substrate` output — all green.
- Ticket status flipped from `Ready` → `Complete` in the same commit.
- PR notes that IR-4 and IR-8 are unblocked, and that IR-5 can rebase now.
