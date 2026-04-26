---
id: LAMBDA-5
title: "Lambda node activation: param + body children, eval returns closure"
status: Pending
type: implementation
blocked_by: [LAMBDA-2, LAMBDA-9]
unlocks: [LAMBDA-10]
severity: High
target_repo: plexus-lambda
---

## Problem

`plexus-lambda` needs a `LambdaExpr` activation: the AST node for `\x -> <body>`. Evaluating a lambda does not recurse into its body — it captures the current environment and returns a closure value. The `LambdaExpr` activation must expose typed children for the param binding and the body expression, and must produce a closure value on `eval` that carries the captured env.

## Context

Target crate: `plexus-lambda` at `/Users/shmendez/dev/controlflow/hypermemetic/plexus-lambda/`.

Depends on LAMBDA-2 (`Expr::Lambda`, `VarName`, `Env`, `Value::Closure`) and LAMBDA-9 (env wire representation — closures carry a captured env across Plexus RPC).

A `LambdaExpr` activation wraps:

| Field | Type | Plexus RPC exposure |
|---|---|---|
| `param` | `VarName` | Static child gate `param()` returning a `Binding` activation (or equivalent typed-value view). |
| `body` | Dynamic child — can be any `Expr` variant | `#[plexus_macros::child]` dynamic gate `body()` returning a handle whose target resolves to whichever node-kind activation represents the body. |

The `body` child is **dynamic** because a lambda's body can be any of the five Expr variants. Use the `#[plexus_macros::child]` attribute with whatever opt-in variant matches the CHILD epic's final macro surface for returning a polymorphic expression handle (static `Handle<Expr>` if the macro accepts it; dynamic-lookup otherwise).

Eval semantics:

| Input | Operation | Expected observable |
|---|---|---|
| `LambdaExpr { param: "x", body: Var("x") }`, arbitrary env | `eval(env)` | Returns `Value::Closure { param: "x", body: <body_handle_or_expr>, captured: env.clone() }` |
| Same, env has `y -> Literal(1)` | `eval(env)` | The returned closure's `captured` env contains `y -> Literal(1)` |

The closure's `body` representation depends on LAMBDA-9's decision: either a by-value `Box<Expr>` (serializable) or a handle to the body's activation on the hub. Honour LAMBDA-9's call.

## Risks

- **Capturing the env cheaply.** Use `Arc<im::HashMap<VarName, Value>>` so cloning the captured env is O(1). Already pinned in LAMBDA-2.
- **Polymorphic body child gate.** The CHILD macro must support returning a handle of a trait-object-ish expression kind. If it does not, the spike (LAMBDA-S01) should have caught it. If a surprise surfaces here, open a follow-up CHILD ticket rather than narrowing LAMBDA's design.
- **Infinite recursion on `eval` when the body is itself a `Lambda`.** Not a risk in practice — `eval` on Lambda does NOT recurse into the body, only captures it. Guard against an accidental recursive `eval` call in the implementation.

## What must NOT change

- LAMBDA-2 types (no edits).
- Other node-kind files (LAMBDA-3, 4, 6, 7, 8).
- Plexus RPC wire format.

## Acceptance criteria

1. A file `plexus-lambda/src/expr/lambda.rs` exists, containing exactly one `#[plexus_macros::activation(namespace = "expr")]` block for `LambdaExpr`.
2. `LambdaExpr` exposes:
   - One RPC method `eval(env)` returning a `Result<Value, EvalError>`.
   - A `#[plexus_macros::child]` static gate `param()` addressing the param binding.
   - A `#[plexus_macros::child]` dynamic gate `body()` addressing the body Expr node.
3. Unit tests cover the two rows of the "Required observable" table above. `cargo test -p plexus-lambda expr::lambda` passes.
4. An in-process Plexus RPC test invokes `eval` via the hub (not a direct Rust call) and asserts the returned `Value::Closure` has the expected captured env bindings.
5. The file is the sole LAMBDA-5-owned file. Disjoint from LAMBDA-3, 4, 6, 7, 8 per ticketing skill rule 10.
6. Integration gate: `cargo build -p plexus-lambda` and `cargo test -p plexus-lambda` pass green end-to-end.

## Completion

Implementor delivers a commit adding `plexus-lambda/src/expr/lambda.rs` with the activation, child gates, eval method, and tests. PR description includes test output. Ticket flips to Complete after LAMBDA-9 has also closed (or in the same commit if batched).
