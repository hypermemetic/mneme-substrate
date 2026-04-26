---
id: IR-9
title: "synapse-cc: typed-handle codegen for dynamic children (DynamicChild<T> + capability intersections)"
status: Complete
type: implementation
blocked_by: [IR-3]
unlocks: []
severity: High
target_repo: hub-codegen
---

## Problem

synapse-cc generates client code from Plexus RPC schemas. For methods tagged `MethodRole::Rpc`, it emits a plain function. For `MethodRole::StaticChild` — names known at codegen time — a property accessor typed as the child's client works naturally. But for `MethodRole::DynamicChild` — methods like Solar's `body(name: &str) -> Option<CelestialBodyActivation>` — the generated code must express two things simultaneously: the child's **type** is known at codegen time (the `ChildClient` class is generated from the child's schema), but the specific child **instance's name** is runtime data.

A flat method (`body(name): Promise<ChildClient | null>`) technically works but loses per-capability typing. A caller can try to call `.list()` on an activation that didn't opt in via `list_method`, and the mistake only shows up at runtime. Worse, sibling methods like `body_names` and `search_bodies` spill into the parent's namespace and imply the caller should know which ones to call together.

The clean representation is a first-class **typed handle** — `DynamicChild<T>` — with capability interfaces (`Listable`, `Searchable`) mixed in via the target language's intersection / trait-impl mechanism, opt-in per the IR. A caller who invokes a capability that wasn't opted in gets a compile-time error in the target language.

## Context

Target repo: `~/dev/controlflow/hypermemetic/synapse-cc/`.

**Upstream:** IR-3 emits `MethodRole::DynamicChild { list_method, search_method }` on the `MethodSchema` for every `#[plexus_macros::child]` method with a name parameter. The child's return type is resolved to another schema in the same batch (the parent activation's IR references child schemas by type).

**Runtime library components** (hand-written once per target language, shipped alongside synapse-cc-generated code):

Shape for TypeScript:
```typescript
interface DynamicChild<T> {
  get(name: string): Promise<T | null>;
}
interface Listable {
  list(): AsyncIterable<string>;
}
interface Searchable {
  search(query: string): AsyncIterable<string>;
}
function makeDynamicChild<T>(
  rpc: RpcClient,
  parentPath: string,
  methodName: string,
  config: { listMethod: string | null; searchMethod: string | null; childClient: new (rpc: RpcClient, path: string) => T; }
): DynamicChild<T> & Partial<Listable & Searchable>;
```

Equivalent Rust shape:
```rust
pub trait DynamicChild {
    type Child;
    async fn get(&self, name: &str) -> Option<Self::Child>;
}
pub trait Listable {
    async fn list(&self) -> impl Stream<Item = String>;
}
pub trait Searchable {
    async fn search(&self, query: &str) -> impl Stream<Item = String>;
}
```

These interfaces and helpers are the **hand-written runtime library per target language** — shipped as a small companion package or vendored into synapse-cc's generated output. synapse-cc generates **client code that references them**, not the interfaces themselves.

## Required behavior

For each `MethodSchema` in the input IR whose `role` is `DynamicChild { list_method, search_method }`:

| Input | Generated output (TypeScript form; other languages equivalent) |
|---|---|
| `DynamicChild { list_method: None, search_method: None }` | A property on the parent client typed `DynamicChild<ChildClient>`. No capability interfaces mixed in. |
| `DynamicChild { list_method: Some("xyz"), search_method: None }` | Property typed `DynamicChild<ChildClient> & Listable`. The `.list()` method calls `<namespace>.<xyz>` at runtime. |
| `DynamicChild { list_method: None, search_method: Some("abc") }` | Property typed `DynamicChild<ChildClient> & Searchable`. `.search(q)` calls `<namespace>.<abc>`. |
| `DynamicChild { list_method: Some, search_method: Some }` | Property typed `DynamicChild<ChildClient> & Listable & Searchable`. |

**Return-shape handling on `.get(name)`:**

The `#[child]` method's `ReturnShape` (from IR-2) determines the generated `.get(name)` return type:

| ReturnShape in IR | `.get(name)` generated return |
|---|---|
| `Option<T>` | `Promise<ChildClient \| null>` / `Option<ChildClient>` |
| `Result<T, E>` | `Promise<ChildClient>` throws typed `E` / `Result<ChildClient, E>` |
| `Result<Option<T>, E>` | `Promise<ChildClient \| null>` throws on `E`, returns `null` on `Ok(None)` |
| `T` bare | `Promise<ChildClient>` — no null, no throw |
| `Vec<T>` or `Stream<T>` | **Not a dynamic-child shape** — this is handled by CHILD-9 as an enumerable/listable method. Covered in IR-3's role determination, not here. |

