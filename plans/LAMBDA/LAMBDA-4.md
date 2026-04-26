---
id: LAMBDA-4
title: "Var node activation: eval looks up binding in env"
status: Pending
type: implementation
blocked_by: [LAMBDA-2]
unlocks: [LAMBDA-10]
severity: High
target_repo: plexus-lambda
---

## Problem

`plexus-lambda` needs one activation per AST node kind. The `Var` node kind has its own ticket so each node kind can be implemented in parallel with the others against the shared LAMBDA-2 types. Without a `VarExpr` activation, no other node can evaluate a program that references a bound name — which includes every non-trivial program.

## Context

Target crate: `plexus-lambda` at `/Users/shmendez/dev/controlflow/hypermemetic/plexus-lambda/`.

Depends on LAMBDA-2's `Expr::Var`, `VarName`, `Env`, `Value`, `EvalError::UnboundVariable`.

A `VarExpr` activation wraps a single `VarName` (the identifier being referenced) and exposes `eval(env)` that looks the name up in the env and returns the bound `Value`. If unbound, it returns `EvalError::UnboundVariable(name)`.

The activation has no children — it is an AST leaf.

Env is represented in-process as `Arc<im::HashMap<VarName, Value>>` per LAMBDA-2. Var's `eval` only performs an in-process lookup; the **wire** shape of Env (pinned by LAMBDA-9) does not affect Var's implementation because Var receives an already-deserialized Env. This ticket does **not** block on LAMBDA-9 — Var can land and reach Complete without waiting for the env wire-shape decision. If a later LAMBDA-9 wire-representation change alters the in-process `Env` alias itself, a minor follow-up edit here is acceptable; the current epic pins the in-process shape, so no such edit is expected.

## Required behavior

| Input | Operation | Expected observable |
|---|---|---|
| `VarExpr { name: "x" }`, env has `x -> Value::Literal(Lit::Int(42))` | `eval(env)` | Returns `Value::Literal(Lit::Int(42))` |
| `VarExpr { name: "y" }`, env does not contain `y` | `eval(env)` | Returns `EvalError::UnboundVariable(VarName("y"))` |
| `VarExpr { name: "x" }`, env has `x -> Value::Closure { ... }` | `eval(env)` | Returns the closure by value (or handle — aligned with LAMBDA-9's env-shape decision) |

The activation is declared with `#[plexus_macros::activation(namespace = "expr")]` (same namespace as the other Expr node kinds — they co-exist as siblings in the expr routing table, disambiguated by their registered activation names).

## Risks

- **Closure values in the env.** A Var can bind to a closure value; returning a closure through `eval` crosses the wire and its representation is LAMBDA-9's concern. Var's implementation just returns whatever the env holds — wire-level representation is already resolved by whoever passed the env in. No action needed at this ticket's landing time.

## What must NOT change

- LAMBDA-2 types (no edits to `Expr`, `Env`, `Value`, `EvalError` from this ticket).
- Other node-kind activations (LAMBDA-5..8 own their own files).
- The parser's output shape (LAMBDA-3 owns it).

## Acceptance criteria

1. A file `plexus-lambda/src/expr/var.rs` exists, containing exactly one `#[plexus_macros::activation(namespace = "expr")]` block for `VarExpr`.
2. `VarExpr` exposes exactly one RPC method: `eval` taking an `Env` and returning a `Result<Value, EvalError>` (or the equivalent shape LAMBDA-9 pins).
3. Unit tests in the same file cover the three rows of the "Required behavior" table. `cargo test -p plexus-lambda expr::var` passes.
4. No `#[plexus_macros::child]` attribute in the file — `VarExpr` is a leaf.
5. The file is the sole LAMBDA-4-owned file; it is disjoint from LAMBDA-3, 5, 6, 7, 8 file writes per ticketing skill rule 10.
6. Integration gate: `cargo build -p plexus-lambda` and `cargo test -p plexus-lambda` pass green end-to-end.

## Completion

Implementor delivers a commit adding `plexus-lambda/src/expr/var.rs`, the unit tests, and the module-tree wiring. PR description includes test output. Ticket flips Ready → Complete in the same commit.
