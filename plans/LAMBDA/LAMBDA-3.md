---
id: LAMBDA-3
title: "Parser activation: parse source → register Expr tree on hub → return root handle"
status: Pending
type: implementation
blocked_by: [LAMBDA-2]
unlocks: [LAMBDA-10]
severity: High
target_repo: plexus-lambda
---

## Problem

`plexus-lambda` needs an entry point: a Plexus RPC activation that takes a lambda-calculus source string, parses it to an `Expr` tree, registers every node as a Plexus activation on the hub, and returns a handle to the root. Without this, there is no way to drive evaluation from the outside — synapse, external clients, and the integration test all need a single call to bootstrap a program.

## Context

Target crate: `plexus-lambda` at `/Users/shmendez/dev/controlflow/hypermemetic/plexus-lambda/`.

Depends on LAMBDA-2's `Expr`, `VarName`, `Lit`, `ParseError` types.

The parser activation is a **Plexus RPC activation** using `#[plexus_macros::activation(namespace = "parser")]`. It exposes one method: a parse call that takes a source string and returns a handle to the root AST node. The parser does not evaluate — it only constructs and registers.

Minimal language surface this parser must accept (aligned with the epic's language scope):

| Expression | Example |
|---|---|
| Variable | `x` |
| Lambda | `\x -> <body>` |
| Application (left-associative, space-delimited) | `f x`, `f x y`, `(\x -> x) 42` |
| Let binding | `let x = 1 in x` |
| Integer literal | `42`, `-7` |
| Parenthesized expression | `(expr)` |
| Binary integer add (for LAMBDA-10's test program `(\x -> x + 1) 41`) | `x + 1` |

The epic's language scope pins "no type-checking, no side effects". `+` is the only operator required; treat it as syntactic sugar desugaring to a built-in primitive application, OR add it as a distinct `Expr::Add(Box<Expr>, Box<Expr>)` variant (in which case LAMBDA-2 gets a follow-up edit to add the variant and a sibling node-activation ticket may be needed). **Decision for this ticket:** add `Add` as a binary primitive — either as a distinct `Expr` variant (edit LAMBDA-2's enum) or desugared to an `Apply` with a reserved `+` `VarName` that a built-in provides. Pin the choice in the implementor's commit; the integration test in LAMBDA-10 only requires one working form.

Child-registration contract:

| Parent Expr | Children registered on hub |
|---|---|
| `Expr::Lambda { param, body }` | The `body` is registered as a child activation; `param` becomes the activation's `param` field (static child of namespace `binding`). |
| `Expr::Apply { function, args }` | `function` and each `args[i]` are registered as children. |
| `Expr::Let { name, value, body }` | `value` and `body` are registered as children. |
| `Expr::Var` / `Expr::Literal` | Leaf; no children to register. Activation is still registered as itself. |

Every registered node gets a unique handle on the hub. The parser returns the root's handle. The handle carries sufficient routing information for synapse and other clients to address any sub-expression via child paths (e.g., `<root>/function/body/...`).

## Required behavior

| Input | Operation | Expected observable |
|---|---|---|
| Source `"42"` | parse | Returns a handle addressing a `LiteralExpr` activation; calling `eval` on it via RPC yields `Value::Literal(Lit::Int(42))` once LAMBDA-7 lands. |
| Source `"\\x -> x"` | parse | Returns a handle to a `LambdaExpr` with `param = VarName("x")` and a body-child accessible via the `body` static child gate. |
| Source `"(\\x -> x) 42"` | parse | Returns a handle to an `ApplyExpr`. Its `function` child is a `LambdaExpr`; its `arg` child at index 0 is a `LiteralExpr(42)`. |
| Source `"let x = 1 in x"` | parse | Returns a handle to a `LetExpr` with children `value` (Literal) and `body` (Var). |
| Source `"((("` (malformed) | parse | Returns a `ParseError` variant surfaced via the activation's method return. No panic. |
| Source `""` | parse | Returns `ParseError::UnexpectedEof`. |

The parser activation registers on the hub under namespace `"parser"`. It is the sole entry point to the `plexus-lambda` service; no other activation is callable externally before parse has run.

## Risks

- **Grammar ambiguity with `+`.** Left-associative application vs. binary-operator precedence can produce different trees for `f + g x`. The epic scope only needs `x + 1`, which is unambiguous. Document the restricted grammar in a rustdoc comment on the parser activation; anything more ambitious is out of scope for this ticket.
- **Handle stability.** Each parse call re-registers the AST; handles from a previous parse are invalidated when the parser is re-invoked or the hub is cleared. The contract is: handles are valid for the lifetime of the hub the parser ran against. Document this; no GC in this epic.
- **Hub reference.** The parser activation must have a reference to the hub to register children. The mechanism (constructor injection, thread-local, parent-context) is a macro-usage choice; follow the substrate Solar activation pattern (`plugin-development-guide` in the architecture docs).

## What must NOT change

- LAMBDA-2's `Expr`, `ParseError`, `VarName`, `Lit` types — except the pinned `Add` decision above, which may add a variant to `Expr` in a small, documented edit.
- Plexus RPC wire format.
- Other `plexus-lambda` files (LAMBDA-4..8 own their respective node-activation files).

## Acceptance criteria

1. A file `plexus-lambda/src/parser.rs` (or equivalent single file) exists and contains exactly one `#[plexus_macros::activation(namespace = "parser")]` block.
2. The parser activation exposes a `parse` method taking a `String` and returning a result carrying either a root handle or a `ParseError`.
3. Unit tests cover the six rows of the "Required behavior" table. `cargo test -p plexus-lambda parser` passes.
4. An in-process integration test parses `"(\\x -> x) 42"` and asserts the resulting hub contains at least three activations: an `ApplyExpr`, a `LambdaExpr`, and a `LiteralExpr`. The test enumerates registered activations via `list_children` or an equivalent introspection route.
5. The file is the only LAMBDA-3-owned file; LAMBDA-4..8 tickets own their own files. No write overlap per ticketing skill rule 10.
6. Integration gate: `cargo build -p plexus-lambda` and `cargo test -p plexus-lambda` pass green end-to-end.

## Completion

Implementor delivers a commit that adds `plexus-lambda/src/parser.rs`, wires it into the crate's module tree, adds the unit + integration tests, and flips this ticket to Complete. PR description includes the test output. LAMBDA-10 (integration test) is unblocked once this and LAMBDA-4..9 are Complete.
