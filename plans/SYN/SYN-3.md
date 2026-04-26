---
id: SYN-3
title: "End-to-end integration smoke test — Solar drivable from synapse"
status: Pending
type: implementation
blocked_by: [SYN-2, CHILD-7]
unlocks: []
severity: Medium
target_repo: synapse (integration tests) + plexus-substrate (fixture)
---

## Problem

SYN-2 adds capability-aware rendering and tab-completion to synapse. Individual unit tests on each piece don't catch regressions that only surface when a real substrate is running, a real activation is migrated, and a real synapse is driving it. This ticket is the end-to-end guarantee — a scripted or documented session that exercises the whole stack and records the output.

## Context

Integration point:
- Substrate binary built with CHILD-7 Solar migration applied.
- Substrate running on a test port (not 4444, which may conflict with the user's live instance).
- Synapse built with SYN-2.
- Script (shell or Haskell) driving synapse through a known sequence of interactions.

## Required behavior

A test harness (location: `synapse/tests/integration/` or equivalent — pick the location consistent with how synapse currently organizes tests) that:

1. Starts a fresh substrate instance on a free ephemeral port.
2. Connects synapse to that instance.
3. Exercises the sequence below and asserts the output.

| Step | Action | Expected |
|---|---|---|
| 1 | `synapse <port> solar` | Output lists `observe`, `info`, and a `body {name}` dynamic gate entry. `body` description matches Solar's `///` doc comment. |
| 2 | Request `solar.list_children` via synapse | Stream of strings; collected items are a superset of {mercury, venus, earth, mars, jupiter, saturn, uranus, neptune} (exact set depends on `build_solar_system()`). |
| 3 | Tab-complete at `synapse <port> solar body <TAB>` | Completions include the planet names from step 2. |
| 4 | `synapse <port> solar body mercury info` | Returns mercury's body info (name, type, mass, etc.). Response is non-empty. |
| 5 | `synapse <port> solar body Mercury info` | Case-insensitive; returns mercury's info (same as step 4). |
| 6 | `synapse <port> solar observe` | Returns the system-level overview event. |
| 7 | `synapse <port> solar body not_a_planet info` | Returns a `MethodNotFound`-style error (not a crash). |
| 8 | Kill the substrate instance | Harness cleans up; no orphan processes. |

## Risks

| Risk | Mitigation |
|---|---|
| Flaky port allocation | Use OS-assigned port (bind to 0 and read the chosen port), pass to synapse explicitly. |
| Substrate startup takes several seconds | Health-check the port with a short poll before connecting. Timeout at 10s. |
| Synapse tab-completion isn't scriptable from outside the interactive shell | Emulate at the library level: call the completion function synapse uses internally with the same input, assert the output list. Do not require a TTY. |
| Integration test is slow and gets skipped in CI | Gate behind a feature flag or integration-test suite; document invocation in synapse README. |

## What must NOT change

- Substrate's fast startup path; the test must not require substrate config that differs from production.
- Solar's response shapes (fixed by CHILD-7).
- Synapse's interactive shell behavior (this test exercises scripted invocation only).

## Acceptance criteria

1. Integration test harness committed to the synapse repo; runnable via a documented command (e.g., `cabal test integration-solar` or `make integration`).
2. The full 8-step sequence above passes.
3. The test captures a transcript of steps 1–6 as a textual fixture in the PR description (or committed alongside as a golden file) showing the actual synapse output for visual inspection.
4. Running the test twice in sequence produces identical output (no flakiness on port allocation, no orphan substrate instances).
5. The test takes under 30 seconds end-to-end on a developer laptop.

## Completion

PR against `synapse` (with substrate fixture if one is needed). CI green. Transcript captured. Status flipped from `Ready` to `Complete` in the same commit that lands the test. With SYN-3 Complete, the SYN-1 epic Completes.
