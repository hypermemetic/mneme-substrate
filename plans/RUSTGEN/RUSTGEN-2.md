---
id: RUSTGEN-2
title: "hub-codegen Rust: per-namespace types.rs generation (parity with TS types.ts)"
status: Pending
type: implementation
blocked_by: [RUSTGEN-S01]
unlocks: [RUSTGEN-3]
severity: High
target_repo: hub-codegen
---

## Problem

The current Rust backend's `generator/rust/types.rs` emits a single `src/types.rs` with only the core transport types (`PlexusStreamItem`, `StreamMetadata`, `PlexusError`). Per-namespace user types (structs, tagged-union enums, type aliases) are glued into the per-namespace modules alongside methods, not into a dedicated namespace-scoped types file.

The TypeScript backend separates concerns: `types.ts` at the top level (core transport types) and a `<namespace>/types.ts` per namespace. This split lets namespace consumers import types without dragging method deps, and it lets `index.ts` / `lib.rs` re-export types cleanly.

This ticket aligns the Rust backend: a top-level `src/types.rs` for core transport types (unchanged shape), plus a per-namespace `src/<namespace>/types.rs` for that namespace's user types. Generated from the IR's `ir_types` map, filtered by `td_namespace`.

## Context

**TS reference:** `hub-codegen/src/generator/typescript/types.rs` — the codegen module. Particularly the `generate_namespace_types` function (by name or equivalent) and its handling of:

- Structs → Rust `struct` with `#[derive(Debug, Clone, Serialize, Deserialize)]`
- Enums with variant data → Rust `enum` with `#[serde(tag = "type", rename_all = "...")]` for tagged unions
- Type aliases → Rust `type Alias = Inner;`
- Cross-namespace type refs → `use crate::<other_ns>::types::<TypeName>;` imports
- Optional fields (TypeRef::RefOptional) → `Option<T>`
- Array fields (TypeRef::RefArray) → `Vec<T>`
- Strong typing passthrough: when ST's newtypes ship, schema types carrying those newtypes (`SessionId`, `GraphId`, etc.) must be emitted as the newtype, not unwrapped. Currently ST hasn't shipped — but this ticket must NOT actively strip type information that would become a newtype. Treat `TypeRef::RefNamed` with a known schema type as a schema-driven name, not a primitive.

**RUSTGEN-S01 outcome** (consumed before this ticket is promoted): pins where the `serde` / `chrono` / other type-system deps live (inline vs sibling crate). This affects the `use ...` preamble at the top of each generated `types.rs` file.

## Required behavior

For each namespace `ns` with at least one entry in `ir.ir_types` having `td_namespace == ns`, emit a file `src/<ns_path>/types.rs` where `<ns_path>` is the dotted-to-slash conversion (`hyperforge.org.hypermemetic` → `hyperforge/org/hypermemetic`).

The top-level `src/types.rs` continues to emit core transport types only (unchanged from current).

| Input (IR shape) | Generated Rust |
|---|---|
| `TypeDef { td_kind: KindStruct { ks_fields } }` | `#[derive(Debug, Clone, Serialize, Deserialize)] pub struct <Name> { <fields> }` |
| `TypeDef { td_kind: KindEnum { ke_variants, ke_tagged: true } }` | `#[derive(Debug, Clone, Serialize, Deserialize)] #[serde(tag = "type", rename_all = "snake_case")] pub enum <Name> { <variants with fields> }` |
| `TypeDef { td_kind: KindEnum { ke_variants, ke_tagged: false } }` | C-style enum (no variant data) |
| `TypeDef { td_kind: KindAlias { ka_target } }` | `pub type <Name> = <target>;` |
| `FieldDef { fd_type: RefOptional(inner) }` | `pub <name>: Option<<inner>>` |
| `FieldDef { fd_type: RefArray(inner) }` | `pub <name>: Vec<<inner>>` |
| `FieldDef { fd_type: RefNamed(qn) }` where `qn.namespace()` differs from this one | Imports `use crate::<other_ns>::types::<Name>;` at the top of the file |
| Field or type with `td_deprecation: Some(info)` (IR-7) | `#[deprecated(since = "...", note = "...")]` attribute; a `DeprecationWarning` is pushed when `emit_deprecation` is true |
| Doc comments from `td_description` / `fd_description` | `/// <text>` comments |

