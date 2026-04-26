---
id: LAMBDA-7
title: "Literal node activation: eval returns wrapped literal value"
status: Pending
type: implementation
blocked_by: [LAMBDA-2]
unlocks: [LAMBDA-10]
severity: High
target_repo: plexus-lambda
---

## Problem

`plexus-lambda` needs a `LiteralExpr` activation: the AST leaf for integer literals like `42`, `-7`. Its eval is an identity — it returns its own literal value, ignoring the environment. Trivially simple, but required for the activation-graph pattern to be complete.

## Context

Target crate: `plexus-lambda` at `/Users/shmendez/dev/controlflow/hypermemetic/plexus-lambda/`.

Depends on LAMBDA-2 (`Expr::Literal`, `Lit`, `Value`).

Not blocked by LAMBDA-9 — literals do not interact with the env and their eval does not return a closure. They are the simplest node kind and can be built immediately after LAMBDA-2.

A `LiteralExpr` activation wraps a single `Lit` value. It has no children. It exposes one RPC method `eval(env)` that returns the literal as a `Value::Literal(lit)`.

## Required behavior

| Input | Operation | Expected observable |
|---|---|---|
| `LiteralExpr { value: Lit::Int(42) }`, arbitrary env | `eval(env)` | Returns `Value::Literal(Lit::Int(42))` |
| `LiteralExpr { value: Lit::Int(-7) }`, empty env | `eval(env)` | Returns `Value::Literal(Lit::Int(-7))` |
| Called via the Plexus hub (not direct Rust call) | `eval(env)` | Same result; call routes through `get_child` / method dispatch. |

The activation is declared with `#[plexus_macros::activation(namespace = "expr")]` (co-resident with the other Expr node kinds).

## Risks

- **None material.** Literals are the degenerate case.

## What must NOT change

- LAMBDA-2 types.
- Other node-kind files.

## Acceptance criteria

1. A file `plexus-lambda/src/expr/literal.rs` exists, containing exactly one `#[plexus_macros::activation(namespace = "expr")]` block for `LiteralExpr`.
2. `LiteralExpr` exposes exactly one RPC method: `eval`.
3. No `#[plexus_macros::child]` attributes — leaf node.
4. Unit tests cover the three rows of the "Required behavior" table. `cargo test -p plexus-lambda expr::literal` passes.
5. File is the sole LAMBDA-7-owned file. Disjoint from LAMBDA-3, 4, 5, 6, 8.
6. Integration gate: `cargo build -p plexus-lambda` and `cargo test -p plexus-lambda` pass green end-to-end.

## Completion

Implementor delivers a commit adding `plexus-lambda/src/expr/literal.rs` with the activation and tests. PR description includes test output. Ticket flips Ready → Complete in the same commit.
