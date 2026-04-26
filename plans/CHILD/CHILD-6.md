---
id: CHILD-6
title: "crate_path default via proc-macro-crate; drop per-activation boilerplate"
status: Complete
type: implementation
blocked_by: []
unlocks: [CHILD-7]
severity: Medium
target_repo: plexus-macros
---

## Problem

The `#[plexus_macros::activation]` macro takes a `crate_path` argument that governs where generated code resolves types such as `ChildRouter` and `Activation`. Its current default is `"crate"` (keyword-relative), which only resolves correctly when the macro is invoked inside plexus-core itself. Every external consumer must write `crate_path = "plexus_core"` by hand. In plexus-substrate, 14 of 15 macro-using activations carry this line (the outlier, `health`, avoids the macro entirely). Since plexus-core does not dogfood its own macro, this boilerplate serves no real use case and creates a failure mode for consumers that rename the crate.

## Context

Idiomatic Rust proc-macros (serde, thiserror, tracing-attributes) solve this with the `proc-macro-crate` helper, which reads the invoking crate's `Cargo.toml` at macro expansion time and returns the name under which the target dep is imported — including when the caller renames it via `[dependencies] foo = { package = "real-name" }`. Plexus RPC terminology is unchanged by this ticket: "Plexus RPC" remains the protocol, `DynamicHub` the in-process router.

## Required behavior

The decision pinned in this ticket: adopt `proc-macro-crate` to resolve the default `crate_path` (Option B from the design discussion). The macro detects the name the caller uses for `plexus-core` at expansion time and emits that path in generated code. Explicit `crate_path = "..."` overrides, as today, always win.

Resolution matrix after this ticket lands:

| Scenario | Resolved `crate_path` |
|---|---|
| Consumer `Cargo.toml` has `plexus-core = "..."` | Generated code uses `::plexus_core::...` |
| Consumer `Cargo.toml` has `renamed = { package = "plexus-core" }` | Generated code uses the renamed path (`::renamed::...`) |
| Consumer passes `crate_path = "custom"` explicitly | Generated code uses `::custom::...` (explicit wins) |
| Internal use inside plexus-core itself (should it ever dogfood the macro) | Either pass `crate_path = "crate"` explicitly, or rely on `proc-macro-crate`'s self-detection — both must yield a working path |
| Consumer has no `plexus-core` dependency at all and invokes the macro | Clear compile-time error naming the missing dependency |

Coupled to the new default, the 14 substrate activations that currently spell out `crate_path = "plexus_core"` have that argument removed and substrate still builds. The removal sweep is part of this ticket (not deferred to CHILD-7).

## Risks

| Risk | Mitigation |
|---|---|
| `proc-macro-crate` version compatibility with the rest of the plexus-macros dep tree | Verify `cargo build -p plexus-macros` green before merge; pin a compatible version in `Cargo.toml`. |
| Consumer crate invokes the macro with no `plexus-core` dependency | Macro emits a clear compile error naming the missing dependency rather than expanding to an unresolvable path. |
| Silent regression for a downstream that already renames or vendors plexus-core | Acceptance criterion 4 exercises the rename case end-to-end with a committed fixture crate. |

## What must NOT change

- Existing `crate_path = "plexus_core"` invocations continue to compile (they become redundant and are removed as part of this ticket).
- Explicit `crate_path = "custom"` overrides continue to work exactly as before.
- Generated code structure and semantics apart from the resolved path prefix.
- Wire format, method routing, hub behavior, and child router generation.

## Acceptance criteria

1. `cargo build -p plexus-macros` succeeds on a clean checkout with this ticket applied.
2. Every `crate_path = "plexus_core"` occurrence has been removed from the plexus-substrate activation crates, and `cargo build --workspace` in plexus-substrate still succeeds without any other edits to those activations.
3. `cargo test --workspace` in plexus-substrate passes.
4. A committed fixture crate that renames plexus-core in `Cargo.toml` (e.g., `renamed-core = { package = "plexus-core" }`) invokes `#[plexus_macros::activation]` without specifying `crate_path` and compiles successfully.
5. A committed fixture that invokes `#[plexus_macros::activation]` with `crate_path = "custom"` continues to emit code routed through the `custom` path (override still wins).
6. A committed fixture (trybuild or equivalent) that invokes the macro in a crate with no `plexus-core` dependency fails to compile with an error message that names the missing dependency.

## Completion

PR (or coordinated PRs) against `plexus-macros` and `plexus-substrate` landing the new default and the substrate boilerplate removal together. CI green on both sides. Status flipped from `Ready` to `Complete` in the same commit that lands the code.
