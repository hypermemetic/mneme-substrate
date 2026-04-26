---
id: RUSTGEN-8
title: "hub-codegen Rust: smoke tests + golden fixtures matching TS coverage"
status: Pending
type: implementation
blocked_by: [RUSTGEN-6]
unlocks: []
severity: High
target_repo: hub-codegen
---

## Problem

The current Rust backend test coverage is sparse: `tests/rust_codegen_smoke_test.rs` exists but verifies only shallow properties, and there are no golden fixtures for Rust matching the breadth of TS coverage (IR-7 deprecation, IR-9 typed-handle generation, static-child emission, per-namespace partitioning, filter support, etc.).

This ticket brings Rust test coverage to parity with the TypeScript backend: one golden fixture per scenario, structural assertions on generated output, and a compile-check pass that confirms the generated Rust actually builds.

## Context

**TS test reference:** `hub-codegen/tests/` contains:

- `typescript_codegen_test.rs` — golden fixture smoke test for TS backend.
- `ir7_deprecation_test.rs` — deprecation-annotation test (TS and Rust; TS passes, Rust partial).
- `ir9_dynamic_child_test.rs` — typed-handle test (TS passes, Rust skeleton).
- `rust_codegen_smoke_test.rs` — existing shallow Rust test.

Plus `tests/test_scenarios/` contains fixture IRs and their expected outputs (golden dirs) for each scenario.

**Test scenarios that must exist for Rust (parity with TS):**

1. **Basic RPC methods** — one namespace with only `Rpc` methods, no children. Golden: generated Rust compiles, method signatures match.
2. **Static children** — one hub activation with static children, no dynamic-child gates. Golden: parent client accessor returns child client.
3. **Dynamic children (typed handles)** — hub with dynamic-child gate, list_method, search_method. Golden: `DynamicChild<Child = <Real>Client>`, `Listable` and `Searchable` impls present, sibling methods hidden.
4. **Capability non-intersection** — gate without `search_method` — consumer code calling `.search(q)` fails to compile. This is a deliberate-compile-error fixture; expected result is `cargo check` FAILS, with the error message containing `Searchable` trait not implemented.
5. **Deprecation annotations (IR-7)** — method with `deprecation: Some(...)` emits `#[deprecated(since, note)]`. Golden: attribute present. `DeprecationWarning` emitted.
6. **Filter support** — IR with multiple namespaces; `--only alpha` produces only alpha's client files. Golden: file set matches filter.
7. **Cross-namespace type refs** — type in namespace A referenced from namespace B. Golden: B's types.rs has `use crate::A::types::X;`.
8. **Nested dynamic children** — hub → hub → gate. Golden: chained typed gates compile.
9. **Pre-IR schema regression** — IR without `MethodRole` field (legacy) produces output byte-identical to pre-RUSTGEN Rust output (modulo file-layout moves; if the file layout is different, the equivalence is on per-file content).
10. **Determinism** — two consecutive regens produce byte-identical output across all scenarios above.

**Golden fixture structure:** each scenario has:

```
tests/test_scenarios/rustgen_<n>_<name>/
  input.json        # the IR
  expected/         # expected generated output tree
    src/
      lib.rs
      types.rs
      transport.rs
      rpc.rs
      <ns>/client.rs
      <ns>/types.rs
      <ns>/mod.rs
    Cargo.toml
```

The test reads `input.json`, runs `hub-codegen`'s generate, diffs against `expected/`, and (for compile-check scenarios) copies the output to a temp dir and runs `cargo check`.

**Compile-check harness:** scenarios 1, 2, 3, 6, 7, 8 require the generated crate to actually compile. The harness:

1. Generates into a temp dir.
2. Copies / writes a minimal Cargo workspace.
3. Runs `cargo check` on the generated crate.
4. Asserts exit code 0.

Scenario 4 (deliberate compile error) asserts `cargo check` exits non-zero AND the stderr contains the expected error string.

## Required behavior

Add test files:

