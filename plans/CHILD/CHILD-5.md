---
id: CHILD-5
title: "Doc-comment extraction for activation/method/child descriptions"
status: Complete
type: implementation
blocked_by: []
unlocks: [CHILD-7]
severity: Medium
target_repo: plexus-macros
---

## Problem

Today, activation and method descriptions must be written twice: once as a `///` doc comment (picked up by `cargo doc` and IDE tooling) and again as `description = "..."` on the macro attribute (surfaced by synapse and the Plexus RPC schema). The two strings drift. The macro should default the description to the item's doc comment so a single source of truth serves both audiences, while still allowing an explicit override when the RPC help text needs to differ from the rustdoc text.

## Context

The three affected attributes are `#[plexus_macros::activation]`, `#[plexus_macros::method]`, and (landing in CHILD-3) `#[plexus_macros::child]`. In Rust's AST, both `///` sugar and the raw `#[doc = "..."]` form appear as `#[doc = "..."]` attributes on the item. The standard common-leading-whitespace-strip rule (as used by `cargo doc`) applies to multi-line doc comments.

Expected mapping:

| Input | Generated description |
|---|---|
| `/// Foo.` + no `description =` | `"Foo."` |
| `/// Foo.` + `/// Bar.` + no `description =` | `"Foo.\nBar."` |
| `/// Foo.` + `description = "Baz"` | `"Baz"` |
| No doc comment, no `description =` | `""` |

## Required behavior

At macro expansion time, for each of `#[plexus_macros::activation]`, `#[plexus_macros::method]`, and `#[plexus_macros::child]`:

| Condition | Resolved description |
|---|---|
| `description = "..."` explicitly provided on the attribute | The explicit string, exactly as written |
| No `description =` and one or more `#[doc = "..."]` attributes on the item | Concatenation of all doc-comment lines joined with `\n`, with common leading whitespace stripped from each line |
| No `description =` and no `#[doc = "..."]` attributes | Empty string |

If CHILD-3 (the `#[plexus_macros::child]` attribute) has not yet landed when this ticket is implemented, scope is limited to `#[plexus_macros::activation]` and `#[plexus_macros::method]` and the `#[child]` extension is tracked as a follow-up noted in the Completion section.

## Risks

| Risk | Mitigation |
|---|---|
| Raw `#[doc = "..."]` vs `///` sugar visible differently in the macro's AST | Verify both forms round-trip to the same `#[doc = "..."]` attribute shape the macro reads; if not, handle both explicitly. |
| Reinventing leading-whitespace stripping | Follow the standard `cargo doc` rule (strip common leading whitespace across all lines). Prefer an existing helper crate over a bespoke implementation. |
| A consumer relies on descriptions being empty by default and has `///` doc comments that they do not want surfaced to RPC | Documented in the migration note of the PR; explicit `description = ""` continues to force empty. |

## What must NOT change

- Explicit `description = "..."` values continue to produce exactly that string.
- All 16 substrate activations compile without edits after this ticket lands.
- Wire format and generated code structure apart from the description string default.
- `#[method]` and `#[activation]` behavior in every other respect (ordering, error messages, other attribute arguments).

## Acceptance criteria

1. `cargo build -p plexus-macros` and `cargo test -p plexus-macros` both succeed on a clean checkout with this ticket applied.
2. A committed fixture activation with a single-line `///` doc comment (`/// Foo`) and no explicit `description =` produces a schema whose description field equals `"Foo"` (observable via the generated `.description()` accessor or the schema-export mechanism equivalent).
3. A committed fixture with multiple `///` doc-comment lines preserves newlines between them in the resolved description (e.g., `/// Foo` followed by `/// Bar` yields `"Foo\nBar"`).
4. A committed fixture with both a `///` doc comment and an explicit `description = "Bar"` resolves to `"Bar"` (explicit wins).
5. A committed fixture with neither a doc comment nor an explicit `description =` resolves to `""` (empty string).
6. The above four behaviors are verified on `#[plexus_macros::activation]`, `#[plexus_macros::method]`, and `#[plexus_macros::child]` (the last only if CHILD-3 has landed; otherwise the follow-up is noted in Completion).
7. `cargo build --workspace` in `plexus-substrate` with this plexus-macros revision pinned succeeds with zero edits to any of the existing 16 activations.

## Completion

PR against `plexus-macros` with fixtures committed. CI green. Status flipped from `Ready` to `Complete` in the same commit that lands the code. If CHILD-3 has not yet landed when this ticket is implemented, a follow-up issue is opened against `plexus-macros` to extend doc-comment extraction to `#[plexus_macros::child]` and linked from the Completion note.
