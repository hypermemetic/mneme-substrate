---
id: RUSTGEN-6
title: "hub-codegen Rust: DynamicChild<T> real-Child wiring (absorbs IR-13)"
status: Pending
type: implementation
blocked_by: [RUSTGEN-5]
unlocks: [RUSTGEN-8, RUSTGEN-9]
severity: High
target_repo: hub-codegen
---

## Problem

After RUSTGEN-5, every `DynamicChild` gate in generated Rust output has `type Child = serde_json::Value` — a placeholder. A Rust consumer calling `client.<hub>.<gate>.get("mercury").await?` receives untyped `serde_json::Value` back, losing the whole point of IR-9's typed-handle pattern. The TypeScript backend already emits typed `Child` (since IR-9 shipped); this ticket brings Rust to parity.

IR-13 was the original ticket attempting this fix but got stopped mid-edit at ~400 lines because the infrastructure it depended on (per-namespace clients, transport, rpc helpers) didn't exist. RUSTGEN-2 through RUSTGEN-5 build that infrastructure; this ticket does the wiring.

## Context

**IR-13 scope (absorbed here):** replace `serde_json::Value` with the real child client struct type. The `.get(name)` method constructs and returns a typed client.

**RUSTGEN-5 output:** per-gate structs and `DynamicChild` trait impls exist, but `Child = serde_json::Value` is hardcoded. This ticket updates the codegen to:

1. Walk the dynamic-child method's return type (`TypeRef::RefNamed(qn)`), resolving `qn` to a plugin namespace and a client type (`<ChildNs>Client`).
2. Emit `impl DynamicChild for <Name>Gate { type Child = <ChildNs>Client; ... }`.
3. Update `.get(name)` body: instead of returning the raw stream value as JSON, construct `<ChildNs>Client::new(&self.plexus, &format!("{}.{}", self.path, name))` and return it.

**Return-shape handling on `.get(name)`** (parity with IR-9):

| IR return shape | `.get(name)` return type |
|---|---|
| `Option<T>` | `Result<Option<<ChildNs>Client>>` — if the dynamic-child lookup fails (runtime), returns `Ok(None)`. |
| `Result<T, E>` | `Result<<ChildNs>Client>` — errors propagate. |
| `Result<Option<T>, E>` | `Result<Option<<ChildNs>Client>>`. |
| `T` bare | `Result<<ChildNs>Client>` — no null. |

**Runtime lookup semantics:** the dynamic child "exists" check requires a round-trip. Options:

| Option | Behavior |
|---|---|
| **A. Lazy:** `.get(name)` just constructs the child client without verifying existence. `Option<T>` still returns `Some(client)` always. Errors surface on the first RPC method call. | Matches TS behavior. Pinned. |
| **B. Eager:** `.get(name)` calls `<list_method>` or a dedicated `exists(name)` method to verify first. | Out of scope — optimization. |

**Pinned: option A.** The TS backend does A; Rust matches.

**Capability intersection (IR-9 pattern):** `.list()` returns `impl Stream<Item = String>`. `.search(query)` returns `impl Stream<Item = String>`. A consumer calling `.search()` on a gate that didn't opt in to `Searchable` gets a Rust compile error (trait not implemented).

## Required behavior

For each `MethodRole::DynamicChild { list_method, search_method }` method in the IR:

