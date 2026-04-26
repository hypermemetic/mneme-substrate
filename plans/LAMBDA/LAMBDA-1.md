---
id: LAMBDA-1
title: "plexus-lambda — AST-as-activation-graph interpreter service"
status: Pending
type: epic
blocked_by: []
unlocks: [LAMBDA-S01, LAMBDA-2, LAMBDA-3, LAMBDA-4, LAMBDA-5, LAMBDA-6, LAMBDA-7, LAMBDA-8, LAMBDA-9, LAMBDA-10, LAMBDA-11]
target_repo: plexus-lambda
---

## Goal

End state: a new Plexus RPC service ships in its own crate, `plexus-lambda`, living at `/Users/shmendez/dev/controlflow/hypermemetic/plexus-lambda/` as a workspace sibling to `plexus-core`, `plexus-substrate`, `plexus-macros`, and friends. The service implements a minimal untyped lambda-calculus interpreter whose AST is represented directly as a tree of Plexus RPC activations registered on a hub. Parsing a program returns a handle to the root expression; each AST node is an activation exposing typed methods (`eval`, `inspect`); evaluating a program is a cascade of RPC calls recursing through the activation graph. Synapse connects to `plexus-lambda` and can drive parsing and evaluation interactively — the AST is as navigable as any other Plexus RPC tree.

## The idea (pin)

Plexus RPC natively supports graphs of activations via `#[plexus_macros::child]` (static, dynamic, and list-opt-in child gates). An AST is a graph. Therefore **a parsed program is a tree of activations on a Plexus hub**. Each AST node kind (Var, Lambda, Apply, Let, Literal) gets its own activation type with a typed RPC surface; its children (sub-expressions, binding name, body) are Plexus children. Evaluation is recursion via RPC; no hand-written "match on node kind" in caller code — polymorphism is carried by the activation registry.

Benefits that fall out of Plexus RPC's existing features:

| Plexus RPC feature | What it gives programs-as-graphs |
|---|---|
| Streaming methods | `eval()` emits progress / intermediate values for long computations |
| Child routing | `program.expr_42.operand_0.eval` addresses any subexpression |
| Schema hashes (IR epic) | Content-addressed memoization is free per node kind |
| Remote children | Distributed evaluation with zero extra plumbing |
| Typed methods | Each node kind has a typed RPC surface; no "match on node kind" in clients |
| Synapse CLI | `synapse lambda parse "(\\x -> x) 42"` → handle → `synapse lambda expr_<id> eval` |
| hub-codegen | Typed clients per AST-node kind for external tools |

**Prior art:** Unison (content-addressed code), Salsa (incremental compilation), effect handlers (typed recursion-with-effects), Dagster/Airflow (coarser-grained graph-of-computations). Plexus-as-program-host sits at the intersection: fine-grained like an AST, distributed + streaming like a service.

**Language scope (minimal, pinned here for downstream tickets):** untyped lambda calculus with five expression kinds — `Var(name)`, `Lambda(param, body)`, `Apply(function, arg)`, `Let(name, value, body)`, `Literal(i64)`. Literals are integers for the spike; no booleans, strings, or floats. No type-checking. No side effects.

## Context

Target crate: `plexus-lambda` (new) at `/Users/shmendez/dev/controlflow/hypermemetic/plexus-lambda/`. Workspace sibling to:

| Crate | Path | Role |
|---|---|---|
| `plexus-core` | `/Users/shmendez/dev/controlflow/hypermemetic/plexus-core` | `Activation`, `ChildRouter`, `DynamicHub` traits |
| `plexus-macros` | `/Users/shmendez/dev/controlflow/hypermemetic/plexus-macros` | `#[activation]`, `#[method]`, `#[child]` |
| `plexus-substrate` | `/Users/shmendez/dev/controlflow/hypermemetic/plexus-substrate` | Reference Plexus RPC server pattern |
| `plexus-synapse` | `/Users/shmendez/dev/controlflow/hypermemetic/synapse` | CLI used by LAMBDA-11 for integration verification |

