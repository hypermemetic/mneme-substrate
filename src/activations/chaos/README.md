# chaos

Fault injection and observability for anti-fragility testing.

## Overview

Chaos exposes controlled chaos primitives against a running substrate:
observe nodes across all Lattice graphs, inject success or failure tokens
directly into Running nodes, list and kill system processes, snapshot a
graph's execution state, or hard-crash the substrate itself to exercise
recovery paths.

**Feature-gated.** Chaos lives behind the `chaos` Cargo feature and is **off
by default**. The feature pulls in `libc` and two narrow `#[allow(unsafe_code)]`
call-sites around `libc::kill` (SIGKILL). Every other build in the workspace
compiles with zero unsafe. Activate it explicitly with `cargo build --features
chaos`; the module is entirely `#[cfg]`-excluded from default builds so the
baseline library has no unsafe surface at all.

Chaos is a read/write client of `LatticeStorage` — it is constructed with
`Chaos::new(lattice.storage())` and manipulates nodes via
`advance_graph(..)` with injected `Ok` or `Err` tokens.

## Namespace

`chaos` — invoked via `synapse <backend> chaos.<method>` (only on chaos-enabled builds).

## Methods

| Method | Params | Returns | Description |
|---|---|---|---|
| `list_running_nodes` | — | `Stream<Item=ListRunningResult>` | List every node currently in Running state across all Lattice graphs. |
| `inject_failure` | `graph_id: String, node_id: String, error: Option<String>` | `Stream<Item=InjectResult>` | Force-fail a Running node with an error message (default `"chaos: injected failure"`). |
| `inject_success` | `graph_id: String, node_id: String, value: Option<String>` | `Stream<Item=InjectResult>` | Force-complete a Running node with an `Ok` token carrying an optional JSON payload. |
| `list_processes` | `pattern: String` | `Stream<Item=ListProcessesResult>` | List processes whose `/proc/<pid>/cmdline` contains `pattern`. |
| `kill_process` | `pid: u32` | `Stream<Item=KillProcessResult>` | SIGKILL a process by PID. (`unsafe`: `libc::kill`.) |
| `graph_snapshot` | `graph_id: String` | `Stream<Item=GraphSnapshotResult>` | Dump all nodes in a graph with their status and spec-type summary. |
| `crash` | — | `Stream<Item=InjectResult>` | Hard-crash the substrate itself (SIGKILL self). Use to exercise restart + recovery. (`unsafe`: `libc::kill`.) |

## Composition

- `Arc<LatticeStorage>` — injected at construction; every node-level
  operation routes through it.

## Example

```bash
# Requires `cargo build --features chaos` on the substrate
synapse --port 44104 lforge substrate chaos.list_running_nodes
synapse --port 44104 lforge substrate chaos.inject_failure \
  '{"graph_id":"<g>","node_id":"<n>","error":"simulated"}'
synapse --port 44104 lforge substrate chaos.crash
```

## Source

- `activation.rs` — RPC method surface + narrow `unsafe` signal calls
- `types.rs` — `ListRunningResult`, `InjectResult`, `ListProcessesResult`,
  `KillProcessResult`, `GraphSnapshotResult`, and their payload structs
- `mod.rs` — module exports (entire module `#[cfg(feature = "chaos")]`)