1. Resolve the child client type. Walk `md_returns` unwrapping `Option` / `Result` / `Result<Option>` wrappers to find `TypeRef::RefNamed(qn)`. Resolve `qn` to a plugin namespace in `ir.ir_plugins`. The client type is `<capitalized_last_segment>Client` (matches RUSTGEN-5's naming — e.g., `hypermemetic.celestial_body` → `CelestialBodyClient`).
2. Emit the gate struct: `pub struct <Name>Gate { plexus: PlexusClient, path: String }` (unchanged from RUSTGEN-5 structure).
3. Emit `impl DynamicChild for <Name>Gate`:
   - `type Child = <ChildNs>Client;`
   - `async fn get(&self, name: &str) -> Result<Option<<ChildNs>Client>>` (shape per table above) — constructs `<ChildNs>Client::new(&self.plexus, &format!("{}.{}", self.path, name))` and returns it wrapped per return-shape.
4. If `list_method.is_some()`: emit `impl Listable for <Name>Gate` with `fn list(&self) -> impl Stream<Item = Result<String>>` — calls `self.plexus.call_stream(&format!("{}.{}", <parent_path>, "<list_method>"), json!({}))` and unwraps `String` items.
5. If `search_method.is_some()`: emit `impl Searchable for <Name>Gate` with `fn search(&self, query: &str) -> impl Stream<Item = Result<String>>` — calls `<search_method>` with `json!({ "query": query })`.
6. Emit `use crate::<child_ns>::<ChildNs>Client;` at the top of the parent's `client.rs`.
7. Update existing `tests/rust_codegen_smoke_test.rs` to assert the typed `Child` path (the stub-path tests are updated OR removed, per RUSTGEN-8's planning — this ticket changes codegen; test updates can land together).

**serde_json::Value must NOT appear as the `Child` type** anywhere in generated output after this ticket lands. A `grep 'type Child = serde_json::Value' <generated_output>/` returns zero matches.

**Consumer compile check:** A consumer-side fixture `.rs` file (committed into the repo as a test fixture) uses the generated gate's `.get(name)` and calls a typed method on the result:

```rust
let solar = PlexusClient::new("ws://localhost:8080");
let mercury: CelestialBodyClient = solar.solar.body.get("mercury").await?.unwrap();
let info: CelestialBodyInfo = mercury.info().await?;
```

`cargo check` on the consumer fixture must succeed — this is the definitive typed-path confirmation.

## Risks

| Risk | Mitigation |
|---|---|
| Child type resolution fails when the child's schema is missing from the IR batch. | Emit a clear codegen-time error naming the missing schema — same pattern as IR-9 acceptance 6. Acceptance 5. |
| Return-shape unwrap ambiguity: `Option<Result<T, E>>` vs `Result<Option<T>, E>` have the same surface but inverted semantics. | Follow IR-2's `ReturnShape` exactly. If the IR says `Result<Option<T>, E>`, that's the emission target. The IR is the source of truth. |
| Capability trait orphan rules — if runtime library is a sibling crate (RUSTGEN-S01 option B), impls must live in the generated crate, not the runtime crate, so they compile. | Emit `impl Listable for <Name>Gate` in the generated crate. The gate struct is local to the generated crate, so the orphan rule allows impls there regardless of where `Listable` is defined. Pinned by S01's spike. |
| Existing Rust smoke tests break (they assert `Child = serde_json::Value`). | This ticket updates them in the same PR. Optionally, defer to RUSTGEN-8 if the update surface is non-trivial — but the minimum is: they don't fail the build. Acceptance 6. |
| Some dynamic children return types that are themselves hub activations (children of children). The child client must support its own dynamic-child gates. | Recursive — the same codegen path emits child clients with their own gates. No special handling needed; pin in acceptance 7. |

## What must NOT change

- TypeScript backend — byte-identical from IR-9.
- IR shape — no changes to `ir.rs` or `ir_types`, `ir_methods`.
- Runtime library trait definitions (`DynamicChild`, `Listable`, `Searchable`) — shape fixed by RUSTGEN-S01. This ticket emits impls, not trait redefinitions.
- Static-child and Rpc-method emission from RUSTGEN-5 — unchanged.
- `transport.rs` and `rpc.rs` — unchanged.
- The `<Name>Gate` struct name, field names (`plexus`, `path`), or constructor signature — stable from RUSTGEN-5.
- Generation CLI surface — no new required flags.

## Acceptance criteria

1. `cargo build -p hub-codegen` and `cargo test --features rust -p hub-codegen` (or equivalent test invocation) succeed.
2. A fixture IR with a dynamic-child gate referencing a child activation produces generated Rust where the gate's `impl DynamicChild` has `type Child = <ChildClient>;` (not `serde_json::Value`).
3. The gate's `.get(name)` method returns the typed child client (`Result<Option<ChildClient>>` for `Option` return shape, etc., per return-shape table).
4. A consumer fixture .rs file (committed at `hub-codegen/tests/fixtures/rustgen_6_consumer/src/main.rs` or equivalent) uses `client.<hub>.<gate>.get(name).await?.unwrap().<typed_method>().await?` — `cargo check` on this consumer succeeds.
5. A fixture IR whose dynamic-child gate references a child type NOT present in the IR batch produces a clear codegen-time error naming the missing schema — not a partial output.
6. A consumer fixture that attempts `.search(q)` on a gate without `search_method` (no `Searchable` impl) fails `cargo check` with a clear error mentioning `Searchable` trait not implemented. This is verified via a deliberate compile-error fixture.
7. A fixture IR with a dynamic-child gate whose child is ITSELF a hub activation (with its own dynamic children) produces nested typed gates — `cargo check` passes, and a consumer fixture calls `client.a.b_gate.get("x").await?.unwrap().c_gate.get("y").await?.unwrap().<method>().await?` successfully (compile-check).
8. `grep -rn 'type Child = serde_json::Value' <generated_output>/` on a post-IR fixture returns zero matches.
9. Existing Rust smoke tests updated and passing (they assert the typed `Child`, not the stub).
10. Two consecutive generator runs produce byte-identical output (determinism).

## Completion

PR against hub-codegen. CI green. PR description includes:

- Before/after sample of the generated gate impl showing the typed `Child` association.
- The consumer fixture `.rs` used for the `cargo check` validation.
- Updated smoke test output showing the typed path.

Status flipped from `Ready` to `Complete` in the same commit. Also flip IR-13 to `Superseded` with `superseded_by: RUSTGEN-6` (already done upstream; verify).
