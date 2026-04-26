---
id: IR-8
title: "Substrate: Solar migrates tests to method-role IR; mark plugin_children deprecated"
status: Complete
type: implementation
blocked_by: [IR-3, IR-4]
unlocks: []
severity: Medium
target_repo: plexus-substrate
---

## Problem

Solar is substrate's reference hub activation (post-CHILD-7). Its tests today read the deprecated `plugin_schema().is_hub` boolean and the deprecated `plugin_schema().children: Vec<ChildSummary>` list. Both fields are marked `#[deprecated]` by IR-4 — Solar's test suite will emit deprecation warnings on every run, and the tests are a textbook example of "schema consumer that has not migrated to the new role-based API." Solar also has a hand-written `fn plugin_children(&self) -> Vec<ChildSummary>` that continues to work (backward compat per IR-4) but is semantically redundant with the role-tagged `#[child]` methods.

Migrate Solar's tests to the role-based query API and annotate its hand-written `plugin_children` with `#[deprecated]` so the deprecation surfaces through synapse (IR-6) and synapse-cc (IR-7). The wire behavior of Solar stays identical: pre-IR consumers see the same `children` and `is_hub` on the wire; `list_children` still returns the planet stream; `get_child("planet/earth")` still resolves.

## Context

Target crate: `plexus-substrate` at `/Users/shmendez/dev/controlflow/hypermemetic/plexus-substrate`. Target activation: Solar, at `src/activations/solar/` (the file paths are implementation details — the observable target is "Solar activation").

**Post-IR-3/IR-4 schema surface Solar tests should migrate to:**

| Old query | New query |
|---|---|
| `solar.plugin_schema().is_hub` | `solar.plugin_schema().is_hub()` (helper method from IR-2) |
| `solar.plugin_schema().children` (to enumerate child names / count) | Filter `solar.plugin_schema().methods` by `role != Rpc`; read `MethodSchema.name` |
| `solar.plugin_schema().children.iter().any(\|c\| c.name == "planet")` | `solar.plugin_schema().methods.iter().any(\|m\| matches!(m.role, MethodRole::DynamicChild { .. }) && m.name == "planet")` |

**`plugin_children` deprecation on Solar:**

Solar's hand-written `fn plugin_children(&self) -> Vec<ChildSummary>` is annotated:

```rust
#[deprecated(since = "0.5", note = "Solar's children are derivable from #[child]-tagged methods. This override is retained for backward compatibility until plugin_children is removed from the schema.")]
#[plexus_macros::removed_in("0.6")]
fn plugin_children(&self) -> Vec<ChildSummary> { /* unchanged body */ }
```

The deprecation flows into `PluginSchema.deprecation` via IR-5's activation-level scanner — but only if `#[deprecated]` is on the `impl Activation for Solar` block. Putting `#[deprecated]` on a **single method** of an impl does not deprecate the whole activation; it scopes to that method. Pin: the goal here is to deprecate the `plugin_children` override specifically, not all of Solar. IR-5 scans method-level `#[deprecated]` on `#[method]`-annotated methods; `plugin_children` is **not** `#[method]`-annotated (it's a plain method the macro recognizes by name). Therefore, for this ticket, the `#[deprecated]` attribute on `plugin_children` produces the standard rustc deprecation warning (visible at compile time for callers) and synapse does **not** automatically surface it through the schema.

To surface it through synapse, this ticket additionally adds a descriptive entry via one of the new IR surfaces: if the activation-level `PluginSchema.deprecation` (from IR-5) is the only available mechanism, Solar opts out of that — Solar as a whole is not deprecated. Instead, this ticket accepts that the Solar-specific `plugin_children` override's deprecation is **compile-time-only** for Rust callers, not wire-surfaced through synapse. The deprecated wire-level `children` field (marked deprecated by IR-4) is what synapse surfaces generically; Solar-specific migration guidance does not need its own wire channel.

Pin this decision: the `#[deprecated]` on Solar's `plugin_children` is a compile-time hint for anyone building against substrate. synapse's deprecation UI surfaces the generic `children` field deprecation (from IR-4), not a Solar-specific override-exists warning.

## Required behavior

**Test migration:**

