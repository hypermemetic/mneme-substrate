---
id: IR-4
title: "Backward-compat shim: populate deprecated PluginSchema fields from methods"
status: Complete
type: implementation
blocked_by: [IR-2, IR-3]
unlocks: [IR-6, IR-7, IR-8]
severity: High
target_repo: plexus-core
---

## Problem

Post-IR-3, every macro-generated `MethodSchema` carries a `MethodRole`. The deprecated side-tables — `PluginSchema.children: Vec<ChildSummary>`, `PluginSchema.is_hub: bool`, and the `ChildCapabilities` bitflags on generated routers — are now fully derivable from the role-tagged method list. The population logic is currently split: `plexus-macros` emits `children` and `is_hub` from a separate `plugin_children()` path, and `ChildCapabilities` from per-method attribute arguments. Consumers reading the schema see data that could disagree between the method list and the side-tables.

Centralize the derivation in `plexus-core` so the side-tables are a deterministic function of the method list. Mark the side-tables deprecated with structured `DeprecationInfo`. Keep them on the wire so pre-IR consumers continue to work.

## Context

Target crate: `plexus-core` at `/Users/shmendez/dev/controlflow/hypermemetic/plexus-core`.

After this ticket:

- `PluginSchema.children: Vec<ChildSummary>` is annotated `#[deprecated(since = "0.5", note = "Derive from MethodRole on MethodSchema; this field is populated from role-tagged methods during the transition window.")]` and carries a `removed_in = "0.6"` marker (surfaced via the crate's deprecation metadata convention — see IR-2).
- `PluginSchema.is_hub: bool` is similarly annotated with `since = "0.5"`, `removed_in = "0.6"`.
- `ChildCapabilities` bitflags are annotated `#[deprecated(since = "0.5", note = "Use MethodRole::DynamicChild { list_method, search_method } on the corresponding gate method.")]`, `removed_in = "0.6"`.
- A single helper in `plexus-core` — call it `derive_legacy_fields(methods: &[MethodSchema]) -> (Vec<ChildSummary>, bool)` (or equivalent API) — produces the deprecated fields from the method list. The helper's exact name and placement are an implementation detail; the observable contract is below.
- `plexus-macros` (unchanged in this ticket) continues to emit `PluginSchema` with populated `children` and `is_hub`, but the population logic in the macro now calls `derive_legacy_fields` on the method list rather than reading a hand-written `plugin_children` or inspecting attributes separately.

**Author-written `fn plugin_children(&self) -> Vec<ChildSummary>` on an activation impl continues to work as a full override** (backward compat, preserving CHILD-8's behavior). No reserved-name check. No compile error. If an author has written it, the macro continues to call it and emit the result; IR-4 does not remove that path.

## Required behavior

**Central derivation helper:**

| Input | Behavior |
|---|---|
| `methods: &[MethodSchema]` where no method has `role != Rpc` | `(vec![], false)` — no children, not a hub. |
| `methods` containing one method with `role = StaticChild` named `"body"` | Returns `children` containing one `ChildSummary` with `name: "body"`, `is_hub: true`. Other `ChildSummary` fields populated with default/empty values — the helper does **not** compute hashes or capabilities (those remain the responsibility of callers that want them; the transition shim pins zero-value placeholders where necessary). |
| `methods` containing one method with `role = DynamicChild { .. }` named `"planet"` | Same shape as above, with `name: "planet"`, `is_hub: true`. |
| `methods` containing a mix of roles | `children` contains one entry per non-`Rpc` method, preserving source order. `is_hub: true`. |

Precise `ChildSummary` field population in the helper — pin what the transition shim writes:

| `ChildSummary` field | Value written by shim |
|---|---|
| `name` | Method name. |
| `description` | The `MethodSchema.description`. |
| `hash` | Empty string (or current sentinel used by macros; see HASH-1 for the runtime-hash replacement story). |
| Any other existing fields | Default value. |

**Deprecation annotations:**

| Field / type | `since` | `removed_in` | Message |
|---|---|---|---|
| `PluginSchema.children` | `"0.5"` | `"0.6"` | `"Derive from MethodRole on MethodSchema."` |
| `PluginSchema.is_hub` | `"0.5"` | `"0.6"` | `"Use PluginSchema::is_hub() helper which reads MethodRole from methods."` |
| `ChildCapabilities` (the type) | `"0.5"` | `"0.6"` | `"Use MethodRole::DynamicChild { list_method, search_method } instead."` |

Each annotation uses Rust's `#[deprecated(since = ..., note = ...)]` plus whichever companion mechanism `plexus-core` has adopted for `removed_in` (IR-2 pins the convention — this ticket adheres to it).

**Golden snapshot regression:**

Add a golden-snapshot test (e.g., with the `insta` crate, already used in the substrate workspace, or an equivalent file-compare harness) that serializes each of substrate's activations' `PluginSchema` to JSON and asserts byte-identity against a committed snapshot. The snapshot is captured immediately before this ticket lands, and it must remain valid after the ticket lands. Fixture: one snapshot per activation under `plexus-core/tests/golden/` (or a mutually-acceptable path — the path is an implementation detail; the **existence of byte-identical snapshots** is the observable criterion).

## Risks

| Risk | Mitigation |
|---|---|
| `#[deprecated]` on a public field causes downstream builds to emit warnings that are escalated to errors by `-D warnings` in CI. | Document in the PR: downstream consumers pin `#[allow(deprecated)]` at the read site during the transition, or suppress the lint in their workspace. Acceptance 3 verifies substrate builds with its current warning policy. |
| Golden snapshot captures a macro emission quirk that changes between this ticket and IR-5. | The snapshot must be captured at IR-3's tip. IR-5's changes do not alter the `children` / `is_hub` fields (IR-5 adds deprecation metadata to new surfaces). If IR-5 regresses the snapshot, fix IR-5, not IR-4's snapshot. |
| Author has hand-written `fn plugin_children()` that returns a different set than the role-tagged methods would derive. | The hand-written override wins (backward compat); the derived helper is **not** called when the override is present. Acceptance 5 pins this. |
| `ChildCapabilities` being `#[deprecated]` at the type level breaks macro codegen that references the type by name. | Sprinkle `#[allow(deprecated)]` inside generated code blocks in `plexus-macros`; this is a macro-author responsibility to be handled in IR-3's follow-up or as a hygiene fix here. Acceptance 2 verifies `plexus-macros` builds without warnings being errors. |

## What must NOT change

- `PluginSchema.children`, `PluginSchema.is_hub`, and `ChildCapabilities` remain on the public API of `plexus-core` and continue to serialize on the wire — a pre-IR consumer reads them exactly as before.
- Hand-written `fn plugin_children(&self) -> Vec<ChildSummary>` on an activation impl still overrides macro synthesis (no reserved-name check, no compile error).
- Every substrate activation serializes to byte-identical `PluginSchema` JSON after this ticket lands — the golden snapshot test pins this.
- All CHILD-series tests continue to pass.
- No wire format changes; no field renames; no new required fields.

## Acceptance criteria

1. `cargo build -p plexus-core` succeeds.
2. `cargo build -p plexus-macros` succeeds — generated code compiles cleanly with the crate's current warning policy (deprecated type references inside generated code are `#[allow(deprecated)]`-annotated as needed).
3. `cargo build -p plexus-substrate` and `cargo test -p plexus-substrate` succeed with no source edits to substrate activations.
4. A unit test on the derivation helper verifies every row of the Required behavior table:

   | Input methods | Output `children` count | Output `is_hub` |
   |---|---|---|
   | Empty | 0 | `false` |
   | One `Rpc` | 0 | `false` |
   | One `StaticChild` named "body" | 1 (name `"body"`) | `true` |
   | One `DynamicChild { .. }` named "planet" | 1 (name `"planet"`) | `true` |
   | One `Rpc` + one `StaticChild` | 1 | `true` |

5. A trybuild fixture for an activation with a hand-written `fn plugin_children(&self) -> Vec<ChildSummary>` returning `vec![ChildSummary { name: "custom".into(), .. }]` and no `#[child]`-tagged methods compiles and produces a `PluginSchema` whose `children` equals the hand-written return value (not the derived empty list).
6. A golden snapshot test in `plexus-core` (or an agreed-upon location in the workspace) asserts that each substrate activation's serialized `PluginSchema` JSON is byte-identical to a committed snapshot captured pre-ticket.
7. Attempting to read `PluginSchema.children` or `PluginSchema.is_hub` or to name `ChildCapabilities` anywhere in downstream code (outside `#[allow(deprecated)]` blocks) produces a compiler warning whose text includes the deprecation `note` written in this ticket's Required behavior.
8. `PluginSchema::is_hub()` helper (added in IR-2) and the deprecated `PluginSchema.is_hub` field return the same value on every schema generated by today's substrate activations (verified by the golden snapshot test reading both and asserting equality).

## Completion

- PR against `plexus-core` adding `#[deprecated]` annotations, centralizing the derivation helper, and adding the golden snapshot test.
- PR may also touch `plexus-macros` to route macro-emitted population through the new helper; if so, that change is scoped minimally and noted in the PR description.
- PR description includes `cargo test -p plexus-core`, `cargo build -p plexus-macros`, and `cargo test -p plexus-substrate` output — all green.
- Ticket status flipped from `Ready` → `Complete` in the same commit.
- PR notes IR-6, IR-7, and IR-8 are unblocked.