Imports are deterministic: sorted alphabetically, grouped (`std::` first, then `crate::`, then external deps).

Cross-namespace type refs use `use crate::<ns_path>::types::<Name>;` — not `super::` relative paths, because the namespace tree is flat under `crate::`.

## Risks

| Risk | Mitigation |
|---|---|
| Cross-namespace imports collide on type names. Two namespaces both define a `Status` enum — importing both in a third namespace causes a name collision. | Always alias on import when a name collision is detected: `use crate::<ns_a>::types::Status as <NsA>Status;`. Detection: walk the imports and dedup by local name. |
| ST's newtypes (`SessionId`, etc.) land as `TypeRef::RefNamed` referring to crate-local type aliases. If the Rust backend emits `type SessionId = String;` inline, but ST ships a distinct newtype in a shared crate later, the aliases collide with the real newtype. | Out of this ticket's scope to solve. Pin the convention: if `ir_types` contains a typedef for `SessionId`, emit it as declared. If another epic later introduces a shared `plexus-domain` crate with newtypes, that epic is responsible for deleting the inline emissions via schema-level marking — not this ticket. |
| Enum variant with single unnamed field (tuple variant) needs `#[serde(untagged)]` or manual flattening. IR doesn't currently distinguish tagged-union-with-one-field from tuple-variant. | Rust backend defers to the IR's `ks_tagged` / `ke_tagged` flags. If a variant has one unnamed field, emit as `Variant(Type)` in a non-tagged enum; or `#[serde(rename)] Variant { field: Type }` in a tagged enum. TS backend's handling is the reference. |
| Determinism across regens. HashMap iteration order in Rust is random. | Sort every collection iteration (types by name, fields by declaration order preserved via `Vec`, imports alphabetically). Acceptance 6 pins this. |

## What must NOT change

- Top-level `src/types.rs` content byte-identical to current for core transport types (`PlexusStreamItem`, `StreamMetadata`, `PlexusError`). This ticket moves namespace types INTO per-namespace files, not OUT of the top-level file.
- `GenerationResult`'s `files: HashMap<String, String>` keys shift: fewer entries at top level, more per-namespace entries. Consumers of `GenerationResult` that iterate `files` receive the new layout — that's expected.
- TypeScript backend — unchanged.
- `TypeDef`, `TypeKind`, `FieldDef`, `VariantDef` shapes in `ir.rs` — unchanged.
- Deprecation-warning emission (IR-7) behavior — types and fields with `td_deprecation` still produce `DeprecationWarning` entries; file path in the warning points at the new per-namespace location.

## Acceptance criteria

1. `cargo build -p hub-codegen` and `cargo test -p hub-codegen` succeed.
2. A fixture IR with types in three namespaces (`a`, `a.b`, `c`) produces generated output with three per-namespace types files: `src/a/types.rs`, `src/a/b/types.rs`, `src/c/types.rs`, each containing only the types for its own namespace.
3. A fixture with a struct in namespace `a` whose field references a type declared in namespace `b` produces an import at the top of `src/a/types.rs`: `use crate::b::types::<Name>;`.
4. A fixture with two namespaces both declaring a type named `Status` and a third namespace that references both produces an import block with aliased imports: `use crate::a::types::Status as AStatus; use crate::b::types::Status as BStatus;` (or equivalent alias scheme).
5. A fixture with a deprecated type produces `#[deprecated(since = "X", note = "...")]` above the type definition when `emit_deprecation = true`; without `emit_deprecation` no attribute is emitted.
6. Running the generator twice against the same IR produces byte-identical output for every per-namespace types file (determinism gate).
7. A fixture with a tagged-union enum produces a Rust enum with `#[serde(tag = "type", rename_all = "snake_case")]` and variant types matching the IR.
8. A fixture with a type alias (`KindAlias`) produces `pub type <Name> = <target>;`.
9. A fixture with a schema type whose name matches an ST-pinned newtype (e.g., `SessionId`, `GraphId`) emits the typedef as declared in the IR — NOT silently stripped or renamed (forward-compat pin with ST epic).

## Completion

PR against hub-codegen. CI green. PR description includes before/after diff of the generated output for a fixture with multi-namespace types, showing the new per-namespace `types.rs` files. Existing golden fixtures for the Rust backend may need reblessing; if so, the reblessed diff is limited to relocation of type definitions (no semantic change). Status flipped from `Ready` to `Complete` in the same commit.
