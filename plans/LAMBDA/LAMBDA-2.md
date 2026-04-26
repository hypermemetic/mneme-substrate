---
id: LAMBDA-2
title: "Core types crate scaffold: Expr, Env, Value, ParseError, EvalError"
status: Pending
type: implementation
blocked_by: [LAMBDA-S01]
unlocks: [LAMBDA-3, LAMBDA-4, LAMBDA-5, LAMBDA-6, LAMBDA-7, LAMBDA-8, LAMBDA-9]
severity: High
target_repo: plexus-lambda
---

## Problem

The `plexus-lambda` crate does not yet exist. Every downstream ticket in this epic (parser, node activations, env handling, integration test, synapse wiring) needs a shared vocabulary of domain types — the AST enum, the runtime value enum, the environment shape, and the error types. Without this scaffold, LAMBDA-3 through LAMBDA-8 cannot fan out in parallel because each would re-invent the shared types and collide on merge.

This ticket creates the crate and its types. **No Plexus RPC integration in this ticket** — types only.

## Context

Target location: `/Users/shmendez/dev/controlflow/hypermemetic/plexus-lambda/` (new crate, workspace sibling).

The crate must be consumable by `plexus-core`, `plexus-macros`, and the future node-activation tickets without pulling in Plexus RPC dependencies itself. Keep it dependency-light: `serde` for the derives, `im` (or an equivalent persistent-HashMap crate already in the workspace) for `Env`, nothing else mandatory. The crate is pure Rust; node activations live in sibling modules / files that add the Plexus dependency in their own tickets.

Domain-type sketch (specifies behaviour, not code):

| Type | Kind | Purpose |
|---|---|---|
| `Expr` | enum | AST node. Variants: `Var(VarName)`, `Lambda { param: VarName, body: Box<Expr> }`, `Apply { function: Box<Expr>, args: Vec<Expr> }`, `Let { name: VarName, value: Box<Expr>, body: Box<Expr> }`, `Literal(Lit)`. |
| `VarName` | newtype wrapping `String` | Identifier. `#[serde(transparent)]`. |
| `Lit` | enum | Literal values. For the initial epic scope: `Lit::Int(i64)` only. |
| `Value` | enum | Runtime values. Variants: `Value::Literal(Lit)`, `Value::Closure { param: VarName, body: Box<Expr>, captured: Env }`. |
| `Env` | newtype / alias | `Arc<im::HashMap<VarName, Value>>` — cheap to clone, persistent. |
| `ParseError` | enum | Error type for the parser activation (LAMBDA-3 populates variants). Initial variants: `UnexpectedEof`, `UnexpectedToken { position: usize, found: String }`. |
| `EvalError` | enum | Error type for node evaluations. Initial variants: `UnboundVariable(VarName)`, `ApplyNonFunction(Value)`, `ArityMismatch { expected: usize, found: usize }`. |

All types derive `Debug`, `Clone`, `Serialize`, `Deserialize`. `VarName`, `Lit`, `Env` additionally derive `PartialEq`, `Eq`, `Hash` where semantically valid (Env's hashing is structural via `im::HashMap`'s own impl). Follow the strong-typing skill's conventions for the newtypes.

**LAMBDA-S01 spike result (populated by the spike before this ticket is promoted to Ready):**

> _To be filled in by the LAMBDA-S01 implementor. PASS/FAIL, any constraints the spike surfaced that refine the types above (e.g., "closures cannot be serialized over the wire; must be handle-based")._

Downstream consumer contracts:

| Consumer | Reads |
|---|---|
| LAMBDA-3 (parser) | `Expr` (constructs), `ParseError` (returns) |
| LAMBDA-4 (Var activation) | `Expr::Var`, `Env`, `Value`, `EvalError` |
| LAMBDA-5 (Lambda activation) | `Expr::Lambda`, `Value::Closure`, `Env` |
| LAMBDA-6 (Apply activation) | `Expr::Apply`, `Value`, `EvalError::ApplyNonFunction`, `EvalError::ArityMismatch` |
| LAMBDA-7 (Literal activation) | `Expr::Literal`, `Value::Literal` |
| LAMBDA-8 (Let activation) | `Expr::Let`, `Env` |
| LAMBDA-9 (env handling) | `Env`, `Value::Closure` — decides the wire representation |
| LAMBDA-10 (integration) | all of the above |

