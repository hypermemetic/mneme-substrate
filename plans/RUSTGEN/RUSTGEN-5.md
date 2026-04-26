---
id: RUSTGEN-5
title: "hub-codegen Rust: per-namespace client module generation (parity with TS namespaces.ts)"
status: Pending
type: implementation
blocked_by: [RUSTGEN-4]
unlocks: [RUSTGEN-6, RUSTGEN-7, RUSTGEN-8, RUSTGEN-9]
severity: High
target_repo: hub-codegen
---

## Problem

This is the "big one" — the Rust parallel to `generator/typescript/namespaces.rs` (778 lines). Per-namespace client module generation: every namespace with at least one method gets a `<namespace>/client.rs` emitting a struct implementing one typed method per `MethodDef`. Namespace-level `get(name)` accessors for static children return typed child clients. Dynamic-child gates emit the typed-handle scaffolding from IR-9 (skeleton today; real `Child` wiring lands in RUSTGEN-6).

The current Rust backend's `generator/rust/client.rs` conflates per-namespace modules with the base transport. This ticket extracts per-namespace generation into `generator/rust/namespaces.rs` and rewrites it to parallel the TS backend's structure and feature set.

## Context

**TS reference:** `hub-codegen/src/generator/typescript/namespaces.rs` — 778 lines. The key functions (by rough name in TS):

- `generate_namespaces(ir, filter, emit_deprecation, warnings)` — top-level entry, returns `HashMap<String, String>` of file paths to content.
- `collect_dynamic_child_target_namespaces(ir)` — IR-9 pre-pass to determine which namespaces need `ClientImpl` exported.
- `generate_namespace(namespace, methods, ir, export_impl, emit_deprecation, warnings)` — per-namespace emit.
- `generate_method(method, ...)` — per-method emit for `MethodRole::Rpc`.
- `generate_static_child(method, ...)` — per-method emit for `MethodRole::StaticChild`.
- `generate_dynamic_child(method, ...)` — per-method emit for `MethodRole::DynamicChild { list_method, search_method }`.

**Current Rust backend partial implementation:**

- `generator/rust/client.rs`'s `generate_namespace_modules_with_deprecation` and `generate_namespace_node` functions do some of this. They handle method emission for `MethodRole::Rpc` and partial handling of `MethodRole::DynamicChild` (emits a gate struct with `Child = serde_json::Value`).
- Static child handling is incomplete or missing — verify during implementation.
- No per-namespace `client.rs` separation yet — everything lives in the namespace's `mod.rs`.

**This ticket's scope:**

1. Create `generator/rust/namespaces.rs` with a `generate_namespaces(ir, filter, emit_deprecation, warnings)` entry point.
2. Emit per-namespace files: `src/<ns_path>/client.rs` (the client struct + methods) and `src/<ns_path>/mod.rs` (module declarations + re-exports). Types stay in `src/<ns_path>/types.rs` from RUSTGEN-2.
3. Emit the client struct: `pub struct <Ns>Client { plexus: PlexusClient, path: String }` where `path` is the dotted namespace path prefix for this namespace.
4. For each `MethodRole::Rpc` method, emit a typed method on the client: `pub async fn <name>(&self, <params>) -> Result<<return>>`.
5. For each `MethodRole::StaticChild` method, emit an accessor returning a typed child client: `pub fn <name>(&self) -> <ChildNs>Client`.
6. For each `MethodRole::DynamicChild { list_method, search_method }` method, emit a gate struct + impls for `DynamicChild`, optionally `Listable`, optionally `Searchable`. **This ticket emits the skeleton shape** with `Child = <placeholder child client type reference>`. RUSTGEN-6 tightens the placeholder into a real type import.
7. Sibling list/search methods referenced by a dynamic-child gate are HIDDEN from the flat method surface (matches TS default from IR-9).