| File | Coverage |
|---|---|
| `tests/rust_codegen_smoke_test.rs` | EXPANDED — covers scenarios 1, 2, 5, 6, 7, 9, 10 (structural assertions on generated output). |
| `tests/rust_dynamic_child_test.rs` | NEW — covers scenarios 3, 4, 8 (typed-handle generation + compile-check + deliberate compile-error). |
| `tests/rust_compile_check_test.rs` | NEW — integration harness that runs `cargo check` on generated output for scenarios 1, 2, 3, 6, 7, 8. |

Plus golden fixture directories for each scenario under `tests/test_scenarios/rustgen_<n>_<name>/`.

**Compile-check scenarios can be gated behind a feature flag** (e.g., `#[cfg(feature = "compile-check")]`) so they don't slow CI if Rust toolchain isn't available. Default CI run includes the compile-check.

**Structural assertions** for every golden fixture:

- Output file set matches `expected/` exactly (no extra files, no missing files).
- Each file's content matches `expected/`'s content byte-for-byte.

**Failure messages** must identify which file diverged and show a diff snippet, so a test failure is actionable without hand-diffing.

## Risks

| Risk | Mitigation |
|---|---|
| Golden fixtures grow stale when codegen output changes legitimately. | Provide a regen script: `./scripts/regen_rustgen_goldens.sh` that updates fixtures. Document in a README in `tests/test_scenarios/`. |
| Compile-check scenarios require `cargo` on the CI path. | CI already has Rust. Gate behind a feature flag if local dev environments lack Rust but this is unlikely. |
| Generated Cargo.toml references external deps (`tokio-tungstenite`, etc.) — `cargo check` requires network on first run to fetch deps. CI has `cargo` caches but local runs may fetch. | Acceptable — a one-time fetch. Don't gate the test on cache warmth. |
| Deliberate compile-error fixture (scenario 4) fragility: if the generated code changes shape, the expected error message may drift. | Assert on a stable substring of the error (e.g., `"Searchable"`) rather than the full rustc error text. |
| Scenario 9 byte-identity against pre-RUSTGEN output: after RUSTGEN-2/3/4 the file layout has moved. "Byte-identical" is wrong — the right equivalence is "no semantic change to any individual generated item". | Replace scenario 9 acceptance with: pre-IR input produces output where every emitted type, method, and struct matches the pre-RUSTGEN output's per-item content (comparison by item-name, not by file path). If this is too hard to automate, document the equivalence manually in the PR. |

## What must NOT change

- TypeScript test suite — unchanged.
- Existing `rust_codegen_smoke_test.rs` continues to pass (expanded with additional coverage, not replaced).
- `tests/test_scenarios/` existing fixtures — unchanged. New fixtures are added alongside.
- Test runtime budget — new tests should complete in under 2 minutes total on CI (compile-check adds the biggest cost).
- Codegen behavior — this ticket tests; it doesn't change codegen.

## Acceptance criteria

1. `cargo test -p hub-codegen` succeeds with all new tests passing.
2. Ten golden fixture directories exist under `tests/test_scenarios/rustgen_<n>_<name>/`, one per scenario.
3. Each golden fixture has `input.json` (the IR) and `expected/` (the generated output tree). File counts match across generation and expectation.
4. `tests/rust_codegen_smoke_test.rs`, `tests/rust_dynamic_child_test.rs`, and `tests/rust_compile_check_test.rs` exist and are runnable.
5. The compile-check harness, given scenario 1 (basic RPC), generates into a temp dir, runs `cargo check`, and asserts exit 0.
6. Scenario 4 (deliberate compile error) runs `cargo check` on a consumer fixture that misuses the generated capability traits; the test asserts exit non-zero AND stderr contains the substring `Searchable`.
7. Scenario 10 (determinism): a test generates output twice for scenario 1 and asserts byte-identity across the two runs.
8. On a test failure, the output identifies the file path that diverged (not just "test failed"). Sample: `--- src/alpha/client.rs\nexpected:\n...\nactual:\n...`.
9. `tests/test_scenarios/README.md` (new file) documents how to regen goldens and how to add a new scenario.

## Completion

PR against hub-codegen. CI green. PR description includes:

- List of all new test files + fixture directories.
- Transcript of a local `cargo test -p hub-codegen` run showing all new tests passing.
- Summary of compile-check scenario count (how many scenarios' generated output is verified to actually compile).

Status flipped from `Ready` to `Complete` in the same commit.
