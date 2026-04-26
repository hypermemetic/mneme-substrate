---
id: LAMBDA-10
title: "Integration test: parse + eval (\\x -> x + 1) 41 via RPC returns Literal(42)"
status: Pending
type: implementation
blocked_by: [LAMBDA-3, LAMBDA-4, LAMBDA-5, LAMBDA-6, LAMBDA-7, LAMBDA-8, LAMBDA-9]
unlocks: [LAMBDA-11]
severity: High
target_repo: plexus-lambda
---

## Problem

Every individual node activation has unit tests, but no test exercises the full pipeline end-to-end: start a substrate-style Plexus RPC hub, register the parser activation, call `parse` from an RPC client, receive a handle, call `eval` on it, walk the resulting activation graph across real RPC boundaries, and verify the computed result. Without this ticket, the epic has no confidence that the component parts compose.

## Context

Target crate: `plexus-lambda` at `/Users/shmendez/dev/controlflow/hypermemetic/plexus-lambda/`.

Depends on every node-kind activation and the env-handling decision: LAMBDA-3 (parser), LAMBDA-4 (Var), LAMBDA-5 (Lambda), LAMBDA-6 (Apply), LAMBDA-7 (Literal), LAMBDA-8 (Let), LAMBDA-9 (env wire).

The integration test runs against a **real in-process Plexus RPC hub** — no mocks, no direct Rust calls that bypass the hub. Follow the substrate-style test pattern (a `DynamicHub`, activations registered, a client that speaks Plexus RPC into it). The parser activation is registered once at hub setup; the test then uses the parser's `parse` RPC to construct the AST and the resulting handle's `eval` RPC to compute the value.

Target program: `(\x -> x + 1) 41`. The parser produces an `ApplyExpr` at the root, whose function is a `LambdaExpr` (param `x`, body is an add of `Var("x")` and `Literal(1)`), and whose arg 0 is `Literal(41)`. Eval is expected to produce `Value::Literal(Lit::Int(42))`.

## Required behavior

| Step | Input | Observable |
|---|---|---|
| 1. Hub setup | — | `DynamicHub` is constructed, parser activation registered. `cargo test` harness. |
| 2. `parse` RPC | Source `"(\\x -> x + 1) 41"` | Returns a handle to an `ApplyExpr` (or whatever the root kind resolves to under the AST). |
| 3. Enumerate hub | After `parse` | Hub contains at least: 1 `ApplyExpr`, 1 `LambdaExpr`, 2 `LiteralExpr` (for `1` and `41`), and the `Var("x")` node (and, per LAMBDA-6's primitive-`+` decision, possibly a primitive-binding activation). |
| 4. `eval` RPC on root handle | Empty env | Returns `Value::Literal(Lit::Int(42))`. |
| 5. Addressability check | Navigate `root.function().body()` via the hub's child routing | Returns a handle corresponding to the `+` application inside the lambda body. Calling `eval` on this sub-handle with env `{x -> Literal(1)}` returns `Value::Literal(Lit::Int(2))`. |

A second, smaller test program asserts the spike-level assertion explicitly: parse `(\\x -> x) 42`, eval, result equals `Literal(42)`. This guards regressions to the LAMBDA-S01 pattern.

Two more tests confirm corner cases:

| Program | Expected result |
|---|---|
| `let x = 1 in let x = 2 in x` | `Value::Literal(Lit::Int(2))` |
| `y` (unbound var) | `EvalError::UnboundVariable(VarName("y"))` surfaces to the RPC client, not a panic. |

All tests run through the hub. If any `eval` bypasses child-gate RPC and reaches into node-kind structs directly, the test must be rewritten.

## Risks

- **Primitive `+` plumbing.** LAMBDA-3 and LAMBDA-6 between them establish how `+` is handled. If the plumbing is incomplete at LAMBDA-10 landing time, the main test target is blocked. Mitigation: the two smaller test programs in the corner-case table above do not need `+` — they can ship even if `+` is partially wired. The main `(\x -> x + 1) 41` test is the critical one; if it fails, fix the upstream issue rather than weakening the assertion.
- **Hub child enumeration.** Asserting "hub contains N activations" requires enumeration. CHILD-4's `list_children` on a root-namespace router, or equivalent, is the hook. If this introspection is not available on the expected router, adapt the assertion to a reachability walk from the root handle.

## What must NOT change

- Any individual node-kind activation's public surface.
- Parser or env-handling contracts.
- The five `Expr` variants.

## Acceptance criteria

1. A test file `plexus-lambda/tests/integration.rs` exists, containing at minimum four tests:
   - `eval_identity_on_literal` — parses `(\\x -> x) 42`, asserts eval returns `Literal(42)`.
   - `eval_increment` — parses `(\\x -> x + 1) 41`, asserts eval returns `Literal(42)`.
   - `let_shadow` — parses `let x = 1 in let x = 2 in x`, asserts eval returns `Literal(2)`.
   - `unbound_var_error` — parses `y`, asserts eval yields an `UnboundVariable` error through the RPC return channel.
2. All four tests use a real in-process Plexus RPC hub; no direct activation-struct method calls.
3. `cargo test -p plexus-lambda --test integration` passes.
4. A rustdoc comment at the top of the integration-test file notes the "every eval goes via the hub" invariant and explains why (so a future maintainer does not "optimise" the tests into direct calls).
5. File is the sole LAMBDA-10-owned file. No writes to any upstream node-activation file.
6. Integration gate: `cargo build -p plexus-lambda` and `cargo test -p plexus-lambda` pass green end-to-end.

## Completion

Implementor delivers a commit adding `plexus-lambda/tests/integration.rs`, the four tests, and flips this ticket to Complete. PR description pastes the `cargo test -p plexus-lambda --test integration` output. LAMBDA-11 is unblocked.
