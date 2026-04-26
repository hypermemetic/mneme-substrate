---
id: STG-S01
title: "Spike: trait shape for ONE activation (Mustache)"
status: Pending
type: spike
blocked_by: []
unlocks: [STG-S02, STG-2]
severity: High
target_repo: plexus-substrate
---

## Question

Can Mustache's current `MustacheStorage` concrete type be replaced with a `MustacheStore` trait plus a concrete SQLite backend implementing it, such that Mustache's existing test suite passes unchanged (same assertions, same fixtures) against the trait-based rewrite?

Mustache is the smallest storage-bearing activation (`src/activations/mustache/storage.rs` is 335 lines, with 5 public methods and 4 test cases). If the trait shape works for Mustache, the pattern is viable for the larger activations. If it fails on Mustache, we have a fundamental issue that would block the epic.

## Setup

1. Work in a throwaway branch on `plexus-substrate`.
2. Define a `MustacheStore` trait (public, in `src/activations/mustache/storage.rs` or a new `store.rs`) whose methods mirror the current concrete `MustacheStorage` method signatures 1:1:

   | Trait method | Current concrete signature |
   |---|---|
   | `get_template` | `async fn (&self, plugin_id: &Uuid, method: &str, name: &str) -> Result<Option<String>, MustacheError>` |
   | `set_template` | `async fn (&self, plugin_id: &Uuid, method: &str, name: &str, template: &str) -> Result<TemplateInfo, MustacheError>` |
   | `list_templates` | `async fn (&self, plugin_id: &Uuid) -> Result<Vec<TemplateInfo>, MustacheError>` |
   | `delete_template` | `async fn (&self, plugin_id: &Uuid, method: &str, name: &str) -> Result<bool, MustacheError>` |

   Use `async_trait` or native async-fn-in-trait (whichever compiles cleanly on the crate's current MSRV and toolchain). The spike picks one and pins it; STG-2 canonicalizes across all activations.

3. Rename the current `MustacheStorage` struct to `SqliteMustacheStore` (or similar) and have it `impl MustacheStore`. All bodies move into the trait impl, unchanged.

4. Update Mustache activation's constructor to accept `Arc<dyn MustacheStore>` (or `Arc<dyn MustacheStore + Send + Sync>` — whatever the trait-object dance requires).

5. Update the existing tests in `storage.rs` to instantiate `SqliteMustacheStore` and call methods via the trait object. The assertions in each test (`test_set_and_get_template`, `test_update_template`, `test_list_templates`, `test_delete_template`) must not change.

6. Run `cargo test -p plexus-substrate --test '*mustache*'` or equivalent narrowest scope that hits Mustache's tests.

## Pass condition

All four current Mustache tests (`test_set_and_get_template`, `test_update_template`, `test_list_templates`, `test_delete_template`) pass against the trait-based rewrite, with assertions unmodified.

Binary: four tests passing → PASS. Any failure, compilation issue, or required assertion change → FAIL.

## Fail → next

If the spike fails due to `dyn`-trait-object limitations on async methods (e.g., `Send` bound issues, lifetime friction), the fallback is: parameterize activations with a generic `<S: MustacheStore>` type parameter instead of `dyn` — less flexible at runtime (cannot hotswap inside a running process) but preserves the trait seam.

If the spike fails due to `async_trait` macro not compiling on the current toolchain, try native async-fn-in-trait (Rust 1.75+). The workspace toolchain can be confirmed via `rust-toolchain.toml` or `cargo --version`.

## Fail → fallback

If both `dyn` and generic approaches fail, the epic's core assumption is wrong and STG-2 cannot proceed. Document the blocking constraint, re-scope the epic to "per-activation storage refactor via a concrete struct + feature flag" (no trait), and replan STG-3..10 accordingly.

## Time budget

Three focused hours. If the spike exceeds this, stop and report regardless of pass/fail state — the budget overrun itself is signal that the trait shape is fighting Rust rather than supporting the design.

## Out of scope

- In-memory backend implementation (that's STG-S02).
- Any other activation.
- Changing method semantics, parameters, or return types beyond the trait envelope.
- ST newtypes — Mustache currently uses `&Uuid` and `&str`; keep those for the spike. STG-8 migrates Mustache to `TemplateId` and friends.

## Completion

Spike delivers:

- A single commit (or branch tip) with the `MustacheStore` trait, `SqliteMustacheStore` impl, and unchanged tests passing.
- Pass/fail result.
- Time spent.
- A one-paragraph report on which trait-object mechanism (`async_trait`, native async fn in trait, generic `<S>`) was used and any friction encountered.

Report lands in STG-2's Context section before STG-2 is promoted to Ready. Spike branch is NOT merged — STG-2 consumes the finding and lands the production version.
