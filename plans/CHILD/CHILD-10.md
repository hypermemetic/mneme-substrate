---
id: CHILD-10
title: "Explicit plugin_children override as a child macro extension"
status: Superseded
superseded_by: IR-1
type: implementation
blocked_by: []
unlocks: []
severity: Medium
target_repo: plexus-macros
---

**Superseded by [IR-1](../IR/IR-1.md).** The unifying IR epic removes `plugin_children` from the schema entirely, obviating the override concept this ticket designed.

## Problem

CHILD-8 synthesizes `plugin_children()` from static `#[plexus_macros::child]` methods. When an author needs a different source (e.g., Solar computes summaries from runtime config), today's escape hatch is to hand-write `fn plugin_children(&self) -> Vec<ChildSummary>` on the impl and rely on the macro's "already defined" detection. That works but it is magical in the worst way: a method with a specific name silently overrides synthesis, a typo falls back to synthesis with empty hashes, and a reader has to know the unwritten macro contract to understand why the method exists.

Fix it two ways:

1. Make the override declarative via a macro attribute. The macro sees the attribute, validates the target, emits a clear error on mismatch.
2. **Reserve the function name `plugin_children`.** Writing `fn plugin_children(&self)` without the explicit override attribute produces a compile error naming the macro extension to apply. No more silent, name-matching override.

## Context

Current behavior (post-CHILD-8):
- If impl defines `fn plugin_children(&self) -> Vec<ChildSummary>` (exact name and signature) → macro skips synthesis, emits `self.plugin_children()` in the schema constructor.
- Otherwise → macro synthesizes from static `#[child]` methods.

The rename-typo risk is real: `fn plugin_childrn` (typo) silently falls back to synthesis with empty hashes, which passes type-check but produces wrong schemas at runtime.

## Required behavior

Introduce an explicit child-summary override. Three shapes supported:

**Shape 1: attribute arg on the activation attr pointing at a method.**

```rust
#[plexus_macros::activation(namespace = "solar", summaries = "body_summaries")]
impl Solar {
    fn body_summaries(&self) -> Vec<ChildSummary> { /* ... */ }
}
```

The macro looks up `body_summaries` in the same impl block, validates its signature (`fn(&self) -> Vec<ChildSummary>` or async variant), and emits `self.body_summaries()` as the summary source in `plugin_schema_body`.

**Shape 2: attribute on the method directly.**

```rust
#[plexus_macros::activation(namespace = "solar")]
impl Solar {
    #[plexus_macros::child_summaries]
    fn body_summaries(&self) -> Vec<ChildSummary> { /* ... */ }
}
```

Macro discovers the annotated method automatically. Exactly one `#[child_summaries]` method permitted per impl.

**Shape 3: inline argument on the `#[child]` attribute.**

```rust
#[plexus_macros::activation(namespace = "solar")]
impl Solar {
    #[plexus_macros::child(list = "body_names", summaries = "body_summaries")]
    async fn body(&self, name: &str) -> Option<CelestialBodyActivation> { /* ... */ }

    fn body_summaries(&self) -> Vec<ChildSummary> { /* ... */ }
}
```

Shape 3 is the most local (lives on the dynamic child method it describes). Prefer Shape 3 when the override is semantically tied to a specific dynamic `#[child]` gate. Fall back to Shape 1 or 2 when the override describes the whole activation's child set.

**Pin in this ticket: support all three shapes.** Each is a different authorial convenience; they don't conflict. Present all three in one fixture and they compose the same concrete `plugin_schema`.

Macro-expansion rules:

