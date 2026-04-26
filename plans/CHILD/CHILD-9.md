---
id: CHILD-9
title: "Flexible return types for #[plexus_macros::child]"
status: Pending
type: implementation
blocked_by: []
unlocks: []
severity: Medium
target_repo: plexus-macros
---

## Problem

CHILD-3 pinned two accepted signatures for `#[plexus_macros::child]` methods: static (`fn NAME(&self) -> Child`) and dynamic (`fn NAME(&self, name: &str) -> Option<Child>`). Anything else produces a "child method signature" compile error. This is too restrictive. Authors should be able to write whatever signature fits their domain — return a `Vec<Child>` for enumerable children, a `Result<Child, E>` for fallible pure lookups, a bare `Child` for infallible static, an `Option<Child>` or `Result<Option<Child>, E>` for fallible-or-absent — and the macro introspects the return type and generates appropriate dispatch.

## Context

Currently the macro's dispatch path is hardcoded to the two CHILD-3 shapes. It awaits async, unwraps `Option<Child>`, and matches names in a `match` arm. Any other return shape produces an unhelpful error at expansion time.

This is out of step with the rest of Rust — method signatures are load-bearing and authors expect the macro to accommodate the type they wrote, not dictate what they can return. The current restriction was a CHILD-3 scope-limit, not a design decision.

## Required behavior

Accept any of these return-type shapes for `#[plexus_macros::child]` methods:

| Shape | Dispatch semantics |
|---|---|
| `T` (bare) | `get_child` returns `Some(Box::new(self.name()))`; always resolves |
| `Option<T>` | `get_child` returns the `Option`, boxed when `Some` |
| `Result<T, E>` | `get_child` returns `Some` on `Ok`, `None` on `Err` (error logged at debug level) |
| `Result<Option<T>, E>` | `get_child` returns `Ok(Some)` → `Some`, `Ok(None)` or `Err` → `None` |
| `Vec<T>` | The method returns multiple children at once; dispatch enumerates them and matches by name. Also implicitly satisfies `list_children` — capabilities includes `LIST` without an explicit `list = "..."` arg. |
| `impl Stream<Item = T>` or `BoxStream<'_, T>` | Async enumerable; same implicit-listing treatment as `Vec<T>`. |

Sync vs async is orthogonal — all shapes are accepted in either form.

Argument-shape taxonomy remains:
- Zero args (`&self` only) — static (or enumerable when return is `Vec`/`Stream`)
- `&self, name: &str` — keyed lookup
- `&self, query: Q` — filter/search (Q implements some discoverable trait like `AsRef<str>`; pin in spike if needed)

Where `T: ChildRouter + Clone + Send + Sync + 'static`.

| Rule | On violation |
|---|---|
| Return type doesn't wrap `T: ChildRouter` in one of the accepted shapes | Compile error with the list of accepted shapes |
| Zero-arg method returning `Vec<T>` OR a stream | Macro implicitly sets `ChildCapabilities::LIST` and generates `list_children` from the method's output (redundant with explicit `list = "..."` — prefer one or the other; if both present, error) |
| Mixing `#[child]` and `#[method]` on same function | Error (unchanged from CHILD-3) |

## Risks

| Risk | Mitigation |
|---|---|
| Return-type introspection is fragile in `syn` (generics, aliases, `impl Trait`) | Pattern-match on the surface of `Return(Type)`; support concrete aliases (`Option`, `Result`, `Vec`) by name; error cleanly on unrecognized shapes and list the accepted ones. |
| `Result<T, E>` with non-`Debug` E makes error logging awkward | Generated code uses `format!("{e:?}")` only behind `cfg(debug_assertions)`; in release, errors are dropped silently with a `tracing::debug!` call. |
| `Vec<T>` vs `Stream<T>` distinction at compile time | Treat `Vec<T>` → collect in memory; `Stream<T>` → preserve streaming. Generated `list_children` accepts either. |
| Users combining `list = "..."` with a zero-arg `Vec`-returning `#[child]` | Compile error naming the conflict — pick one source of listing. |

## What must NOT change

- The two CHILD-3 shapes continue to work — existing fixtures pass.
- CHILD-4's `list = "..."` / `search = "..."` attr args continue to work when the `#[child]` method itself returns `Option<T>` (the original dynamic shape).
- All 16 substrate activations compile unchanged.

## Acceptance criteria

1. `cargo build -p plexus-macros` and `cargo test -p plexus-macros` succeed.
2. New fixtures exercise each accepted shape: `T`, `Option<T>`, `Result<T, E>`, `Result<Option<T>, E>`, `Vec<T>`, `impl Stream<Item = T>`. Each compiles and `get_child` behaves per the dispatch semantics table.
3. A fixture with `Vec<T>`-returning zero-arg `#[child]` has `capabilities() contains LIST` and `list_children()` returns `Some(stream)` yielding each child's name.
4. A fixture combining `Vec<T>`-returning `#[child]` with `list = "..."` fails to compile with a clear error.
5. A fixture with a return type that doesn't wrap `ChildRouter` (e.g., `fn(&self) -> String`) fails to compile with an error listing the accepted shapes.
6. All CHILD-3 fixtures (static + dynamic lookup) continue to pass without modification.
7. CHILD-7's migrated Solar continues to work with zero edits.
8. Substrate `cargo build --workspace` passes.

## Completion

PR against plexus-macros. CI green. Status flipped to Complete in the same commit.
