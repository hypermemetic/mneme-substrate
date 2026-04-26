---
id: LAMBDA-11
title: "Synapse integration: drive plexus-lambda parse + eval from the CLI"
status: Pending
type: implementation
blocked_by: [LAMBDA-10]
unlocks: []
severity: Medium
target_repo: plexus-lambda
---

## Problem

`plexus-lambda` works end-to-end when driven by a Rust test harness (LAMBDA-10). It should also work when driven by the standard Plexus RPC CLI, synapse — that is the canonical interactive client and the way a human uses any Plexus RPC service. Without this ticket, the epic's "programs as activation trees are navigable" promise is unverified from the user's actual tool.

## Context

Target crate: `plexus-lambda` (binary entry point for a `plexus-lambda` server) and the synapse CLI at `/Users/shmendez/dev/controlflow/hypermemetic/synapse/`.

Depends on LAMBDA-10 (the integration test ensures the service itself works).

This ticket **does not modify synapse source**. Synapse already speaks Plexus RPC against any Plexus RPC server's schema. Exposing `plexus-lambda` to synapse requires:

1. A binary entry point in `plexus-lambda` that boots a hub, registers the parser activation (and whatever else the service needs at startup), and listens on a transport (the default substrate-style transport — matching what synapse expects).
2. A synapse configuration pointing at the running `plexus-lambda` service (a profile / endpoint / alias — whatever synapse's existing mechanism is for connecting to a new service).
3. Verification that synapse's introspection walks the tree correctly: namespace `parser` is visible, `parse` method is visible, a handle returned from `parse` can be navigated into and its typed methods (eval) called.

Tree-rendering expectations (per CHILD / IR epic semantics):

| Node visible in `synapse lambda` tree | Rendered as |
|---|---|
| `parser` activation | A namespace node with `parse` as a child method. |
| Post-`parse` root handle | A dynamic child node reflecting the root AST-node kind (e.g., `ApplyExpr`). |
| `function()` static child gate | A static child with its own typed methods. |
| `arg(idx)` dynamic child gate with `list = "arg_positions"` | A child-gate node that synapse can list via `list_children` to see `arg_0`, `arg_1`, etc. |
| Leaf methods (`eval`) | Method nodes with signature rendered. |

## Required behavior

| User action | Expected observable |
|---|---|
| Start `plexus-lambda` server (command TBD — likely `cargo run -p plexus-lambda` or a named binary) | Server boots, listens on a transport, prints a connection string to stdout. |
| `synapse lambda` (or the configured alias) | Synapse connects, displays the `parser` namespace with a `parse` method. |
| `synapse lambda parser parse '{"source": "(\\x -> x + 1) 41"}'` | Returns a handle (or handle descriptor) that synapse prints. |
| `synapse lambda <handle> eval '{"env": {}}'` | Returns `Value::Literal(Lit::Int(42))` in synapse's output. |
| `synapse lambda <handle> function body` (navigating children) | Synapse renders the body sub-tree with its methods callable. |

No synapse source changes. If synapse cannot express any of the above against `plexus-lambda`'s schema, open a ticket in `plans/SYN/` — do NOT patch synapse locally as part of this epic.

## Risks

- **Transport defaults.** `plexus-lambda`'s binary must use the transport synapse expects by default. Follow substrate's transport setup verbatim.
- **Handle serialization in CLI args.** Handles on synapse's argv are typically JSON-ish strings. The representation is synapse-pinned; LAMBDA-9's env wire shape must be compatible with synapse's JSON-based RPC args encoding. If LAMBDA-9's decision works for synapse as-is, no action. If not, file a cross-epic ticket.
- **Tree rendering of dynamic children.** Synapse renders children according to their `ChildCapabilities`. If `arg_positions` (the list opt-in) is not enabled properly, synapse won't auto-list args. Verify in this ticket.

## What must NOT change

- Synapse source code.
- `plexus-lambda`'s node-activation surfaces (all pinned by LAMBDA-3..8).
- Plexus RPC wire format.

## Acceptance criteria

1. A binary entry point in `plexus-lambda` (e.g., `plexus-lambda/src/bin/server.rs` or a `[[bin]]` target in `Cargo.toml`) exists, starts the hub, registers the parser activation, and listens on the default transport. `cargo run -p plexus-lambda --bin <name>` prints a connection string and stays alive.
2. A short README or ticket-completion-note documents the exact commands to run the server and connect synapse — the two-stranger-test criterion: a fresh person can follow the instructions and replicate.
3. Running `synapse` against the live `plexus-lambda` server and calling `parser parse` with the test source `(\\x -> x + 1) 41` returns a handle; calling `eval` on that handle with an empty env returns `Value::Literal(Lit::Int(42))`. The synapse transcript is pasted into the PR description.
4. Synapse's tree rendering for `plexus-lambda` shows the parser namespace, the `parse` method, and — after a `parse` call — the root Expr activation with its child gates. Transcript pasted in PR.
5. No synapse source is modified. `git status` in the synapse repo shows no changes across the duration of this ticket's implementation.
6. `plans/README.md` roadmap entry for LAMBDA is updated to reflect Shipped status for the full epic.
7. Integration gate: `cargo build -p plexus-lambda` and `cargo test -p plexus-lambda` pass green end-to-end.

## Completion

Implementor delivers a commit adding the `plexus-lambda` server binary, the README / connection notes, and a transcript of the synapse session pasted into the PR. Ticket flips Ready → Complete. LAMBDA epic closes (LAMBDA-1's completion criterion satisfied).
