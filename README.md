# mneme-substrate

> A Plexus RPC server forked from [`plexus-substrate`](https://github.com/hypermemetic/plexus-substrate), evolving toward the architecture in [`hypermemetic/mneme`](https://github.com/hypermemetic/mneme).

This repository is the **implementation** repo for the mneme harness. The planning, tickets, and BLF paper live in the sibling [`mneme/`](https://github.com/hypermemetic/mneme) repo. This repo is where the code happens.

## Where this stands

This is a Phase 0 fork. The contents are still ~99% upstream `plexus-substrate`. The package and crate name have been renamed (`plexus-substrate` → `mneme-substrate`) and the build is verified green. **No behavior change in the Phase 0 commit.** Subsequent phases will add the mneme-specific machinery on top.

## What it inherits from upstream

The full upstream substrate is here unchanged in behavior, including:

| Activation | Purpose |
|---|---|
| **claudecode** | Claude Code CLI session wrapper. Spawns and manages sessions. The Layer 0 primitive mneme builds on. |
| **claudecode_loopback** | Tool-use approval routing. Claude sessions request permission; routed through the approval API. |
| **arbor** | Conversation tree storage. Backs agent session history. |
| **bash** | Shell command execution. |
| **mustache** | Template rendering. |
| **changelog** | API hash tracking. |
| **orcha**, **lattice** | Multi-agent orchestration / DAG execution (predecessor to mneme's swarm primitives). |

Plus the Plexus RPC core, transport (WebSocket + MCP on port 4444), and the synapse CLI integration.

## What mneme will add

Per the tickets in [`mneme/plans/MNEME/`](https://github.com/hypermemetic/mneme/tree/master/plans/MNEME):

- **Substrate runtime** — program lifecycle middleware (wraps dispatch), per-program tool registry (loopback enhancement), session attribution, schema enforcement on activation returns, calibration store
- **`swarm` activation** — `trial / aggregate / sequential / race` orchestration primitives, with logit-shrinkage / concat-evidence / majority / max-severity aggregations
- **`respond` protocol** — per-program structured-output coercion via the loopback MCP tool registry
- **Skill activations** — `forecast` (validation skill), then `ticketing`, `planning`, `security_review`, `strong_typing` ports

The architecture is "single Rust binary" — Plexus is the **boundary protocol** for outside callers (synapse, MCP, WS); inside the binary, modules compose via normal Rust function calls. See `mneme/README.md` for the architecture diagram and the BLF (Bayesian Linguistic Forecaster) framework that informs the design.

## Quickstart (inherited)

```bash
# Start
mneme-substrate    # was: plexus-substrate; binary renamed in Phase 0

# Explore available methods
LANG=C.UTF-8 synapse mneme

# Or talk to the substrate's existing orcha activation
LANG=C.UTF-8 synapse mneme orcha run_tickets_files \
  --ticket_files '["plans/TDD/TDD-1.md"]' \
  --model sonnet \
  --working_directory /workspace/hypermemetic/mneme-substrate
```

## Companion repos

| Repo | Role |
|------|------|
| [`mneme`](https://github.com/hypermemetic/mneme) | Planning, tickets (18), ISSUES log, BLF paper + figures |
| [`mneme-substrate`](https://github.com/hypermemetic/mneme-substrate) (this repo) | Implementation forked from plexus-substrate |
| `plexus-substrate` (upstream, unrelated org) | Generic Plexus RPC server; mneme-substrate diverges |

## License

AGPL-3.0-only (inherited from upstream substrate; see `Cargo.toml`).