`plexus-lambda` depends on the published (or patched) versions of `plexus-core` and `plexus-macros` the rest of the workspace already pins. See the substrate `.cargo/config.toml` patch block documented in `plans/README.md` — `plexus-lambda` must appear there (as a local patch entry or workspace member) for local builds.

## Dependency DAG

```
                LAMBDA-S01
             (spike: viability)
                    │
                    ▼
                LAMBDA-2
            (core types crate)
                    │
      ┌──────┬──────┼──────┬──────┬──────┐
      ▼      ▼      ▼      ▼      ▼      ▼
  LAMBDA-3 LAMBDA-4 LAMBDA-5 LAMBDA-6 LAMBDA-7 LAMBDA-8
  (parser) (Var)  (Lambda) (Apply) (Literal) (Let)
      │              │        │              │
      │              └────────┴──────────────┘
      │                        │
      │                        ▼
      │                   LAMBDA-9
      │             (environment handling)
      │                        │
      └────────────┬───────────┘
                   ▼
               LAMBDA-10
           (integration test)
                   │
                   ▼
               LAMBDA-11
           (synapse wiring)
```

LAMBDA-S01 is the binary-pass spike that gates the whole epic. If S01 fails, the pattern does not work as conceived and the epic is replanned or shelved; no downstream ticket should start.

Once LAMBDA-2 lands, LAMBDA-3 through LAMBDA-8 can fan out in parallel — each targets a different file / node kind (file-boundary concurrency per ticketing skill rule 10). LAMBDA-9 serializes with LAMBDA-5 / LAMBDA-6 / LAMBDA-8 because those tickets' implementations consume the pinned `Env` shape; the LAMBDA-9 decision must land before they integrate, even if they begin work in parallel against a placeholder.

## Phase Breakdown

| Phase | Tickets | Parallelism |
|---|---|---|
| 0. Viability | LAMBDA-S01 | Binary spike gates the epic. |
| 1. Scaffold | LAMBDA-2 | Single ticket; defines the shared types. |
| 2. Node activations | LAMBDA-3, LAMBDA-4, LAMBDA-5, LAMBDA-6, LAMBDA-7, LAMBDA-8 | Parallel — one file per node kind. |
| 3. Environment | LAMBDA-9 | Pins Env shape; consumed by 5/6/8. |
| 4. Integration | LAMBDA-10 | End-to-end eval via RPC. |
| 5. CLI | LAMBDA-11 | Synapse exposes the tree. |

## Tickets

| ID | Summary | Target repo | Status |
|---|---|---|---|
| LAMBDA-1 | This epic overview | — | Epic |
| LAMBDA-S01 | Spike: AST-as-activation viability (parse + eval `(\x -> x) 42` via RPC) | plexus-lambda | Pending |
| LAMBDA-2 | Core types crate scaffold (Expr, Env, Value, ParseError, EvalError) | plexus-lambda | Pending |
| LAMBDA-3 | Parser activation (`parser.parse(source) -> Handle<RootExpr>`) | plexus-lambda | Pending |
| LAMBDA-4 | Var node activation (`eval(env) -> Value`) | plexus-lambda | Pending |
| LAMBDA-5 | Lambda node activation (param + body children, eval returns closure) | plexus-lambda | Pending |
| LAMBDA-6 | Apply node activation (function + indexed arg children, beta-reduction) | plexus-lambda | Pending |
| LAMBDA-7 | Literal node activation (`eval(_env) -> Value` identity) | plexus-lambda | Pending |
| LAMBDA-8 | Let node activation (binding + value + body, extended-env eval) | plexus-lambda | Pending |
| LAMBDA-9 | Environment handling (serde-on-wire vs handle-based EnvId, pinned) | plexus-lambda | Pending |
| LAMBDA-10 | Integration test: parse + eval `(\x -> x + 1) 41 == 42` via RPC | plexus-lambda | Pending |
| LAMBDA-11 | Synapse integration: `synapse lambda parse "…"` + per-node `eval` | plexus-lambda, synapse | Pending |

## Cross-epic references

