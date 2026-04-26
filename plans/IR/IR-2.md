---
id: IR-2
title: "plexus-core: add MethodRole + DeprecationInfo; extend MethodSchema"
status: Complete
type: implementation
blocked_by: []
unlocks: [IR-3, IR-5]
severity: High
target_repo: plexus-core
---

## Problem

`PluginSchema` today carries three parallel child descriptions: `methods: Vec<MethodSchema>`, `children: Vec<ChildSummary>`, and `is_hub: bool`, plus `ChildCapabilities` bitflags on generated routers. All three of the child-related surfaces are derivable from information that naturally belongs on the method itself: whether a method is an RPC, a static child accessor, or a dynamic child gate (and if dynamic, which sibling methods enumerate/search it). Without a role tag on `MethodSchema`, no consumer can answer "is this method a child accessor?" without consulting a separate side-table, and every downstream tool (synapse, synapse-cc, introspection clients) has to reconstruct the mapping.

Introduce the role tag and a structured deprecation envelope so downstream tickets (IR-3, IR-5) have concrete types to emit into.

## Context

Target crate: `plexus-core` at `/Users/shmendez/dev/controlflow/hypermemetic/plexus-core`.

Current `MethodSchema` (pre-IR) carries:

| Field | Type | Meaning |
|---|---|---|
| `name` | `String` | Method name |
| `params` | `Vec<ParamSchema>` | Declared parameters |
| `return_shape` | `ReturnShape` | Bare / Option / Result / Vec / Stream |
| `description` | `String` | Doc-comment-derived description |

Current `PluginSchema` carries `methods`, `children`, `is_hub`, plus the trait-level `ChildRouter::capabilities()`. Those three side-channels stay on the wire through the transition window — IR-4 owns the population logic once this ticket has landed the types.

Downstream consumers:

- **IR-3** reads `MethodRole` and emits role-tagged `MethodSchema` from `plexus-macros`.
- **IR-5** reads `DeprecationInfo` and emits populated `Option<DeprecationInfo>` on methods/activations/param fields.
- **IR-4** uses `MethodRole` queries (e.g., `is_hub()` helper) to populate the deprecated `children` / `is_hub` fields from the method list.

Serde: both `MethodRole` and `DeprecationInfo` must serialize via serde (the existing derivation approach on `MethodSchema` — `Serialize`, `Deserialize`, `Clone`, `Debug`, `PartialEq`).

## Required behavior

Add the following public types to `plexus-core`:

**`MethodRole`** — enum with three variants:

| Variant | Shape | Meaning |
|---|---|---|
| `Rpc` | unit | Method is an RPC endpoint (default). |
| `StaticChild` | unit | Method returns a child activation by static name (no lookup arg). |
| `DynamicChild { list_method, search_method }` | struct with two `Option<String>` fields | Method gates a dynamic child keyed by its argument; if present, `list_method` names a sibling method that lists keys, and `search_method` names a sibling method that searches keys. |

**`DeprecationInfo`** — struct with three public `String` fields:

| Field | Meaning |
|---|---|
| `since` | plexus-core version at which deprecation began (e.g., `"0.5"`). |
| `removed_in` | plexus-core version planned for removal (e.g., `"0.6"`). Not binding; serves as consumer-visible hint. |
| `message` | Human-readable migration guidance. |

**Extend `MethodSchema`**:

| New field | Type | Default | Meaning |
|---|---|---|---|
| `role` | `MethodRole` | `MethodRole::Rpc` | How this method participates in the graph. |
| `deprecation` | `Option<DeprecationInfo>` | `None` | If set, this method is deprecated. |

Both new fields must have sensible serde defaults so older serialized schemas (which omit them) deserialize cleanly as `role: Rpc, deprecation: None`.

**Query helper on `PluginSchema`:**

Add `PluginSchema::is_hub(&self) -> bool` that returns `true` iff **any** method has a `StaticChild` or `DynamicChild { .. }` role. During the transition window (pre-IR-4), this helper must return the same value as the existing `is_hub: bool` field on any schema the current macros produce — so a pre-IR schema deserialized today (all methods `Rpc`, `is_hub: true`) returns the appropriate value per the field. Pin: the helper's logic reads **only** `methods`, not the deprecated `is_hub` field. Existing callers of the field continue to work; new callers prefer the helper.

