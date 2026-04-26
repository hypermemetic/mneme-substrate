---
id: CHILD-4
title: "list = / search = attribute args and ChildCapabilities bitflags"
status: Complete
type: implementation
blocked_by: [CHILD-3]
unlocks: [CHILD-7]
severity: High
target_repo: plexus-macros
---

## Problem

CHILD-3 makes child lookup ergonomic but leaves listing and searching invisible to clients. An activation's child set may be uncountable, policy-restricted, or remote, so listing and searching must be explicitly opt-in per activation. Without a way for authors to link a list or search implementation to their child-lookup method, clients using Synapse cannot discover what dynamic children exist behind a `get_child` gate, and introspection has to fall back to blind graph traversal with cycle detection. This ticket adds two attribute arguments — `list = "..."` and `search = "..."` — that let the author point the macro at sibling methods providing those streams, and generates the matching `ChildCapabilities` bitflags so clients can discover the capability at runtime.

## Context

Terminology: Plexus RPC is the protocol. `ChildRouter` gained `capabilities()`, `list_children()`, and `search_children(query)` methods in CHILD-2. `ChildCapabilities` is a bitflags type whose presently relevant flags are `LIST` and `SEARCH`. The default (no flags set) means the activation does not support listing or searching — clients must fall back to `get_child(name)`.