## Required behavior

| Check | Expected |
|---|---|
| Build the crate | `cargo build -p plexus-lambda` green |
| Unit test: construct `Expr::Literal(Lit::Int(42))` and round-trip via serde JSON | Deserialized value equals the original |
| Unit test: construct `Expr::Lambda { param: "x".into(), body: box Expr::Var("x".into()) }` and round-trip via serde JSON | Deserialized value equals the original |
| Unit test: construct an `Env` and insert/lookup a `VarName`-keyed binding | Returned `Value` equals the inserted value |
| Unit test: `EvalError::UnboundVariable(VarName)` displays via `Debug` containing the var name | `Debug` output contains the variable's string form |

No Plexus macro usage in this crate at this ticket's landing. No `#[plexus_macros::activation]`, `#[plexus_macros::method]`, or `#[plexus_macros::child]` in the source. Those come with LAMBDA-3..8.

## Risks

- **`im` version collision with other workspace crates.** If the workspace already pins `im` at a specific version, match it. If not, pick the latest stable `im` v15.x. If a collision surfaces, spike to decide: (a) switch to `rpds` or another persistent-map crate, or (b) re-pin across the workspace. Not a LAMBDA-2-blocker unless it breaks the build.
- **Closure serialization.** `Value::Closure` carries a `Box<Expr>` and an `Env`. Naive serde-derive will serialize them — LAMBDA-9 later decides whether closures are allowed to cross Plexus RPC wires at all, or whether they must be handle-based. This ticket derives `Serialize`/`Deserialize` on `Value` to unblock downstream work; LAMBDA-9 may add wire-representation wrappers without rewriting `Value` itself.
- **`Apply` arity.** The epic pin says lambda calculus, where `Apply` is strictly binary (function + one argument). Supporting `args: Vec<Expr>` upfront preserves the option for multi-arg application without re-shaping the AST. If the spike or LAMBDA-6 proves multi-arg is out of scope, the vec collapses to a single `Box<Expr>` with a minor downstream edit.

## What must NOT change

- No modifications to `plexus-core`, `plexus-macros`, `plexus-substrate`, or any other sibling crate. This ticket creates `plexus-lambda` in isolation.
- Workspace `Cargo.toml` may gain a new member entry for `plexus-lambda` if the workspace is structured to require it; otherwise the crate is standalone and discovered via the `.cargo/config.toml` patch list.

## Acceptance criteria

1. `/Users/shmendez/dev/controlflow/hypermemetic/plexus-lambda/Cargo.toml` exists and declares the crate at version 0.1.0.
2. `cargo build -p plexus-lambda` succeeds from the workspace root.
3. `cargo test -p plexus-lambda` succeeds. At minimum the four unit tests from the "Required behavior" table are present and pass.
4. The `Expr`, `Value`, `Env`, `VarName`, `Lit`, `ParseError`, `EvalError` types are public from the crate root (or a well-named module re-exported at the root).
5. No source file in `plexus-lambda` imports from `plexus_macros` or `plexus_core` at this ticket's completion. `grep -r 'plexus_macros' plexus-lambda/src` returns zero matches; same for `plexus_core`.
6. `plans/README.md` has a one-line entry under "Shipped / In flight" indicating `plexus-lambda` scaffolding has landed. (This is the coordination-doc update promised in LAMBDA-1.)
7. LAMBDA-S01's PASS/FAIL result is present in this ticket's Context section before the ticket is flipped from Pending to Ready.
8. Integration gate: `cargo build` and `cargo test` across every workspace this commit touches are green end-to-end.

## Completion

Implementor delivers:

- A commit that creates `plexus-lambda/` with the types, unit tests, and `Cargo.toml`.
- Updated `plans/README.md` entry.
- PR description includes `cargo build -p plexus-lambda` and `cargo test -p plexus-lambda` output.
- Ticket status flipped Ready → Complete in the same commit.
- LAMBDA-3 through LAMBDA-9 are unblocked; the implementor calls this out in the PR description.