**Plugin partitioning** (parity with TS's `resolve_type_dependencies`): if the user requests generation for only some namespaces, other namespaces still get their types generated (for cross-namespace type refs) but not their client modules. Port this logic.

**Filter support:** `--only <namespace>` / `--except <namespace>` flags in `GenerationOptions`. Port the TS filter logic.

## Required behavior

For each namespace `ns` in `ir` with at least one method, and passing the plugin-partition / filter checks, emit:

| File | Content |
|---|---|
| `src/<ns_path>/client.rs` | A `<Ns>Client` struct. Constructor `<Ns>Client::new(plexus: &PlexusClient, path: &str) -> Self`. One method per `MethodDef` with role-based shape (below). |
| `src/<ns_path>/mod.rs` | `pub mod client;`, `pub mod types;` (if types exist), child namespace declarations (`pub mod child1; pub mod child2;`), `pub use client::<Ns>Client;`, `pub use types::*;` re-exports. |

Per-method emission by role:

| Role | Emitted Rust |
|---|---|
| `Rpc` | `pub async fn <name>(&self, <params>) -> Result<<return>>` — frames call via `self.plexus.call_stream(&format!("{}.{}", self.path, "<name>"), params).await?`, then uses `rpc::unwrap_single_data` or `rpc::unwrap_all_data` or `rpc::unwrap_stream` based on `ReturnShape`. |
| `StaticChild` | `pub fn <name>(&self) -> <ChildNs>Client { <ChildNs>Client::new(&self.plexus, &format!("{}.{}", self.path, "<name>")) }`. |
| `DynamicChild { list_method, search_method }` | A `<Name>Gate` struct implementing `DynamicChild`. If `list_method.is_some()`, also implements `Listable`. If `search_method.is_some()`, also implements `Searchable`. The parent client exposes a field: `pub <name>: <Name>Gate` populated in the constructor. |

**Return-shape handling** for `Rpc` methods (parity with IR-2's `ReturnShape`):

| ReturnShape | Generated body |
|---|---|
| `Bare(T)` | `rpc::unwrap_single_data::<T>(stream).await` |
| `Option<T>` | `rpc::unwrap_single_data::<Option<T>>(stream).await` (schema-level nullable) |
| `Result<T, E>` | `rpc::unwrap_single_data::<T>(stream).await` — error propagates via `?`; `E` is the transport-level `anyhow::Error` in this iteration (domain-level `E` pass-through is a future ticket) |
| `Vec<T>` | `rpc::unwrap_all_data::<T>(stream).await` |
| `Stream<T>` | returns `Pin<Box<dyn Stream<Item = Result<T>> + Send>>` via `rpc::unwrap_stream::<T>(stream)` |

**Sibling hiding:** methods referenced by a dynamic-child gate's `list_method` / `search_method` are excluded from the flat-method emission loop (parity with TS IR-9).

**DynamicChild gate placeholder:** this ticket emits `impl DynamicChild for <Name>Gate { type Child = serde_json::Value; ... }` — the stub. RUSTGEN-6 replaces `serde_json::Value` with the real child client type. This ticket's generated gate struct and trait impl STRUCTURE is production-shape; only the `Child` type is stubbed.

**Imports:** per-namespace client.rs imports:

- `use crate::rpc;`
- `use crate::transport::PlexusClient;`
- `use crate::types::*;` (core transport types)
- `use super::types::*;` (this namespace's types)
- `use crate::<other_ns>::types::*;` for each cross-namespace type reference
- `use crate::<child_ns>::<ChildNs>Client;` for each static child's target client
- Trait imports: `use <runtime>::{DynamicChild, Listable, Searchable};` — location pinned by RUSTGEN-S01

Sorted, deduped, grouped.

## Risks

| Risk | Mitigation |
|---|---|
| Method-name collisions with Rust keywords (e.g., a method named `type` or `match`). | Emit `r#type` / `r#match` raw identifiers. Standard Rust escape. |
| Param name collisions with Rust keywords. | Same — raw identifiers. |
| DynamicChild placeholder `Child = serde_json::Value` requires consumer unwrapping. RUSTGEN-6 fixes this but this ticket must not introduce breakage beyond the existing stub. | Acceptance 4 pins: existing rust smoke tests (which rely on the stub) continue to pass after this ticket. |
| Cross-namespace static-child target resolution — a static child returning a type in another namespace needs correct `use` path. | Walk the IR's `ir_plugins` map to resolve the child's full namespace. Pin in acceptance 5. |
| Filter semantics diverge from TS. | Port the TS filter logic verbatim. Acceptance 6 verifies parity. |
| `generate_base_client()` (the `PlexusClient` in `transport.rs`) — this ticket does not touch it, but the namespace clients reference it. Ensure the re-export path in `lib.rs` is stable across this ticket and RUSTGEN-4. | Depends on RUSTGEN-4 landing `transport.rs` first — pinned via `blocked_by`. |
| Determinism. | Sort namespaces, methods within a namespace by name, imports alphabetically. |

## What must NOT change

- TypeScript backend — unchanged.
- Core transport types in `src/types.rs` — unchanged.
- `src/rpc.rs` from RUSTGEN-3 — unchanged.
- `src/transport.rs` from RUSTGEN-4 — unchanged.
- IR shape — no additions to `MethodDef`, `TypeDef`, etc.
- Existing Rust smoke tests pass — they consume the stub `Child = serde_json::Value`, which this ticket preserves. RUSTGEN-6 will update those tests.
- Generation CLI / API surface — no new required flags.
- Pre-IR schema output — pre-IR input (no `MethodRole` field; defaults to `Rpc`) produces output equivalent to pre-epic Rust output minus the file-layout moves already made by RUSTGEN-2/3/4.

## Acceptance criteria

1. `cargo build -p hub-codegen` and `cargo test -p hub-codegen` succeed.
2. A fixture IR with two namespaces (`alpha` and `beta`) each having two `Rpc` methods produces `src/alpha/client.rs` and `src/beta/client.rs`, each containing an `AlphaClient` / `BetaClient` struct with two async methods.
3. A fixture IR with a `StaticChild` method produces a parent-client accessor method returning the child client type (`pub fn <name>(&self) -> <ChildNs>Client`).
4. A fixture IR with a `DynamicChild { list_method: Some("xyz"), search_method: None }` method produces:
   - A `<Name>Gate` struct.
   - `impl DynamicChild for <Name>Gate` with `type Child = serde_json::Value` (placeholder — RUSTGEN-6 fixes).
   - `impl Listable for <Name>Gate` (because `list_method` is Some).
   - No `impl Searchable for <Name>Gate` (because `search_method` is None).
   - The parent client has a `pub <name>: <Name>Gate` field.
   - The sibling `xyz` method is NOT emitted on the parent client (hidden).
5. A fixture IR with a static child targeting a child in a different namespace produces `use crate::<child_ns>::<ChildNs>Client;` at the top of the parent's client.rs.
6. Filter support: running `generate_with_options(ir, opts)` with `opts.only = Some(vec!["alpha"])` produces namespace client files only for `alpha` (and namespace-prefixed descendants); `beta`'s client file is absent. Types for `beta` are still generated if `alpha` references `beta`'s types (parity with TS).
7. Two consecutive generator runs produce byte-identical per-namespace client.rs files (determinism gate).
8. The generated crate compiles (`cargo check` on the generated output succeeds) with the stub `Child = serde_json::Value`.
9. A method with return shape `Vec<T>` emits `rpc::unwrap_all_data::<T>(stream).await`; a method with shape `Stream<T>` emits `rpc::unwrap_stream::<T>(stream)`; a method with `Bare(T)` emits `rpc::unwrap_single_data::<T>(stream).await`.
10. Raw-identifier handling: a method named `type` emits `pub async fn r#type(&self, ...)`.

## Completion

PR against hub-codegen. CI green. PR description includes:

- File-count comparison of generated output before/after this ticket for the Solar fixture: previously N files in the Rust output, now M files (M > N, showing per-namespace client.rs files).
- A before/after diff of one namespace's generated client code — e.g., `solar/client.rs` — illustrating the new shape.
- Diff of `generator/rust/` showing `namespaces.rs` appearance and `client.rs` shrinkage.

Golden fixtures reblessed. Status flipped from `Ready` to `Complete` in the same commit.
