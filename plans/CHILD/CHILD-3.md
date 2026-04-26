---
id: CHILD-3
title: "Basic #[plexus_macros::child] method attribute"
status: Complete
type: implementation
blocked_by: [CHILD-2]
unlocks: [CHILD-4, CHILD-7]
severity: High
target_repo: plexus-macros
---

## Problem

Declaring children on an activation currently requires one of two ergonomic dead ends. Either the author writes `#[activation(children = [field_a, field_b])]` — which is static, field-based, and cannot express dynamic children whose set is computed at call time — or the author hand-writes an entire `ChildRouter` impl (the Solar pattern, ~25 lines of boilerplate per activation). A single method-level attribute lets authors declare children inline on the impl block, express static and dynamic children with the same syntax, and unlocks further opt-in routing features (listing, searching) in CHILD-4.

## Context

Terminology: "Plexus RPC" is the protocol. `DynamicHub` is the in-process router. `ChildRouter` is the trait an activation implements to route `get_child(name)` calls. Plexus RPC networks are graphs, not trees, so `get_child(name)` remains the only mandatory operation — listing and searching are layered in CHILD-4.

Two method shapes are accepted by the attribute:

| Shape | Meaning | Example |
|---|---|---|
| `fn NAME(&self) -> Child` (no args) | Static child; the method name is the child's routing name | `fn mercury(&self) -> Mercury` |
| `fn NAME(&self, name: &str) -> Option<Child>` (sync or async) | Dynamic fallback dispatcher; receives the unmatched name | `fn planet(&self, name: &str) -> Option<Planet>` |

Wire semantics: `#[child]` methods are NOT exposed as Plexus RPC methods. They only contribute to `ChildRouter::get_child` routing. If a method ever needs to be both an RPC method and a child lookup, that is a separate future decision.

## Required behavior

When the `#[plexus_macros::activation]` macro sees one or more methods annotated with `#[plexus_macros::child]` in its impl block, it synthesises a `ChildRouter` impl whose `get_child(name)` dispatches as follows:

| Input | Generated dispatch |
|---|---|
| A name matching a static `#[child]` method's identifier | Returns that method's output wrapped for `ChildRouter` |
| A name not matching any static method, when a dynamic `#[child]` method is present | Falls through to the dynamic method (the `_ =>` arm) |
| A name not matching any static method, when no dynamic `#[child]` method is present | Returns `None` |

Rules enforced at macro expansion:

| Rule | Observable outcome |
|---|---|
| At most one dynamic child method per activation | Compile error naming the conflict |
| A `#[child]` static method's return type must satisfy `ChildRouter + Clone + Send + Sync + 'static` | Standard trait-bound error from the generated code |
| `#[child]` and `#[method]` are mutually exclusive on the same function | Compile error containing "mutually exclusive" (or similar clearly worded phrase) |
| Both sync and async `#[child]` method bodies are accepted | Macro handles awaiting in the generated dispatcher |
| `#[child]` and legacy `children = [...]` on the same impl | Compile error — forces migration clarity |
| Unsupported signature on a `#[child]` method | Compile error containing "child method signature" |

Legacy `children = [...]` continues to work on impls that do NOT contain any `#[child]` method. Deprecation of the legacy syntax is out of scope for this epic and is tracked as a follow-up.

## Risks

| Risk | Mitigation pinned in this ticket |
|---|---|
| Full Rust signature polymorphism (lifetimes, generics, `impl Trait` return types) is large | Support only the two shapes in the table above. More complex signatures produce the "child method signature" compile error and can be added in a later ticket. |
| An author puts both `children = [...]` and `#[child]` on the same impl and expects a merge | Macro errors out. No silent union. |
| A `#[child]` dynamic method using a non-`&str` name parameter (e.g., `name: u32`) | Treated as an unsupported signature; compile error. |

## What must NOT change

- `ChildRouter::get_child(name)` trait signature.
- The `hub` flag on `#[activation]` continues to work on impls without `#[child]` methods.
- Legacy `children = [...]` continues to work on impls without `#[child]` methods.
- All 16 substrate activations compile unchanged after this ticket lands.
- `#[method]` semantics, codegen, and wire format are untouched.

## Acceptance criteria

1. `cargo build -p plexus-macros` and `cargo test -p plexus-macros` both succeed on a clean checkout with this ticket applied.
2. A committed fixture activation with only static `#[child]` methods expands such that calling `get_child("mercury")` returns `Some(...)` for a known static child name and `get_child("not_a_child")` returns `None`.
3. A committed fixture activation with one static `#[child]` method and one dynamic `#[child]` method expands such that `get_child("<static_name>")` resolves via the static arm and `get_child("<unknown_name>")` resolves via the dynamic method (observable by the dynamic method returning a distinguishable child).
4. A committed trybuild (or equivalent) fixture with a `#[child]` method whose signature is `fn planet(&self, name: u32)` fails to compile and the compiler output contains the phrase `child method signature`.
5. A committed trybuild fixture with both `#[child]` and `#[method]` on the same function fails to compile and the compiler output contains `mutually exclusive` (or a clearly equivalent phrase pinned in the fixture's expected-error file).
6. A committed trybuild fixture with two dynamic `#[child]` methods on the same impl fails to compile and the error names the conflict.
7. A committed trybuild fixture with both `#[child]` and `children = [...]` on the same impl fails to compile.
8. Running `cargo build --workspace` in `plexus-substrate` with this plexus-macros revision pinned succeeds without edits to any of the existing 16 activations.

## Completion

PR against `plexus-macros` with fixtures and trybuild cases committed. CI green. Status flipped from `Ready` to `Complete` in the same commit that lands the code.
