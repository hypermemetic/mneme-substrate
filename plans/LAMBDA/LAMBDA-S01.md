---
id: LAMBDA-S01
title: "Spike: AST-as-activation viability — parse + eval (\\x -> x) 42 via RPC"
status: Pending
type: spike
blocked_by: []
unlocks: [LAMBDA-2]
severity: Critical
target_repo: plexus-lambda
---

## Question

Can a parsed lambda-calculus AST be registered as a tree of Plexus RPC activations on a hub such that `root_handle.eval()` — called via real Plexus RPC, not a direct Rust call — recurses through the activation graph and returns the correct result for the program `(\x -> x) 42`?

Binary: end-to-end RPC-driven eval returns `Literal(42)` → **PASS**. Any other outcome (compile error, runtime panic, wrong value, eval that bypasses RPC boundaries) → **FAIL**.

## Setup

1. Create a throwaway `plexus-lambda/spike/S01/` directory (or a throwaway branch of a minimal crate) containing:
   - A minimum `Expr` enum with two variants sufficient for the target program: `Lambda { param: String, body: Box<Expr> }` and `Literal(i64)`. Apply is needed to even run the program; add `Apply { function: Box<Expr>, arg: Box<Expr> }` and `Var(String)` — four variants total for this spike.
   - Minimum Plexus RPC activation types, one per variant, using `#[plexus_macros::activation]`. Each exposes one method `eval` returning a `Value` (literal integer or closure sentinel).
   - A hard-coded "parser" that builds the AST for `(\x -> x) 42` in-memory — no string parsing in this spike. Registration of every resulting node on a test hub is the spike's load-bearing work.
   - A minimum in-process Plexus RPC test harness (following substrate's pattern — a `DynamicHub` driven via `plexus-core` test utilities).
2. Register the AST nodes on the hub. The root `Apply` activation must hold Plexus child references (via `#[plexus_macros::child]`) to its `function` (a `Lambda`) and `arg` (a `Literal`). The `Lambda` must hold a child reference to its `body` (a `Var`).
3. Invoke `eval` on the root handle **through the Plexus RPC dispatcher** — not by calling the Rust method directly on the struct. The call must route through `get_child` / `call_method` of the hub.

## Pass condition

Running the spike binary prints `Literal(42)` (or a structurally equivalent observable: the integer `42` tagged as a literal value). The call path is verifiable by logging: every `eval` invocation emits a log line, and the logs show the call descending through Apply → Lambda (eval-function) → Literal (eval-arg) → Lambda body eval (Var lookup resolving to the Literal value).

Binary: correct value AND visible RPC-mediated call trace → PASS.

## Fail → next

If the spike fails because the Plexus macro cannot express a child gate whose return type is a trait object or enum variant (e.g., "body can be any `Expr` kind"), open a follow-up: try an approach where every node kind is represented by a single activation type that internally discriminates by variant (loses the polymorphism benefit but preserves RPC-mediated recursion). File as LAMBDA-S02 if that becomes necessary.

## Fail → fallback

If both spike approaches fail, the AST-as-activation-graph pattern is not viable in the current Plexus macro surface. Document the constraint, close the epic, and open a separate epic in `plans/` proposing the macro extensions needed to make it viable. Do **not** silently narrow LAMBDA's scope to "one giant Expr activation" — that defeats the epic's thesis.

## Time budget

Four focused hours. If the spike exceeds this budget without a PASS, stop and report regardless. The budget overrun itself is signal and feeds the fallback decision.

## Out of scope

- String parsing. The AST is built by hand.
- Environment threading for closures. The target program `(\x -> x) 42` only needs the argument value visible to the body — the minimum env plumbing to make this work is in scope; anything richer is not.
- Error handling beyond "panics fail the spike". Proper `EvalError` is LAMBDA-2's concern.
- Performance. One allocation per node is fine.
- Any work in the `plexus-lambda` production crate. This is throwaway code.

## What must NOT change

- `plexus-core`, `plexus-macros`, `plexus-substrate` source. The spike exists purely to verify the existing macro surface is sufficient.
- Any other epic's in-flight tickets.

## Acceptance criteria

1. The spike binary at `plexus-lambda/spike/S01/` (or equivalent throwaway location) builds: `cargo build` inside the spike directory is green.
2. The spike binary runs: `cargo run` inside the spike directory exits 0.
3. The binary's stdout contains the line `Literal(42)` (or the structurally equivalent observable pinned in the Setup section).
4. The binary's stdout contains at least four distinct `eval` trace lines in the expected recursion order (Apply, Lambda-param-eval-path, Literal-arg, Var-lookup-returns-literal). A grep for `eval` in the output yields >= 4 matches.
5. A one-paragraph PASS/FAIL report lands as a comment at the bottom of LAMBDA-2's ticket file (in its Context section, as a "Spike: LAMBDA-S01 result" block) before LAMBDA-2 is promoted from Pending to Ready.
6. `cargo build` and `cargo test` for any workspace the spike touched remain green end-to-end (integration gate per ticketing skill rule 12). Since the spike lives in a throwaway directory, this reduces to "the spike crate itself builds and runs".

## Completion

Implementor delivers:

- A single commit on a throwaway branch or directory containing the spike source and a README with the PASS/FAIL result.
- Report pasted into LAMBDA-2's Context section as described in acceptance criterion 5.
- Ticket status flipped from Ready → Complete (with PASS) or Ready → Superseded (with FAIL + replan note). No Complete-with-fail outcomes.
