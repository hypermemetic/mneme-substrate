---
id: RL-2
title: "Replace src/builder.rs .expect() chains with typed startup errors"
status: Pending
type: implementation
blocked_by: []
unlocks: [RL-10]
severity: High
target_repo: plexus-substrate
---

## Problem

`src/builder.rs` uses `.expect(...)` on every storage init call. Any partial failure during boot (missing `~/.plexus/substrate/activations/{name}/` directory, corrupt SQLite file, wrong permissions, schema migration failure) kills the whole server with a raw panic and a generic expect message. Operators see "thread 'main' panicked at '...' " instead of an actionable error pointing to which activation failed, which path was involved, and what the underlying cause was.

## Context

`builder.rs` initialises each activation in turn; each stateful activation opens its per-activation SQLite pool under `~/.plexus/substrate/activations/{name}/`. The existing pattern calls `.await.expect("<activation> storage init failed")` or similar on every call. The audit flagged this as "`.expect()` on every storage init. Startup panics on partial failure."

Substrate already has per-activation error enums (`OrchaError`, `ConeError`, `ClaudeCodeError`, etc.). What is missing is a **substrate-level** startup error that wraps them with activation identity and path context so the binary entry point can print an actionable message and exit non-zero without panicking.

The existing `From<String>` escape hatch on each activation error enum can be used if a structured variant is too invasive for a first pass, but the preferred shape is a structured variant per failure mode (storage-init, schema-migration, cyclic-parent-injection).

## Required behavior

| Startup condition | Current observable behavior | Required observable behavior |
|---|---|---|
| All activations initialise cleanly | Server starts; logs register each activation | Unchanged. |
| One activation's SQLite pool fails to open (e.g., permission denied) | Server panics with `.expect` message | Binary prints a structured error naming the activation, the path attempted, and the underlying OS / sqlx error; exits with non-zero status. No panic. |
| One activation's schema migration fails | Server panics | Binary prints structured error naming the activation and the migration step; exits non-zero. No panic. |
| `~/.plexus/substrate/activations/{name}/` directory cannot be created | Server panics | Binary prints structured error naming the path and OS error; exits non-zero. No panic. |
| A later activation's init succeeds but an earlier one has silently failed (impossible in today's code; included as regression guard) | n/a | Startup returns on first failure; no subsequent activation init runs. |

Error output format (binary entry point):

```
substrate: startup failed
  activation: <name>
  stage: <storage-init | schema-migration | cyclic-parent-injection>
  context: <path or identifier>
  cause: <underlying error's Display>
```

## Risks

- **Structured error type placement.** A new `SubstrateStartupError` enum at the binary level is cleanest but adds a new module. Alternative: reuse `anyhow::Error` at the binary entry point and add structured context via `with_context()`. Either is acceptable; the ticket picks whichever minimises diff size at implementation time, preserving the observable format above. This is a coding decision, not a planning decision — no spike needed.
- **Ordering assumptions.** Some activations' init may appear idempotent but actually leave a half-initialised pool on failure. The ticket does not attempt to make init transactional; it only ensures that reporting is accurate and the process exits cleanly.

## What must NOT change

- The **order** in which activations are initialised in `builder.rs`. Cyclic-parent injection via `OnceLock<Weak<DynamicHub>>` is preserved.
- The startup success path's observable behavior: when all activations initialise, the server is indistinguishable from HEAD.
- Per-activation SQLite file paths under `~/.plexus/substrate/activations/{name}/`.
- Existing `cargo test` pass rate.
- The `main` function's public surface (it still returns whatever it returns today — typically `Result<(), ...>` or `()` with exit codes via `std::process::exit`).

## Acceptance criteria

1. Grep for `.expect(` in `src/builder.rs` returns zero matches in the storage-init and activation-register code paths.
2. A manual test: temporarily `chmod 000` one activation's storage directory (e.g., `~/.plexus/substrate/activations/orcha/`), run `cargo run`, observe: no panic message (no `thread 'main' panicked`); process exits with non-zero status; stderr contains a structured error block matching the format above with `activation: orcha`, `stage: storage-init`, the attempted path, and the OS permission-denied cause.
3. A unit or integration test inside the substrate crate constructs a builder pointed at a non-existent / unwritable directory and asserts that the error returned is the structured startup-error variant (not a panic, not a bare string).
4. All existing `cargo test` targets pass.
5. When all activations initialise successfully, `cargo run` logs the same set of "registered activation X" lines (or equivalent) as HEAD, in the same order.

## Completion

Implementor delivers:

- Patch to `src/builder.rs` (and a new error module if introduced) replacing every storage-init `.expect(...)` with `?` propagation against a typed startup error.
- Patch to the binary entry point (`src/main.rs` or `src/bin/*.rs`) that formats the startup error using the block above and exits non-zero on failure.
- Test output showing the manual permission-denied test from acceptance criterion 2, and the `cargo test` run from criterion 4.
- Status flip to `Complete` in the same commit that lands the code.