**Regression guarantees:**

| Surface | Post-ticket behavior |
|---|---|
| `ChildSummary` type | Unchanged — same fields, same serde shape. |
| `PluginSchema.children: Vec<ChildSummary>` | Still present, still populated by macros, still on the wire. |
| `PluginSchema.is_hub: bool` | Still present, still populated by macros, still on the wire. |
| `ChildCapabilities` bitflags | Unchanged. |
| `ChildRouter` trait | Unchanged. |
| Serde wire format for `PluginSchema` | Same fields plus new `role` / `deprecation` on each method. Old clients that ignore unknown fields deserialize successfully. |

## Risks

| Risk | Mitigation |
|---|---|
| Serde default for `MethodRole` when deserializing a pre-IR schema. | Derive `Default` returning `Rpc`; apply `#[serde(default)]` on `MethodSchema.role`. |
| Downstream crates (`plexus-macros`, substrate) pattern-match on `MethodRole` non-exhaustively. | Add `#[non_exhaustive]` to `MethodRole` so future variants don't break downstream match arms. |
| `PluginSchema::is_hub()` helper returns a different value than the existing `is_hub` field for some macro-generated schema. | Acceptance criterion 4 pins that the helper and the field agree on every schema generated by today's macros — verified by a test that enumerates every activation in substrate and asserts agreement. |

## What must NOT change

- `ChildSummary`, `ChildRouter`, `ChildCapabilities`, and `DynamicHub` surfaces are untouched.
- `PluginSchema.children`, `PluginSchema.is_hub`, `PluginSchema.namespace`, `PluginSchema.description`, `PluginSchema.version` remain with the same types and field names.
- Serde-deserializing a pre-IR `PluginSchema` (no `role` or `deprecation` fields on methods) succeeds and yields `MethodRole::Rpc` and `None` for each method.
- Wire format stays backward-compatible: a pre-IR consumer receiving a post-IR schema ignores the new fields and reads the rest correctly.
- Every substrate activation continues to compile and all substrate tests continue to pass.

## Acceptance criteria

1. `cargo build -p plexus-core` succeeds.
2. `cargo test -p plexus-core` succeeds.
3. `cargo build -p plexus-substrate` succeeds with no source edits to substrate's activations.
4. A unit test in `plexus-core` constructs a `PluginSchema` with mixed-role methods and asserts:

   | Methods present | `PluginSchema::is_hub()` returns |
   |---|---|
   | All `Rpc` | `false` |
   | At least one `StaticChild` | `true` |
   | At least one `DynamicChild { .. }` | `true` |
   | Mix of `Rpc` + `StaticChild` | `true` |
   | Empty `methods` | `false` |

5. A serde round-trip test deserializes a JSON `PluginSchema` with **no** `role` or `deprecation` fields on its methods and asserts each method's `role == MethodRole::Rpc` and `deprecation == None`.
6. A serde round-trip test round-trips a `PluginSchema` containing each of `Rpc`, `StaticChild`, and `DynamicChild { list_method: Some("list_x"), search_method: Some("search_x") }` variants and confirms equality before and after.
7. A serde round-trip test round-trips a `MethodSchema` with `deprecation: Some(DeprecationInfo { since: "0.5".into(), removed_in: "0.6".into(), message: "use MethodRole".into() })` and confirms equality before and after.
8. `MethodRole` and `DeprecationInfo` are exported from `plexus-core`'s public prelude (or root module), matching the convention used by `ChildCapabilities`.

## Completion

- PR against `plexus-core` adding the two new public types, the extended `MethodSchema`, the `PluginSchema::is_hub()` helper, and the unit tests.
- PR description includes `cargo build -p plexus-core`, `cargo test -p plexus-core`, and `cargo build -p plexus-substrate` output — all green.
- Ticket status flipped from `Ready` → `Complete` in the same commit as the code.
- PR notes that IR-3 and IR-5 are unblocked.
