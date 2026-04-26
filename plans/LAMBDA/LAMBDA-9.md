---
id: LAMBDA-9
title: "Environment handling: pin Env wire representation across Plexus RPC"
status: Pending
type: implementation
blocked_by: [LAMBDA-2]
unlocks: [LAMBDA-4, LAMBDA-5, LAMBDA-6, LAMBDA-8, LAMBDA-10]
severity: Critical
target_repo: plexus-lambda
---

## Problem

Every AST-node `eval` method takes an `Env` and sometimes returns a `Value::Closure` that captures an `Env`. Envs and closures cross Plexus RPC boundaries whenever an `eval` call is made via the hub. The `Env` shape pinned by LAMBDA-2 is `Arc<im::HashMap<VarName, Value>>` — cheap to clone in-process, but its wire representation is an open decision:

| Option | Shape on wire | Pros | Cons |
|---|---|---|---|
| (a) serde round-trip | Env and Value::Closure are serialized as JSON / CBOR / msgpack | No extra hub state; self-contained | Closures with non-primitive captured envs balloon the payload; nested closures can cycle; `Box<Expr>` body must also round-trip; schema hashing must account for this. |
| (b) handle-based | Envs live on the hub keyed by `EnvId`; the wire carries `EnvId`; `Value::Closure` carries a `BodyHandle` and an `EnvId` | Small payloads; closures stay on hub | Hub-side lifetime management; GC or TTL; server becomes stateful about envs; integration tests must bootstrap env-lifecycle machinery. |
| (c) hybrid | Primitives go serde; closures become handles | Best of both | Most complex; two code paths for `Value`. |

LAMBDA-4, LAMBDA-5, LAMBDA-6, LAMBDA-8 all block on this decision. This ticket makes the call and pins it.

## Context

Target crate: `plexus-lambda` at `/Users/shmendez/dev/controlflow/hypermemetic/plexus-lambda/`.

Depends on LAMBDA-2's `Env`, `Value`, `VarName`, `Lit`.

**Decision pinned here (this ticket commits to one option):** Option (a) — **serde round-trip for the epic's minimal scope**. Rationale:

- The epic's language scope is unary lambda calculus with integer literals only. Closures exist but capture minimal envs in practice for test programs.
- Option (a) keeps `plexus-lambda` stateless beyond the parser's registered-AST hub, which aligns with Plexus RPC's default stateless-service assumption and keeps LAMBDA-10's integration test simple.
- Option (b) is the right answer at scale (content-addressed envs, shared closures) but is premature — it introduces GC and lifetime concerns before the pattern is proven.
- Hybrid (c) is strictly harder than either and solves a problem (giant payloads) the epic doesn't have.

Revisit when: any LAMBDA-10 test program produces an Env serialization > 10 KB, or a closure capture chain exceeds 3 levels, or a HASH-epic-era content-addressed-memoization ticket lands and needs handle-based envs. Document the revisit triggers in a rustdoc comment on the `Env` wire wrapper type.

Wire contract:

| Type | Wire shape |
|---|---|
| `Env` (alias `Arc<im::HashMap<VarName, Value>>`) | Serde-serialized as a JSON object `{ "<varname>": <Value>, ... }` — ordered alphabetically for stability. |
| `Value::Literal(Lit::Int(n))` | `{ "kind": "literal", "lit": { "kind": "int", "value": n } }` |
| `Value::Closure { param, body, captured }` | `{ "kind": "closure", "param": "<name>", "body": <Expr-serde>, "captured": <Env-serde> }` |
| `VarName(String)` | `#[serde(transparent)]` → bare JSON string. |

`Expr` round-trips via its own serde derive (already present from LAMBDA-2). Closure bodies serialize by value (`Box<Expr>` inline), not by handle.

## Required behavior

| Input | Operation | Expected observable |
|---|---|---|
| Empty env | serde_json round-trip | Deserialized Env is empty and equal to the original. |
| Env `{x -> Literal(Int(42))}` | serde_json round-trip | Deserialized Env equals the original. |
| `Value::Closure { param: "x", body: Box::new(Expr::Var(VarName("x".into()))), captured: empty_env }` | serde_json round-trip | Deserialized closure equals the original (`PartialEq` on Value must be derived; trivially equal if Expr PartialEq is derived). |
| Env `{x -> Closure(...) }` (closure-valued binding) | serde_json round-trip | Full round-trip fidelity — outer env, bound closure, closure's captured inner env. |
| Env serialized wire payload | byte-sized sanity check | Single-binding int-only env payload is under 200 bytes. |

This ticket also introduces a wrapper type or trait-impl path that ensures every `#[plexus_macros::method]` method taking `Env` or returning `Value` uses this serialization shape. A minimal verification: a round-trip test invoked via a real in-process Plexus RPC test harness shows the env arriving on the callee side equal to the env sent on the caller side.

## Risks

- **Cyclic closures.** A closure that captures itself directly produces an infinite serialization. Mitigation: serde already fails on cycles (no `Rc`-via-Serialize in scope); document that closures are not permitted to be cyclic in the minimal language scope. Detection is out of scope.
- **`im::HashMap` serde.** Confirm `im`'s serde feature is enabled. If not, either enable it or wrap with a manual serde path that goes via `Vec<(VarName, Value)>` sorted alphabetically. Acceptance criterion 6 covers this.
- **`Value` `PartialEq`.** Required for the round-trip assertion. If `Value::Closure` contains a `Box<Expr>`, `PartialEq` on `Expr` must be derived (it is, per LAMBDA-2). If env equality is tricky because `Arc<HashMap>` comparison is by content, use `im::HashMap`'s `PartialEq`. Verify and document.

## What must NOT change

- LAMBDA-2's enum shapes. This ticket adds derives (if missing) and wrapper types, not enum variants.
- Plexus RPC wire format at the protocol level. This ticket defines `plexus-lambda`'s serialization of its domain types; the Plexus RPC envelope itself is unchanged.
- Other node-kind activation files. This ticket pins the contract; LAMBDA-4/5/6/8 consume it in their own commits.

## Acceptance criteria

1. A new file `plexus-lambda/src/env.rs` (or equivalent module) contains the env / closure wire-shape documentation, any wrapper types, and the five round-trip tests from the "Required behavior" table.
2. `cargo test -p plexus-lambda env` passes.
3. `im` crate is confirmed to expose `Serialize` / `Deserialize` on `HashMap` (via feature flag if needed); `Cargo.toml` records the feature.
4. `Value`, `Env`, `Expr` all derive `Serialize`, `Deserialize`, and — where semantically valid — `PartialEq`.
5. A rustdoc comment on the `Env` wrapper type records the revisit-triggers list from the Context section (payload size, chain depth, HASH-epic alignment).
6. The ticket's top block pins the chosen option (option a) and the pinning is unambiguous to a downstream implementor reading only this ticket. No open decision.
7. The `plans/README.md` coordination doc is updated with a new entry under "Pinned cross-epic contracts" titled "LAMBDA env wire shape" summarising the decision in one sentence.
8. Integration gate: `cargo build -p plexus-lambda` and `cargo test -p plexus-lambda` pass green end-to-end.

## Completion

Implementor delivers a commit that lands the env-module wire contract, the round-trip tests, the README.md coordination-doc update, and flips this ticket to Complete. PR description notes the decision and unblocks LAMBDA-4 / LAMBDA-5 / LAMBDA-6 / LAMBDA-8.