- **CHILD epic** — this epic is the first heavy exercise of `#[plexus_macros::child]` on a non-plumbing domain. Static children (lambda's `param`/`body`), dynamic children (Apply's indexed args), and list opt-in (parser's registered-expr enumeration) all appear. If LAMBDA hits a CHILD-epic gap, file it back to CHILD as a follow-up rather than patching locally.
- **IR epic** — each node-kind activation emits its own role-tagged `MethodSchema`. Once HASH lands, schema-hash-based memoization of `eval` per `(node_kind, env_hash)` becomes a natural future extension. Out of scope for LAMBDA but noted for downstream planning.
- **RUSTGEN** — when it lands, external Rust consumers can generate typed clients for every `plexus-lambda` node kind exactly as they would for any other Plexus RPC service.
- **`plans/README.md` coordination doc** — `plexus-lambda` is a new workspace-sibling service. The "Pointers" and "Roadmap" / "Shipped / In flight" sections of `plans/README.md` must gain entries referencing this epic. That README update is a follow-on within LAMBDA-1's scope (commit alongside this epic's first landing), not a separate ticket.

## Out of scope

- **Typed lambda calculus / type-checking.** The `type_check` method mentioned in the idea-pin is a future concern; not in this epic. A separate epic can add it once the runtime pattern is validated.
- **Side effects / effect handlers.** No `IO`, no mutable refs, no exceptions.
- **Optimization / JIT / compilation.** Tree-walking interpreter only.
- **Richer literal types.** Integers only; no floats, strings, booleans. Extending the literal lattice is trivial once the pattern is in place — deferred.
- **Language integration beyond the interpreter.** No Python bindings, no LSP, no REPL, no language-server. The interpreter is reachable only via Plexus RPC (synapse CLI, or direct RPC client).
- **Multi-language dispatch.** Mixing Python activations and Rust activations on the same program graph is a provocative future direction but not this epic.
- **Non-trivial parsing.** The parser accepts the minimal surface `(\x -> x) 42`-class expressions. Operator precedence, comments, multi-char identifiers with underscores-and-dashes: accept what the minimal spec requires; anything else is out of scope.

## What must NOT change

- Nothing outside `plexus-lambda` and the `plans/README.md` entries noted above. Specifically: `plexus-core`, `plexus-macros`, `plexus-substrate`, and `synapse` source are **not** modified by this epic except for the synapse wiring in LAMBDA-11, which is purely configuration / runtime connection (no synapse source changes required).
- Existing Plexus RPC wire format is unchanged.
- The CHILD, IR, HASH, SYN, ST, STG, RL, OB epics and their in-flight tickets are untouched.

## Acceptance criteria

1. LAMBDA-S01 through LAMBDA-11 exist as ticket files in `plans/LAMBDA/` with status `Pending` and the DAG reflected in their `blocked_by` / `unlocks` fields.
2. `plans/README.md` is updated to list `plexus-lambda` under "Shipped / In flight" and to reference this epic in the "Pointers" section, committed in the same landing as the epic.
3. Each downstream ticket in this epic has acceptance criteria that include the integration gate per ticketing skill rule 12 (`cargo build` + `cargo test` green for the affected workspace).
4. The five AST node kinds Var, Lambda, Apply, Let, Literal are each tracked by exactly one implementation ticket (LAMBDA-4, LAMBDA-5, LAMBDA-6, LAMBDA-8, LAMBDA-7 respectively). No node kind is split across tickets; no ticket covers multiple node kinds.
5. LAMBDA-9's acceptance criteria pin the environment-propagation choice (wire-serde vs handle-based EnvId) unambiguously before LAMBDA-5 / LAMBDA-6 / LAMBDA-8 are promoted to `Ready`.

## Completion

Epic is Complete when LAMBDA-S01 through LAMBDA-11 are all Complete and a fresh clone can run:

```
cargo build -p plexus-lambda
cargo test -p plexus-lambda
```

both green, and `synapse lambda parse "(\\x -> x + 1) 41"` followed by an `eval` on the resulting handle returns `Literal(42)` in the user's terminal. The `plans/README.md` entries reference the completed epic with a Shipped badge.