| Condition | Outcome |
|---|---|
| No `summaries` referenced, no `fn plugin_children` present | Macro synthesizes from static `#[child]` methods (unchanged CHILD-8 behavior). |
| `summaries = "name"` on activation attr or `#[child(summaries = "name")]` | Macro validates the named method exists in the same impl with signature returning `Vec<ChildSummary>` (sync or async), emits `self.name()` as the summary source. Synthesis skipped. |
| `#[plexus_macros::child_summaries]` on a method | Discovered automatically; same effect as Shape 1. |
| A method named `plugin_children` exists on the impl and **none of the explicit override shapes** references it | **Compile error** with the exact wording: `"\`plugin_children\` is a reserved function name on activations. If you meant to override the synthesized plugin_children, apply #[plexus_macros::child_summaries] above this method."` |
| A method named `plugin_children` exists on the impl AND IS referenced by one of the override shapes | Permitted — the author explicitly opted in via the macro contract. |
| Referenced method doesn't exist, or signature wrong | Clear compile error. |
| Multiple of Shapes 1/2/3 present on the same impl | Compile error — force the author to pick one canonical source. |

`plugin_children` is reserved. There is no silent name-matching override. CHILD-7's Solar, which currently hand-writes `fn plugin_children(&self) -> Vec<ChildSummary>` without the explicit attribute, will stop compiling until it is updated to use one of the three override shapes. That migration lands in the same PR as this ticket.

## Risks

| Risk | Mitigation |
|---|---|
| Three shapes is three things to document | Rustdoc on `#[plexus_macros::child]` enumerates them and the "when to use which" guidance. |
| Shape 3 (`summaries = "name"` on `#[child]`) creates the illusion that the referenced method is per-child instead of whole-impl | Doc: the referenced method produces the summaries for the ENTIRE impl, not just that `#[child]`. It's just placed near the child it most naturally describes. |
| Reserving `plugin_children` breaks CHILD-7's Solar migration | Solar migration to Shape 3 (`#[child(summaries = "plugin_children")]` or rename to `body_summaries` + one of the shapes) is included in this ticket, not deferred. |
| Other activations outside substrate that named a method `plugin_children` by coincidence | This is a plexus-macros-level contract — applies only to impls under `#[plexus_macros::activation]`. Free-standing methods on non-activation structs are unaffected. |

## What must NOT change

- CHILD-8's synthesis path continues to apply when no override is present and no method named `plugin_children` exists.
- All CHILD-3 / CHILD-4 / CHILD-7 / CHILD-8 fixtures pass — CHILD-7 may require a one-line update to the Solar migration to use the new explicit override attribute (done in this ticket).
- Substrate `cargo build --workspace` passes after Solar's one-line update.
- Free-standing methods named `plugin_children` on structs outside an `#[plexus_macros::activation]` impl are unaffected.

## Acceptance criteria

1. `cargo build -p plexus-macros` and `cargo test -p plexus-macros` succeed.
2. Fixtures for each of Shapes 1, 2, 3 produce identical generated `plugin_schema` given identical summary methods.
3. A trybuild fixture with a method named `plugin_children` on a `#[plexus_macros::activation]` impl, and no explicit override attribute referencing it, fails to compile. The error message contains the exact phrase `` `plugin_children` is a reserved function name on activations `` and names `#[plexus_macros::child_summaries]` as the fix.
4. A fixture combining two of Shapes 1/2/3 on the same impl fails to compile with an error naming the ambiguity.
5. A fixture referencing a nonexistent method (`summaries = "typo"`) fails to compile with a clear error.
6. A fixture where `#[plexus_macros::child_summaries]` is placed on a method named `plugin_children` compiles and works — reserved-name check is about opt-in, not the name itself.
7. CHILD-7's Solar migration is updated in this ticket's PR to use one of the three override shapes. The migration is a single-file diff on `src/activations/solar/activation.rs` plus whatever fixture and doc changes accompany it.
8. Substrate `cargo build --workspace` and `cargo test -p plexus-substrate` pass after Solar is updated.
9. A free-standing `fn plugin_children(&self)` on a struct that is NOT under `#[plexus_macros::activation]` (e.g., a helper or unrelated type) compiles without any macro intervention.

## Completion

PR(s) against `plexus-macros` and `plexus-substrate`. Plexus-macros changes land the reserved-name check and the three override shapes. Substrate changes update Solar to use the new shape. CI green on both. Status flipped to Complete in the same commit as the plexus-macros code.