A static child method declared via `#[plexus_macros::child]` with no arguments (CHILD-3's `fn mercury(&self) -> Mercury` shape) has a compile-time name that is not a secret. Pinned decision for this ticket: **static children are always included in `list_children` regardless of whether the author wrote `list = "..."`**. The `list = "..."` argument only affects whether dynamic children are streamed. If an activation has only static `#[child]` methods, the macro generates `list_children` that yields the static names and sets `ChildCapabilities::LIST`.

Pinned decision on method location: **`list = "name"` and `search = "name"` require the referenced method to live in the same impl block as the `#[child]` method**. Cross-impl resolution is out of scope. This is a constraint that keeps macro implementation tractable since the proc macro can only see the tokens of the impl it is attached to.

Pinned decision on stream types: the referenced sibling method may return either `impl Stream<Item = String>` or `BoxStream<'_, String>`. The macro boxes the stream if needed so the generated `ChildRouter::list_children` / `search_children` return a uniform `BoxStream` wrapped in `Some`.

Accepted attribute arguments on `#[plexus_macros::child]`:

| Arg | Expected value | Effect on generated `ChildRouter` |
|---|---|---|
| `list = "method_name"` | Ident of a sibling method in the same impl | `capabilities()` includes `LIST`; `list_children()` returns `Some(stream)` that awaits/wraps the named method |
| `search = "method_name"` | Ident of a sibling method in the same impl | `capabilities()` includes `SEARCH`; `search_children(query)` returns `Some(stream)` that awaits/wraps the named method |

Expected sibling method signatures:

| For | Accepted signatures |
|---|---|
| `list = "..."` | `fn METHOD(&self) -> impl Stream<Item = String>` or `fn METHOD(&self) -> BoxStream<'_, String>`, sync or `async` |
| `search = "..."` | `fn METHOD(&self, query: &str) -> impl Stream<Item = String>` or `fn METHOD(&self, query: &str) -> BoxStream<'_, String>`, sync or `async` |

## Required behavior

| Attribute usage | `capabilities()` | `list_children()` | `search_children(q)` |
|---|---|---|---|
| `#[child]` only (no args), dynamic method | empty | `None` | `None` |
| `#[child]` only (no args), static methods only | `LIST` | `Some(stream of method-name strings)` | `None` |
| `#[child(list = "names")]` on dynamic method | `LIST` | `Some(stream from the named method)` (plus any static names when present) | `None` |
| `#[child(search = "find")]` on dynamic method | `SEARCH` | `None` (unless static children exist — see row 2) | `Some(stream from the named method)` |
| `#[child(list = "names", search = "find")]` | `LIST \| SEARCH` | `Some(stream)` | `Some(stream)` |

Macro-expansion errors the implementor must produce:

| Condition | Observable outcome |
|---|---|
| The method named in `list = "..."` does not exist in the same impl | Compile error containing `not found in impl` |
| The method named in `search = "..."` does not exist in the same impl | Compile error containing `not found in impl` |
| The referenced method's signature does not match one of the accepted shapes | Compile error referencing the signature mismatch |
| `list = "..."` or `search = "..."` is used on a method that is not itself a `#[child]` method | Compile error (attribute misuse) |

## Risks

| Risk | Mitigation pinned in this ticket |
|---|---|
| `impl Stream<Item = String>` return-position `impl Trait` may not be stable in all positions the macro wants to generate | Macro boxes the stream internally; generated code always returns `BoxStream<'_, String>`. |
| Sibling method lives in a different `impl` block and cannot be resolved by the proc macro | Unsupported. Author must place it in the same impl block. Compile error when the name is missing. |
| Author writes `list = "method"` pointing at a method whose return type is some other stream element (e.g., `Stream<Item = &str>`) | Compile error referencing signature mismatch. Only `String` element type is accepted in this ticket; widening can be a later decision. |
| Static-child auto-listing surprises an author who wanted to hide a static child name | Documented as a pinned decision in Context. If suppression is needed, it becomes a separate ticket. |

## What must NOT change

- CHILD-3 behavior for `#[child]` methods with no `list` or `search` args.
- Legacy `children = [...]` list attribute continues to work on impls that do not use `#[child]`.
- The `hub` flag on `#[activation]` continues to work.
- `ChildRouter::get_child(name)` remains the only mandatory operation.
- `ChildRouter` default implementations of `capabilities`, `list_children`, `search_children` (from CHILD-2) are unchanged; the macro merely overrides them when opt-in args are present.
- Substrate's 16 activations continue to compile without edits.

## Acceptance criteria

1. `cargo build -p plexus-macros` and `cargo test -p plexus-macros` both succeed on a clean checkout with this ticket applied.
2. A committed fixture activation whose dynamic `#[child]` method carries `list = "list_method"` and a valid sibling list method: at runtime, `capabilities()` equals `ChildCapabilities::LIST`, and awaiting `list_children()` returns `Some(stream)` whose collected items equal the names the sibling method emits.
3. A committed fixture activation whose dynamic `#[child]` method carries `search = "search_method"` and a valid sibling search method: at runtime, `capabilities()` equals `ChildCapabilities::SEARCH`, and awaiting `search_children("foo")` returns `Some(stream)` whose collected items equal what the sibling method emits for query `"foo"`.
4. A committed fixture activation using `list = "..."` and `search = "..."` together: `capabilities()` equals `ChildCapabilities::LIST | ChildCapabilities::SEARCH`.
5. A committed fixture activation with a bare `#[child]` dynamic method (no args): `capabilities()` is empty, `list_children()` returns `None`, `search_children("x")` returns `None`.
6. A committed trybuild (or equivalent) fixture with `#[child(list = "nonexistent_method")]`: compilation fails with an error message containing `not found in impl`.
7. A committed trybuild fixture where `list = "wrongly_typed"` points at a method whose return element is not `String`: compilation fails with a signature-mismatch error.
8. A committed fixture activation with only static `#[child]` methods and no `list = "..."` arg: `capabilities()` includes `ChildCapabilities::LIST`, and awaiting `list_children()` returns `Some(stream)` whose collected items equal the set of static method names.
9. A committed fixture activation exercising both a sibling method returning `impl Stream<Item = String>` and another returning `BoxStream<'_, String>`: both compile and behave identically at runtime.
10. Running `cargo build --workspace` in `plexus-substrate` with this plexus-macros revision pinned succeeds without edits to any of the existing 16 activations.

## Completion

PR against `plexus-macros` with runtime fixtures, trybuild cases, and documentation of the pinned decisions (same-impl scoping, static-children-always-listed, macro-boxes-streams). CI green. Status flipped from `Ready` to `Complete` in the same commit that lands the code.
