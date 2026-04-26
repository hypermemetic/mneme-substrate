---
id: STG-10
title: "End-to-end integration: substrate full test suite against non-SQLite backend"
status: Pending
type: implementation
blocked_by: [STG-3, STG-4, STG-5, STG-6, STG-7, STG-8, STG-9]
unlocks: []
severity: High
target_repo: plexus-substrate
---

## Problem

Per-activation trait migrations in STG-3..9 each prove their local activation passes tests against both backends. STG-10 proves the whole substrate composes: wire in-memory backends for every storage-bearing activation (plus MCP session) via a test-only builder path, run the full `cargo test -p plexus-substrate` suite, and assert every test passes. This is the epic's integration gate — it confirms no activation's in-memory backend has hidden incompatibilities with another activation's assumptions.

Without this ticket, we have seven independent proofs but no composite proof. The failure mode we're guarding against: activation A's SQLite backend returns results in order X while activation A's in-memory backend returns order Y, and activation B (downstream) depends on order X silently. Only a full-suite run catches that class of bug.

## Context

Target file set: `src/builder.rs` (or a new test-only builder variant), plus whatever test harness glue lives adjacent.

Pattern pinned in `docs/architecture/<nanotime>_storage-trait-pattern.md` (STG-2).

**Inputs:**
- Every `*Store` trait with both backends exists and is individually tested (STG-3..9 complete).
- The `test-doubles` feature flag (or equivalent gating) exposes every `InMemory*Store` type.
- `builder.rs` has a documented production path; this ticket adds an all-in-memory variant.

**Cross-epic inputs:**
- None directly — this ticket runs at the end of the STG epic.

## Required behavior

1. **Add a test-only builder entry point** — in `src/builder.rs` or a sibling `src/builder/test_harness.rs`, expose a function (e.g., `build_substrate_all_in_memory() -> Arc<DynamicHub>`) that:
   - Constructs each storage-bearing activation with its `InMemory*Store` backend.
   - Constructs the MCP session manager with `InMemoryMcpSessionStore`.
   - Wires parent injection and hub registration identically to the production path.
   - Returns a fully-functional substrate hub suitable for in-process testing.

2. **Run the full substrate test suite against the all-in-memory build.**
   - Execute `cargo test -p plexus-substrate --features test-doubles` (or the feature flag that enables the all-in-memory builder).
   - Every existing test that invokes the standard builder's default path must have a sibling variant that invokes the all-in-memory builder, OR tests must be parameterized over the builder choice.
   - The implementor picks the approach (feature-gated duplicate modules vs. runtime parameterization vs. fixture fn swap) based on the existing test-harness patterns in substrate.

3. **Document the harness** — a short section in the STG-2 pattern doc (appended in this PR) explains how to add a new activation to the all-in-memory builder when future activations are created.

## Regression guarantees

| Surface | Post-ticket behavior |
|---|---|
| Default (SQLite) build | Unchanged. All pre-epic tests continue to pass against it. |
| `cargo test -p plexus-substrate` (default) | All tests green. |
| All-in-memory build | Every test that passes against SQLite also passes against in-memory. |
| Production user-facing behavior | Zero change — this ticket only adds a test path. |

## Risks

| Risk | Mitigation |
|---|---|
| A test implicitly depends on SQLite-specific semantics (ordering, timestamp resolution, specific error messages) that in-memory cannot reproduce. | For each such test, the contract is under-specified. Tighten the relevant `*Store` trait docstring in the trait's owning crate module, amend the in-memory backend to match, and re-run. If genuinely irreconcilable, flag as an epic finding and fail this ticket — the abstraction has a gap. |
| Feature-flag matrix proliferation (`test-doubles`, default, various opt-in) makes CI confusing. | Pin exactly two CI modes: `cargo test -p plexus-substrate` and `cargo test -p plexus-substrate --features test-doubles`. No other variants. |
| Cross-activation coupling (e.g., Orcha reads Loopback directly per DC epic's concern) causes in-memory Loopback + SQLite Orcha to desynchronize in mixed tests. | This ticket requires ALL activations in the build to be the same backend kind. No mixed builds. The all-in-memory build is homogeneous. |

## What must NOT change

- Production (default) builder path.
- Production SQLite DB paths, schemas, behaviors.
- Any activation's Plexus RPC surface.
- Any existing test's assertions (the test body runs against both backends — the assertion set is identical).
- Cargo feature names already in use.

## Acceptance criteria

1. `cargo build -p plexus-substrate` succeeds (default features).
2. `cargo build -p plexus-substrate --features test-doubles` succeeds.
3. `cargo test -p plexus-substrate` — all tests pass against the default (SQLite) backend.
4. `cargo test -p plexus-substrate --features test-doubles` — all tests pass against the all-in-memory backend.
5. A new integration test at `tests/all_in_memory_smoke.rs` (or similar) constructs a substrate via `build_substrate_all_in_memory()`, calls at least one method on at least three different activations (e.g., Arbor, Mustache, Orcha), and asserts expected results.
6. The STG-2 pattern doc gains an appended section titled "Adding a new activation to the all-in-memory builder" with a step-by-step checklist.
7. PR description tabulates the two CI commands and their pass/fail status.

## Completion

- PR against `plexus-substrate` landing the test-harness builder, any test parameterization glue, the integration smoke test, and the pattern doc update.
- PR description includes both `cargo test` invocations' full output (or at least the summary lines) — both green.
- PR description notes any contract-tightening required in `*Store` trait docstrings during this ticket (and in which trait).
- PR description confirms the STG epic's end-to-end goal: "an in-memory substrate passes the same tests as the SQLite substrate."
- Ticket status flipped from `Ready` → `Complete` in the same commit as the code.
- Epic STG-1's Completion section is satisfied; the epic closes.
