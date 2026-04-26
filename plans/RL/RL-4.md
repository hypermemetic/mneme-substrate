---
id: RL-4
title: "Replace load-bearing panic in Bash executor (panic on unexpected variant)"
status: Pending
type: implementation
blocked_by: []
unlocks: [RL-7]
severity: High
target_repo: plexus-substrate
---

## Problem

`bash/executor/mod.rs` contains `panic!("Expected stdout/exit")` when a `BashOutput` variant arrives that the executor did not expect. This sits on the bash subprocess lifecycle path, which means any change in shape of the upstream subprocess output stream (e.g., a new variant added for a new feature, a reordered enum) crashes the bash executor and takes any calling activation (Cone, Orcha-via-bash) with it.

The panic is load-bearing because it is not on a "can't happen" branch — it is on an exhaustiveness-of-variants assumption that is not compile-enforced.

## Context

The `BashOutput` enum is the type carrying subprocess stream events. The executor dispatches on the variant and expects only `Stdout` / `Exit` (or similar names) at the panic site. Adding a new variant, or receiving one out of order, reaches the panic.

Bash owns a `BashError` enum. The required change is a structured variant for "unexpected output variant" carrying enough context for the caller to diagnose. Exact variant name is implementor's discretion; shape must carry:

- The variant that was received (serialised as a string or debug repr).
- The set of variants the executor expected at that point.
- Any in-flight subprocess identifier (pid, command line summary).

`bash/executor/mod.rs` is **shared with RL-7's stderr truncation fix**. This ticket lands first (larger surface), RL-7 second against the resulting file.

## Required behavior

| Bash subprocess output | Current observable behavior | Required observable behavior |
|---|---|---|
| Expected `Stdout` / `Exit` variant | Normal path | Unchanged. |
| An unexpected `BashOutput` variant reaches the executor's dispatch | `panic!("Expected stdout/exit")` crashes the executor and any caller | `BashError::UnexpectedOutputVariant { ... }` is returned to the caller; tracing event at ERROR level; no panic; the executor leaves the subprocess in a clean-terminated state (best-effort: abort the subprocess if still running). |
| A caller issues a bash command and the subprocess exits normally (regression) | `BashOutput::Exit` reaches the executor, executor returns Ok | Unchanged. |

## Risks

- **Subprocess cleanup on unexpected variant.** If the executor bails early on an unexpected variant, the subprocess may still be running. Implementor's best effort is to `kill()` the child (or equivalent via tokio's `Child::kill()`) before returning the error. A failure of that kill is *not* a blocker for this ticket — log it at WARN and continue with the primary error return.
- **Missing variants today.** If the current `BashOutput` enum has only `Stdout` and `Exit`, the panic site is unreachable today. Ticket still replaces it (as a "future-proofing without assumption-burying" measure) but the unit test in acceptance criterion 3 requires either adding a synthetic variant behind `#[cfg(test)]` or using a mock that yields a non-existent variant via test-only hooks. Implementor picks the cheapest of the two.

## What must NOT change

- The bash executor's happy-path output buffering, stdout/exit semantics, or return shape to callers on successful subprocess runs.
- The `BashOutput` variants that today reach the executor normally.
- The executor's stderr handling (RL-7 owns the truncation fix; this ticket leaves stderr paths alone).
- Existing `cargo test` pass rate.
- Files outside `bash/executor/mod.rs` and `bash/error.rs`.

## Acceptance criteria

1. Grep for `panic!` in `bash/executor/mod.rs` returns zero matches.
2. `bash/error.rs` has a new structured variant covering the unexpected-output-variant case, carrying the context fields listed above.
3. A unit test inside the `bash` module drives the executor with an unexpected variant (via a `#[cfg(test)]` synthetic variant or a test-only mock) and asserts the returned `Err` matches the expected variant.
4. A unit test confirms the happy path (`Stdout` then `Exit`) still returns `Ok` with the same shape as HEAD.
5. All existing `cargo test` targets pass.

## Completion

Implementor delivers:

- Patch to `bash/executor/mod.rs` replacing the `panic!` with `?` propagation.
- Patch to `bash/error.rs` adding the new variant.
- Two unit tests (criteria 3 and 4).
- `cargo test` output confirming criterion 5.
- Status flip to `Complete` in the same commit that lands the code.