For every Solar test file (under substrate's test harness) that reads `plugin_schema().is_hub` or `plugin_schema().children`:

| Current call | Replacement |
|---|---|
| `.plugin_schema().is_hub` (field access) | `.plugin_schema().is_hub()` (method call on the helper from IR-2) |
| `.plugin_schema().children` (enumeration / count) | `.plugin_schema().methods.iter().filter(\|m\| !matches!(m.role, MethodRole::Rpc))` or equivalent |
| `.plugin_schema().children.iter().find(\|c\| c.name == X)` | Analogous filter on methods, matching by name |

Tests assert the same semantic properties they did before (Solar has exactly one child gate named `planet`, Solar is a hub, etc.). The assertion shape changes from field-access to helper/method filtering; the assertion content stays the same.

**`plugin_children` deprecation:**

Annotate Solar's hand-written `plugin_children` with `#[deprecated(since = "0.5", note = "...")]` + `#[plexus_macros::removed_in("0.6")]`. The full note text is pinned in Acceptance 3.

**Wire regression:**

Solar's serialized `PluginSchema` must be byte-identical to its pre-ticket serialization — confirmed by IR-4's golden snapshot (which this ticket does not modify). IR-4 established the snapshot; IR-8 must pass it unchanged.

**Runtime regression:**

| Operation | Expected result |
|---|---|
| `solar.list_children().await` | Returns a `BoxStream<'_, String>` yielding the same planet name sequence as before. |
| `solar.get_child("earth")` (or whatever the current child lookup key is) | Returns the same `Option<Handle>` as before. |
| Any orbit / observe / nested method invocation on Solar | Returns the same result as before. |
| Synapse rendering of Solar's schema | Shows `solar` with its child gate and method list, identical structure to pre-ticket. IR-6's deprecation markers may decorate the deprecated `children` / `is_hub` fields in synapse's detail view, but the structural output is unchanged. |

## Risks

| Risk | Mitigation |
|---|---|
| `#[deprecated]` on `plugin_children` emits warnings throughout substrate's build if any code inside the substrate workspace calls `solar.plugin_children()` directly. | Add `#[allow(deprecated)]` at any internal call sites. These should be rare — the macro's generated schema-construction code is the main caller, and IR-4 already places `#[allow(deprecated)]` there. Acceptance 4 verifies a clean substrate build. |
| `.plugin_schema().is_hub()` is a method; rust-analyzer may flag the replacement as a possible-rename-candidate for the old field access. The old field `is_hub: bool` remains on the struct (deprecated). Both compile — but callers reading the old field emit a warning. | Migrate each test and verify the warning disappears. Rust-analyzer's autofix is not in scope here. |
| Solar's hand-written `plugin_children` body relies on internal state (bodies registry) that the derivation helper (IR-4) cannot access. Removing the override would break Solar. | This ticket does **not** remove the override — only deprecates it. The override continues to run; its `#[deprecated]` annotation only warns Rust callers. |
| New test-assertion code using `MethodRole` pattern matching may miss future variants when `MethodRole` is extended. | `MethodRole` is `#[non_exhaustive]` (IR-2). Pattern matches that care only about "is it a child?" use `matches!(m.role, MethodRole::StaticChild | MethodRole::DynamicChild { .. })` with a catch-all implicit. Test code follows the same pattern. |

## What must NOT change

- Solar's wire-serialized `PluginSchema` — identical to pre-ticket, verified by IR-4's golden snapshot (Acceptance 5 re-verifies by running the snapshot test).
- Solar's `list_children`, `search_children`, and `get_child` behavior — identical response streams / values.
- Solar's runtime hash output (the existing `ChildSummary.hash` contents) — unchanged. HASH epic owns the hash semantics; IR-8 does not touch them.
- Solar's hand-written `plugin_children` body — only its attributes change. The code inside the function is not touched.
- Any non-Solar activation in substrate — untouched. If other activations' tests also read `plugin_schema().is_hub` or `plugin_schema().children`, their migration is out of scope for this ticket (follow-up).

## Acceptance criteria

1. `cargo build -p plexus-substrate` succeeds.
2. `cargo test -p plexus-substrate` succeeds — specifically every Solar-targeted test passes.
3. Solar's hand-written `plugin_children` method has both attributes:
   - `#[deprecated(since = "0.5", note = "Solar's children are derivable from #[child]-tagged methods. This override is retained for backward compatibility until plugin_children is removed from the schema.")]`
   - `#[plexus_macros::removed_in("0.6")]`
   Verified by a test that reads Solar's source metadata via `plexus_macros` introspection, or by a grep-style assertion that the source file contains both attribute strings on the same function.
4. `cargo build -p plexus-substrate` emits zero `deprecated` warnings from substrate's own source. (Substrate's build is clean. Any `#[deprecated]` surfaces Solar reads are wrapped in `#[allow(deprecated)]` or migrated.)
5. The golden snapshot test introduced in IR-4 passes — Solar's serialized `PluginSchema` is byte-identical to the pre-IR-8 snapshot.
6. A Solar runtime integration test asserts:

   | Call | Expected |
   |---|---|
   | `solar.list_children().await.unwrap().collect::<Vec<_>>().await` | The same planet-name list as the pre-ticket baseline (list is captured as a committed fixture; the test compares against it). |
   | `solar.get_child(<some_planet_name>)` | `Some(_)` (specifically, a valid `Handle`). |
   | `solar.plugin_schema().is_hub()` | `true` |
   | `solar.plugin_schema().methods.iter().filter(\|m\| !matches!(m.role, MethodRole::Rpc)).count()` | 1 (matches the number of `#[child]`-tagged methods on Solar). |

7. Zero Solar tests reference `plugin_schema().is_hub` as a field (only as a method call), and zero Solar tests reference `plugin_schema().children` as a field. Verified by a code-search assertion (e.g., a test that fails if `grep -r "plugin_schema().is_hub " solar_tests/` finds any match; exact grep phrasing is an implementation detail — the observable is that the field-access patterns are absent from Solar's test code).

## Completion

- PR against `plexus-substrate` migrating Solar's tests and annotating `plugin_children`.
- PR description includes `cargo build -p plexus-substrate` output (zero deprecation warnings from substrate's own code), `cargo test -p plexus-substrate` output, and the IR-4 golden snapshot assertion output — all green.
- Ticket status flipped from `Ready` → `Complete` in the same commit.
