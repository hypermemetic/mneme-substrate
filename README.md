# mneme-substrate

A Plexus RPC server that hosts mneme's skill activations and the substrate runtime they need: program lifecycle, per-program loopback tools, calibration store, swarm orchestration primitives.

For the concept, the architecture, the BLF inspiration, and the tickets: see the [`mneme`](https://github.com/hypermemetic/mneme) companion repo.

## What it serves

| Activation | Purpose |
|------------|---------|
| `claudecode` | Manage Claude Code sessions; the Layer 0 primitive |
| `swarm` | Trial fan-out, aggregation, sequential update, race |
| `forecast` | Binary forecasting with multi-trial aggregation and Platt calibration |
| `ticketing` | TDD ticket writer |
| `planning` | Epic DAG planner |
| `security_review` | SOC2-grouped audit with multi-trial severity calibration |
| `strong_typing` | Newtype proposal |
| `arbor`, `bash`, `mustache`, `orcha`, `lattice`, `changelog` | Inherited from upstream substrate |

## Run it

```bash
cargo run --bin mneme-substrate                 # starts on port 4444
synapse -P 4444                                  # list activations
synapse -P 4444 forecast create --question ...   # invoke a skill
```

## Inspect programs

```bash
cargo build --bin mneme
./target/debug/mneme programs list
./target/debug/mneme inspect <program-id>
./target/debug/mneme programs trace <program-id>
```

## Architecture

mneme-substrate is one Rust binary. Plexus RPC is the boundary protocol exposed to outside callers (synapse, MCP, WS); inside the binary, modules compose via Rust function calls. Loopback is the only place we cross a serialization boundary internally — when Claude inside a session calls back via MCP, the per-program tool registry handles routing.

Detail: [`docs/architecture/`](docs/architecture/).

## Lineage

Forked from [plexus-substrate](https://github.com/hypermemetic/plexus-substrate); inherits its activations and the Plexus RPC core unchanged in behavior.

## License

AGPL-3.0-only.
