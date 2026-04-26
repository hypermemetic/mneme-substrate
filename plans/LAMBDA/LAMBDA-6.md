---
id: LAMBDA-6
title: "Apply node activation: function + indexed arg children, beta-reduction on eval"
status: Pending
type: implementation
blocked_by: [LAMBDA-2, LAMBDA-9]
unlocks: [LAMBDA-10]
severity: High
target_repo: plexus-lambda
---

## Problem

`plexus-lambda` needs an `ApplyExpr` activation: the AST node for `f x` / `f x y z` / `(\x -> x) 42`. Evaluating an application evaluates the function, evaluates each argument, and performs beta-reduction — extending the closure's captured env with the param binding and evaluating the body. Without `ApplyExpr`, no function can be called; the interpreter is useless.

## Context

Target crate: `plexus-lambda` at `/Users/shmendez/dev/controlflow/hypermemetic/plexus-lambda/`.

Depends on LAMBDA-2 (`Expr::Apply`, `Value`, `EvalError::ApplyNonFunction`, `EvalError::ArityMismatch`) and LAMBDA-9 (env wire shape).

The `ApplyExpr` activation exposes:

| Child gate | Shape | Purpose |
|---|---|---|
| `function()` | Static `#[plexus_macros::child]` returning a handle to the function expression | Address the function subtree. |
| `arg(idx: &str)` | Dynamic `#[plexus_macros::child(list = "arg_positions")]` returning `Option<Handle<Expr>>` | Address argument `idx`. |
| `arg_positions()` | List opt-in from the `child(list = ...)` attribute | Enumerate the registered argument indices. |

The arg gate is keyed by position stringified (`"0"`, `"1"`, ...). The opt-in `list_children` capability yields each `arg_N` name so synapse can autocomplete.

Eval semantics:

| Input | Operation | Expected observable |
|---|---|---|
| `ApplyExpr` whose function evaluates to `Value::Closure { param: "x", body, captured }` and one arg that evaluates to `Value::Literal(Lit::Int(42))` | `eval(env)` | Returns the result of evaluating `body` under `captured` extended with `x -> Literal(42)`. For `(\x -> x) 42`, this is `Value::Literal(Lit::Int(42))`. |
| `ApplyExpr` whose function evaluates to `Value::Literal(_)` (not a closure) | `eval(env)` | Returns `EvalError::ApplyNonFunction(Value::Literal(_))`. |
| `ApplyExpr` whose closure expects one param but the call-site provides zero or two args | `eval(env)` | Returns `EvalError::ArityMismatch { expected: 1, found: <n> }`. |
| `ApplyExpr` applying `(\x -> x + 1)` to `41` (once the `+` primitive is wired per LAMBDA-3's decision) | `eval(env)` | Returns `Value::Literal(Lit::Int(42))`. |

Argument evaluation is **left-to-right** and happens via RPC child-gate calls: `apply_expr.arg("0").eval(env)`, then `apply_expr.arg("1").eval(env)`, etc. The function is evaluated via `apply_expr.function().eval(env)`. Beta-reduction is then a single RPC call into the closure's body with the extended env.

## Risks

- **Arity discipline.** Lambda calculus is strictly unary. If the parser emits a multi-arg Apply (e.g., `f x y`), beta-reduce it as left-associative chain of unary applies — OR keep the multi-arg closure and extend the env with multiple bindings in one step. Pin the choice: **extend env in one step when closure param-count matches the apply arity; emit `ArityMismatch` otherwise**. Documented in a rustdoc on `ApplyExpr::eval`.
- **Function evaluates to something new (handle vs value closure).** LAMBDA-9 decides. Adapt the apply-eval logic to whichever representation LAMBDA-9 pins — the acceptance criterion is that beta-reduction succeeds under the LAMBDA-9 contract, regardless of representation.
- **Primitive `+`.** LAMBDA-3 pinned the choice (Expr variant or built-in Apply). If built-in: `ApplyExpr` must handle the primitive case — when `function` resolves to a primitive identifier, dispatch to the primitive impl instead of closure-body eval. Add this path in this ticket; cover with a dedicated unit test using `+`.

## What must NOT change

- LAMBDA-2 types.
- Other node-kind files.
- Plexus RPC wire format.

## Acceptance criteria

1. A file `plexus-lambda/src/expr/apply.rs` exists, containing exactly one `#[plexus_macros::activation(namespace = "expr")]` block for `ApplyExpr`.
2. `ApplyExpr` exposes: `eval`, `function()` static child, `arg(idx)` dynamic child with `list = "arg_positions"` opt-in, and `arg_positions()` list method.
3. Unit tests cover the four rows of the "Required observable" table. `cargo test -p plexus-lambda expr::apply` passes.
4. An in-process Plexus RPC test evaluates `(\x -> x) 42` via RPC (all child calls go through the hub) and asserts the result equals `Value::Literal(Lit::Int(42))`.
5. File is the sole LAMBDA-6-owned file. Disjoint from LAMBDA-3, 4, 5, 7, 8.
6. Integration gate: `cargo build -p plexus-lambda` and `cargo test -p plexus-lambda` pass green end-to-end.

## Completion

Implementor delivers a commit adding `plexus-lambda/src/expr/apply.rs` with the activation, child gates, eval method (including the primitive-`+` path if that's LAMBDA-3's choice), and tests. PR description includes test output. Ticket flips to Complete after LAMBDA-9 lands.
