# mneme-substrate

The Plexus RPC server implementing the [Bayesian Linguistic Forecaster (BLF)](https://arxiv.org/abs/2604.18576) algorithm.

For the concept, paper-faithfulness story, benchmark results, and design rationale: see the [`mneme`](https://github.com/hypermemetic/mneme) companion repo.

This README is the **operator's guide** — how to run it, how to fire forecasts, how to interpret what comes back.

## Quick start (Claude subscription, no API key)

You need:
- macOS or Linux
- Docker Desktop / Podman / Colima (any Docker-API-compatible runtime)
- A Claude subscription with the `claude` CLI installed (the run script will trigger `claude /login` if you don't have an OAuth token yet)

```bash
git clone git@github.com:hypermemetic/mneme-substrate.git
cd mneme-substrate

make install         # symlink the `mneme` CLI into ~/.local/bin
make build           # build the container (~3 min cold; ~30s incremental thanks to BuildKit cache mounts)
make run             # start substrate detached, drop into the container shell
```

That's it. `make run` does:

1. Resolves your auth token (priority: `ANTHROPIC_API_KEY` env → `CLAUDE_CODE_OAUTH_TOKEN` env → macOS Keychain entry `"Claude Code-credentials"`). If none of those have a token, it runs `claude /login` automatically.
2. Starts the substrate in the container, bound to `ws://localhost:4456`, with `programs/` and `.plexus-state/` bind-mounted to host disk so state survives restarts.
3. `docker exec`s you into the container with the `mneme` CLI on PATH and a friendly MOTD.

`exit` returns you to your host shell. The substrate keeps running until `make down`.

## Fire your first forecast

From inside the container shell (after `make run`):

```bash
mneme forecast "Will Bitcoin trade above \$200,000 on any day before 2026-12-31?"
```

That blocks until completion (~2-5 min) and prints the structured belief: probability, calibrated + raw, evidence_for, evidence_against, open_questions.

From the **host** (after `make install`, anywhere with the substrate running):

```bash
mneme forecast "Will Bitcoin trade above \$200,000 on any day before 2026-12-31?" --trials 3 --steps 5
mneme last              # show the most recent program's belief
mneme status <id>       # check program status
mneme wait <id>         # block until done + print
mneme resolve <id> no   # record outcome; calibration store grows
```

If you'd rather use synapse directly (bypassing the wrapper):

```bash
synapse -P 4456 substrate forecast update \
  --program-id MY-Q-001 \
  --new-evidence "Will Bitcoin trade above \$200K by 2026-12-31?" \
  --trials 3 \
  --iterative-max-steps 5

# Returns immediately with Started { program_id }. The work is fire-and-return.
# To see the result, either re-run `mneme wait <program_id>`, or watch `programs/<program_id>/`.
```

The artifact has:
- `probability` — calibrated point estimate
- `raw_probability` — pre-Platt aggregate (so consumers can recalibrate against a later-fit Platt model)
- `evidence_for` / `evidence_against` — arrays of `{claim, source, weight}`
- `open_questions` — what would tighten the answer
- `summary` — deterministic prose render
- `n_trials` / `confidence` / `belief_schema_version`

## Resolve it later

```bash
mneme resolve <program_id> yes      # or `no`
# equivalent synapse:
#   synapse -P 4456 substrate forecast resolve --program-id <id> --actual true
```

Once you have ≥10 resolved observations, Platt calibration kicks in automatically; subsequent forecasts have `probability != raw_probability` reflecting the bias correction.

## Run the benchmark

```bash
# Vendor a ForecastBench release (CC BY-SA 4.0)
mkdir -p programs/_benchmarks/forecastbench
curl -L -o programs/_benchmarks/forecastbench/2024-07-21-llm.json \
  https://raw.githubusercontent.com/forecastingresearch/forecastbench-datasets/main/datasets/question_sets/2024-07-21-llm.json
curl -L -o programs/_benchmarks/forecastbench/2024-07-21_resolution_set.json \
  https://raw.githubusercontent.com/forecastingresearch/forecastbench-datasets/main/datasets/resolution_sets/2024-07-21_resolution_set.json

# n=20 paired vs the prediction-market crowd, ~10 min wall-clock at concurrency=4
mneme bench --n 20 --name mybench
```

Reports independent + paired Brier Index for both mneme and the crowd, with bootstrap 95% CIs. Per-source breakdowns. The default in v0.1 will land you somewhere near our bench-005 result (BI ~84 vs crowd ~70 on this contaminated 2024 sample).

For honest post-cutoff held-out evaluation see `scripts/forecastbench_holdout_run.py` and result docs in [`mneme/plans/BLFX/results/`](https://github.com/hypermemetic/mneme/tree/master/plans/BLFX/results).

## Watch live Manifold markets

The live marketplace pipeline runs the substrate continuously against open prediction markets. No web-search contamination is possible — the answers don't exist yet.

```bash
mneme markets watch         # pick ~10 open binary markets, log paired (manifold_p, our_p)
mneme markets resolve       # sweep resolved markets, feed calibration store
```

Designed for cron — see `scripts/marketwatch_README.md`. Pairings accumulate in `programs/_marketwatch/pairings.jsonl`; the dataset that grows over weeks/months is what eventually produces defensible product claims.

## Forecast a software design decision

Add a `forecast:` block to a ticket's frontmatter:

```yaml
---
id: MY-TICKET-1
title: "..."
forecast:
  hypothesis: "Will <measurable outcome> by <date>?"
  resolution_method: "Run X; check Y; YES if Z."
  deadline: "2026-06-01T00:00:00Z"
---
```

Then:

```bash
mneme tickets predict       # fires forecast.update for each ticket with a forecast: block
mneme tickets resolve       # CLI prompt for actual outcome; calls forecast.resolve
```

The substrate's same forecasting machinery applied to its own design choices. Over time, the calibration store accumulates rows specifically about the system's design intuitions — meta-evidence about whether to trust the system on design questions.

## Activations

| Activation | Purpose |
|---|---|
| `forecast` | The headline — `update` (fire forecast), `resolve` (record outcome), `create` (open a question), `schema` |
| `claudecode` | Manage Claude Code sessions — fork, chat, get, stream, delete |
| `swarm` | Trial fan-out + logit-space aggregation primitives |
| `programs` | Inspect program directories — `list`, `inspect`, `status` |
| `arbor` | Conversation tree storage — every message in every session lives here |
| `lattice` | DAG execution engine (used by orcha) |
| `orcha` | Workflow orchestration — ticket-DSL → executable graph |
| `cone`, `bash`, `mustache`, `changelog` | Inherited from upstream substrate |

161 methods total; `synapse -P 4456 substrate _info` for the full menu.

## Architecture

```
┌─ External callers ─────────────────────────────────────────┐
│  synapse / MCP clients / WebSocket clients                 │
│  ↓ Plexus RPC (JSON-RPC 2.0 over WS / stdio / MCP-HTTP)    │
└────────────────────────────────────────────────────────────┘
                              │
┌─ mneme-substrate (one Rust binary, port 4456) ─────────────┐
│                                                             │
│  forecast activation                                        │
│    ↓ direct Rust call                                       │
│  swarm.trial — K parallel trials via futures::join_all      │
│    ↓                                                         │
│  ClaudecodeStepDriver — one trial = forked session +        │
│    iterative loop (parse step → execute action → repeat)    │
│    ↓                                                         │
│  CapabilityRegistry — picks model per task                  │
│    (reasoning_default=Sonnet, json_cleanup=Haiku, ...)      │
│    ↓                                                         │
│  claudecode activation — runs claude-code CLI subprocess    │
│    ↓                                                         │
│  CalibrationStore — Platt params fit from resolved obs      │
│                                                             │
│  Persistence:                                               │
│    /workspace/programs/   — per-call audit trail            │
│    /root/.plexus/         — per-activation state            │
│      (both bind-mounted to host disk)                       │
└────────────────────────────────────────────────────────────┘
                              │
                              ↓ subprocess
                     `claude` CLI (Node)
                              │
                              ↓ HTTPS
                     Anthropic API (your subscription)
```

## State files

After running, the host filesystem accumulates:

| Path | What |
|---|---|
| `programs/<program_id>/` | Per-forecast: manifest, artifact, trace, claudecode sessions |
| `programs/_calibration/history.jsonl` | Resolved (predicted, actual) pairs |
| `programs/_calibration/bias.json` | Current Platt params (refit on each `record()`) |
| `programs/_benchmarks/forecastbench/` | Vendored benchmark data |
| `programs/_benchmarks/runs/<ts>-<name>/` | Per-bench results.jsonl + summary.json |
| `programs/_marketwatch/pairings.jsonl` | Live (mneme_p, manifold_p) tuples |
| `programs/_marketwatch/resolutions.jsonl` | Live market outcomes when they resolve |
| `.plexus-state/` | All activation state (arbor trees, claudecode sessions, etc) |

`programs/` and `.plexus-state/` are gitignored — per-machine, per-run.

## Test status

```bash
cargo test --lib                             # 370+ tests, all green at v0.1
cargo build --release --bin mneme-substrate  # release binary
```

## Troubleshooting

**`Not logged in · Please run /login`** — the OAuth token wasn't forwarded. Check that `bash scripts/run_container.sh up -d` printed `auth: macOS Keychain (...)` or `auth: ANTHROPIC_API_KEY (env)`. If neither, run `claude /login` on the host first; the script then reads the resulting Keychain entry.

**Forecast hangs >10 min** — typically a metaculus question with a complex resolution criterion the model gets stuck on. Check `programs/<id>/sessions/` and the substrate logs (`docker logs mneme | tail -200`). The lenient EvidenceItem parser + JSON-cleanup recovery (per MNEME-29) eliminates ~95% of parse-failure-driven aborts since v0.1.

**Container restart wipes state** — should not happen in v0.1; `.plexus-state/` is bind-mounted. If it does, check `bash scripts/run_container.sh up` printed `plexus state: bind-mounting ... to /root/.plexus`. If that line is missing your local copy of `scripts/run_container.sh` is older — git pull.

## Lineage

Forked from [plexus-substrate](https://github.com/hypermemetic/plexus-substrate). The Plexus RPC framework is upstream; the BLF / forecast / capabilities / calibration / benchmarks / marketwatch are mneme-specific.

## License

AGPL-3.0-only.

## Authorship

Implementation: **Claude (Opus 4.7)** with the Anthropic Code SDK / Code CLI, via the methodology described in [`mneme`'s README](https://github.com/hypermemetic/mneme#authorship).

Direction: **Ben Haware**.

Every commit carries `Co-Authored-By: Claude Opus 4.7 (1M context)`.
