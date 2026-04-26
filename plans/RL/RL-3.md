---
id: RL-3
title: "Replace load-bearing panics in Orcha (graph_runner unreachable, ticket_compiler × 3)"
status: Pending
type: implementation
blocked_by: []
unlocks: [RL-5]
severity: High
target_repo: plexus-substrate
---

## Problem

Orcha has four production-path panics that crash the whole runner on unexpected input rather than returning a typed error:

- `orcha/graph_runner.rs` — `unreachable!()` at the end of a match arm. If ever reached (schema evolution, unexpected node variant, malformed persisted state), the runner dies mid-graph and no progress is saved.
- `orcha/ticket_compiler.rs` — three `panic!("wrong spec")` sites in the compiler itself (not in tests). A malformed or unfamiliar ticket spec takes down the compiler instead of surfacing a typed compilation error to the caller.

These are load-bearing because they sit in paths that run under normal user input (new graph kinds, ticket specs authored by humans or agents). A single surprise crashes Orcha.

## Context

Orcha already owns an `OrchaError` enum with `From<String>` for ergonomic wrapping. The required change is to add structured variants for the failure modes currently handled via panic, and return `Result<_, OrchaError>` from the affected functions.

Expected structured variants (final names are at the implementor's discretion; the shape must carry enough context for a caller to diagnose):

- `OrchaError::GraphRunnerUnexpectedState { node_id: NodeId, state: ..., context: ... }` — replaces the `unreachable!()` site. If ST has shipped, `NodeId` is ST's newtype; otherwise, the bare type in HEAD.
- `OrchaError::TicketCompilerInvalidSpec { spec_kind: ..., field: ..., reason: String }` — replaces each `panic!("wrong spec")` site. Shape consistent across the three sites.

The caller signatures of functions containing these panics already return `Result` in most cases. Where they do not, the function signature changes to `Result<T, OrchaError>`. This is a per-function call-site audit inside `orcha/graph_runner.rs` and `orcha/ticket_compiler.rs` only.

`orcha/error.rs` is **shared with RL-5**. This ticket lands first and introduces the variant shape; RL-5 adds its own persistence-failure variants against the file this ticket leaves. Implementors of RL-5 should rebase on RL-3 before editing.

## Required behavior

| Orcha input | Current observable behavior | Required observable behavior |
|---|---|---|
| A graph node whose state reaches the `unreachable!()` arm | Runner panics; substrate process likely crashes or thread aborts; no graph progress checkpointed | `OrchaError::GraphRunnerUnexpectedState { ... }` is returned to the caller; Orcha's tracing span logs the variant at ERROR level; the graph is marked failed in storage; no panic. |
| A ticket spec that mismatches the compiler's expected shape | `panic!("wrong spec")` crashes the compiler | `OrchaError::TicketCompilerInvalidSpec { ... }` is returned to the caller; tracing event at WARN or ERROR level; the calling RPC method surfaces the error to the client via its method signature's `Result` type. |
| All valid graph nodes and ticket specs (regression) | Runner and compiler behave as HEAD | Unchanged. |

## Risks

- **Signature change cascade.** Promoting a function from `-> T` to `-> Result<T, OrchaError>` changes every call site. The scope is bounded to `orcha/graph_runner.rs` and `orcha/ticket_compiler.rs`, but may ripple into `orcha/activation.rs` callers. Implementor updates all call sites in the same commit. If the cascade reaches outside `orcha/`, stop and replan — that would be a scope violation against the file-boundary table in RL-1.
- **Variant naming collision with RL-5.** Both tickets edit `orcha/error.rs`. RL-5 rebases on RL-3. If RL-5's persistence-failure variants collide with RL-3's names, rename at RL-5 time — RL-3 is not responsible for reserving names.

## What must NOT change

- The set of RPC method names or request/response shapes on the Orcha activation.
- The SQLite schema for Orcha's storage (no migrations added by this ticket).
- The set of valid inputs that today reach a non-panic path: those continue to succeed.
- Orcha's tracing span structure outside the new ERROR/WARN events this ticket adds.
- Existing `cargo test` pass rate.
- Files outside `orcha/graph_runner.rs`, `orcha/ticket_compiler.rs`, `orcha/error.rs`, and the call-site files inside `orcha/` that consume the changed signatures.

## Acceptance criteria

1. Grep for `panic!` in `orcha/ticket_compiler.rs` returns zero matches.
2. Grep for `unreachable!` in `orcha/graph_runner.rs` returns zero matches.
3. `orcha/error.rs` has two new structured variants (or equivalent) covering the graph-runner unexpected-state case and the ticket-compiler invalid-spec case, each carrying enough context for a caller to diagnose without reading source.
4. A unit test inside the `orcha` module feeds a ticket spec that previously triggered each `panic!("wrong spec")` site and asserts the returned `Err` matches the expected variant.
5. A unit test inside the `orcha` module constructs a graph-runner state that previously reached the `unreachable!()` arm and asserts the returned `Err` matches the expected variant.
6. All existing `cargo test` targets pass.
7. The substrate binary still starts cleanly against a fresh `~/.plexus/substrate/` directory (no startup regression).

## Completion

Implementor delivers:

- Patch to `orcha/error.rs` adding the two new variants.
- Patch to `orcha/graph_runner.rs` replacing `unreachable!()` with `?` propagation.
- Patch to `orcha/ticket_compiler.rs` replacing each `panic!("wrong spec")` with `?` propagation.
- Updated call sites within `orcha/` to thread the new `Result` types.
- Two new unit tests (criteria 4 and 5).
- `cargo test` output confirming criterion 6.
- Status flip to `Complete` in the same commit that lands the code.
