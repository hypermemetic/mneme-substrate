---
id: LAMBDA-8
title: "Let node activation: binding + value + body children, extended-env eval"
status: Pending
type: implementation
blocked_by: [LAMBDA-2, LAMBDA-9]
unlocks: [LAMBDA-10]
severity: High
target_repo: plexus-lambda
---

## Problem

`plexus-lambda` needs a `LetExpr` activation: the AST node for `let x = <value> in <body>`. Evaluation evaluates the value, extends the env with the new binding, and evaluates the body under the extended env. Without `LetExpr`, non-lambda local bindings are not supported — and the epic's minimal language scope names `Let` as one of the five required node kinds.

## Context

Target crate: `plexus-lambda` at `/Users/shmendez/dev/controlflow/hypermemetic/plexus-lambda/`.

Depends on LAMBDA-2 (`Expr::Let`, `VarName`, `Env`, `Value`) and LAMBDA-9 (env wire shape).

The `LetExpr` activation wraps:

| Field | Plexus exposure |
|---|---|
| `name: VarName` | Static child gate `binding()` returning the `Binding` view (or inline on the activation). |
| `value: Box<Expr>` | Static `#[plexus_macros::child]` gate `value()` returning handle to the value expression. |
| `body: Box<Expr>` | Static `#[plexus_macros::child]` gate `body()` returning handle to the body expression. |

Both `value` and `body` are dynamic-typed (any Expr kind) so their child gates use the polymorphic-expr handle representation pinned by CHILD / LAMBDA-S01.

## Required behavior

| Input | Operation | Expected observable |
|---|---|---|
| `let x = 1 in x` (parsed tree) | `eval(env)` with empty env | Returns `Value::Literal(Lit::Int(1))` |
| `let x = 1 in y` with env containing `y -> Literal(2)` | `eval(env)` | Returns `Value::Literal(Lit::Int(2))` — the let-bound `x` is added to the env but doesn't shadow `y` |
| `let x = 1 in let x = 2 in x` | `eval(env)` with empty env | Returns `Value::Literal(Lit::Int(2))` — inner let shadows outer |
| Called via Plexus hub (not direct Rust) | `eval(env)` | Same result; both child calls (value, body) route through RPC. |

Eval order: **evaluate value first, then bind, then evaluate body**. The value expression's eval happens via a child-gate RPC call, as does the body's eval. No recursion inside `LetExpr::eval` that bypasses the hub.

## Risks

- **Recursive `let`.** Classical `let rec` requires the env to contain the binding while the value is evaluated (so it can reference itself). The minimal language scope does **not** require `let rec` — use plain `let` semantics (value evaluated in the outer env, body in the extended env). Document this; a future ticket can add `let rec` as a distinct node kind if needed.
- **Env wire shape.** LAMBDA-9 pins this. Follow the decision.

## What must NOT change

- LAMBDA-2 types.
- Other node-kind files.
- Plexus RPC wire format.

## Acceptance criteria

1. A file `plexus-lambda/src/expr/let_expr.rs` (module name workable around `let` keyword) exists, containing exactly one `#[plexus_macros::activation(namespace = "expr")]` block for `LetExpr`.
2. `LetExpr` exposes `eval`, and child gates `value()` and `body()` both via `#[plexus_macros::child]`.
3. Unit tests cover the four rows of the "Required behavior" table. `cargo test -p plexus-lambda expr::let_expr` passes.
4. An in-process Plexus RPC test evaluates `let x = 1 in x` via the hub and asserts the result equals `Value::Literal(Lit::Int(1))`.
5. File is the sole LAMBDA-8-owned file. Disjoint from LAMBDA-3, 4, 5, 6, 7.
6. Integration gate: `cargo build -p plexus-lambda` and `cargo test -p plexus-lambda` pass green end-to-end.

## Completion

Implementor delivers a commit adding `plexus-lambda/src/expr/let_expr.rs` with the activation, child gates, eval method, and tests. PR description includes test output. Ticket flips to Complete after LAMBDA-9 has closed.