**ChildClient generation:** ChildClient is the typed client synapse-cc emits for the child's own schema. This ticket reuses the existing per-activation codegen path — the child is just another activation, its client generated the same way. No new codegen for the child itself; only the link from `DynamicChild<T>` to the existing child class.

**Sibling methods for list/search:** The methods referenced by `list_method` and `search_method` are ALSO present in the parent's `methods: Vec<MethodSchema>` (as `Rpc`-role methods — every author-written method appears there). synapse-cc's decision for this ticket:

| Option | Behavior |
|---|---|
| **Default: hide the sibling on the parent client.** The list/search methods are accessible only through the `DynamicChild<T>` gate's `.list()` / `.search()` — not as flat methods on the parent. Keeps the parent's API minimal. | Pinned default. |
| Opt-in: `--expose-raw-list-search` flag | Also emits the raw sibling methods on the parent client for users who want the lower-level access. Not part of this ticket — scoped as a follow-up. |

## Integration with IR-7 (deprecation annotations)

If the `#[child]` method itself carries `deprecation`, the generated `DynamicChild<T>` property is annotated per IR-7's rules (native language marker plus the deprecation comment). If `list_method` or `search_method` reference a sibling that's deprecated, the capability method on the gate gets the annotation — the gate's `.list()` or `.search()` is flagged, not the whole gate.

## Risks

| Risk | Mitigation |
|---|---|
| Not all target languages have clean intersection types (TypeScript does; Rust uses traits; Python relies on ABCs + multiple inheritance) | Per-target-language codegen backend handles the idiom. The runtime library shape differs across languages, but the concept (`DynamicChild<T>` + opt-in capabilities) is uniform. Acceptance 3 covers two backends. |
| The runtime library needs to be versioned alongside synapse-cc output | Document the version pinning in synapse-cc's README and in the generated output's preamble. Out of scope: a formal versioning scheme — that's a follow-up. |
| Child schema resolution: synapse-cc must know `CelestialBodyActivation`'s schema to emit `CelestialBodyClient` | Synapse-cc already handles this: the child's schema is a separate activation in the input batch. If a child's schema is missing from the input, synapse-cc errors out with a clear message (acceptance 6). |
| Flat-method fallback for consumers who prefer it | Not in scope for this ticket. The typed-handle form is the generated output. If a flat-method form is later needed, that's a separate ticket. |

## What must NOT change

- Static children continue to generate typed property accessors (unchanged — this ticket doesn't touch `MethodRole::StaticChild` handling).
- `Rpc`-role methods continue to generate flat client methods (unchanged).
- synapse-cc's existing CLI surface is unchanged — no new required flags (the list/search sibling exposure is an opt-in future flag, not added here).
- Generated client code compiles and runs in every supported target language with no new runtime dependencies beyond the per-language runtime library.
- Regenerating a pre-IR substrate schema (no `MethodRole` field) produces output indistinguishable from pre-ticket output. Typed-handle generation is only triggered when `MethodRole::DynamicChild` is present.

## Acceptance criteria

1. `cargo build` and `cargo test` in synapse-cc succeed.
2. A fixture post-IR schema with one `DynamicChild { list_method: Some("xyz"), search_method: None }` method and a referenced child schema produces generated client code where:
   - The parent client has a property typed `DynamicChild<ChildClient> & Listable` (or language-equivalent intersection).
   - `.get(name)` returns `Promise<ChildClient | null>` (for `Option<T>` return shape).
   - `.list()` returns `AsyncIterable<string>` and at runtime calls the `xyz` method.
   - The `xyz` sibling method is NOT emitted as a flat method on the parent client (default hiding).
3. Acceptance 2 verified on at least two target-language backends (TypeScript + Rust minimum; Python if supported).
4. A fixture with `DynamicChild { list_method: None, search_method: None }` produces `DynamicChild<ChildClient>` alone — neither `Listable` nor `Searchable` mixed in.
5. A fixture calling `.search(q)` on a `DynamicChild<T>` that lacks `Searchable` fails to compile in the target language. The test asserts this via a deliberate type-error fixture that is expected to fail type-checking.
6. A fixture whose input schema references a dynamic child's type without providing that child's schema in the batch produces a clear codegen-time error naming the missing schema — not a partial/incorrect output.
7. Regenerating a pre-IR schema produces output byte-identical to pre-ticket output (regression gate with IR-7).
8. The integration test runs two consecutive regens against the same post-IR schema and produces byte-identical generated files (determinism pin).
9. Generated client code, when compiled or run, produces no new errors — the typed-handle form behaves identically at runtime to what a hand-written equivalent would do.

## Completion

PR against synapse-cc (plus per-language runtime library updates if they're in the same repo or a companion repo). PR description includes a generated-output diff showing the `DynamicChild<T>` emission on at least two target-language backends. CI green. Ticket status flipped from `Ready` → `Complete` in the same commit.
