# orcha.pm

Project management view of orcha graph execution in ticket vocabulary.

## Overview

Pm is Orcha's static child activation — reachable as `orcha.pm.<method>`
via the `#[plexus_macros::child] fn pm(&self) -> pm::Pm` gate. It projects
the raw Lattice graph state back into ticket vocabulary: ticket ids,
ticket status, blocked-by relationships, per-ticket inspection, and
per-node execution logs.

Pm owns its own storage (`PmStorage`) — a small SQLite database that
persists three things:
1. The ticket-id ↔ lattice node-id map for each graph (saved by Orcha
   after compiling tickets into a graph).
2. The raw ticket source document for each graph (so
   `get_ticket_source` can reproduce the original text).
3. An append-only per-node execution log (events emitted during
   `dispatch_task`: `prompt`, `start`, `tool_use`, `tool_result`,
   `complete`, `error`, `passthrough`, `outcome`).

The read side joins this with the shared `LatticeStorage` to produce
ticket-level status summaries without the caller needing to understand
lattice node ids.

## Namespace

`pm` (reached as `orcha.pm`) — invoked via `synapse <backend> orcha.pm.<method>`.

## Methods

| Method | Params | Returns | Description |
|---|---|---|---|
| `graph_status` | `graph_id: String, recursive: Option<bool>` | `Stream<Item=PmGraphStatusResult>` | Status of all tickets in a graph. When `recursive=true`, includes `child_graph_id` from completed SubGraph node outputs. |
| `what_next` | `graph_id: String` | `Stream<Item=PmWhatNextResult>` | Tickets currently ready to execute (no unsatisfied dependencies). |
| `inspect_ticket` | `graph_id: String, ticket_id: String` | `Stream<Item=PmInspectResult>` | Full detail for one ticket: kind, task/command, output, error, child-graph id. |
| `why_blocked` | `graph_id: String, ticket_id: String` | `Stream<Item=PmWhyBlockedResult>` | List the tickets currently blocking `ticket_id` (or report `NotBlocked`). |
| `get_ticket_source` | `graph_id: String` | `Stream<Item=Value>` | Raw ticket source text for a graph. |
| `list_graphs` | `project: Option<String>, limit: Option<usize>, root_only: Option<bool>, status: Option<String>` | `Stream<Item=PmListGraphsResult>` | List graphs tracked by PM, filterable by `metadata.project`, `status`, and whether to include child graphs (`root_only` defaults to `true`, limit defaults to `20`). |
| `get_node_log` | `graph_id: String, node_id: String` | `Stream<Item=Value>` | Full execution log for a node — all events recorded by `dispatch_task` in sequence order. |

## Storage

- Backend: SQLite
- Config: `PmStorageConfig { db_path }`.
- Schema: `ticket_maps(graph_id, ticket_id, node_id, created_at)`,
  `ticket_sources(graph_id, source)`, `node_logs(graph_id, node_id,
  ticket_id, seq, event_type, event_data, created_at)`. See
  `src/activations/orcha/pm/storage.rs`.

## Composition

- `Arc<PmStorage>` — owned by Pm, injected at construction.
- `Arc<LatticeStorage>` — shared read access to graph + node state.
- Orcha calls the non-RPC helper methods (`save_ticket_map`,
  `save_ticket_source`, `log_node_event`, `get_ticket_map`,
  `list_all_graph_ids`, `get_ticket_source_raw`) directly during graph
  build and execution — these are in-process Rust APIs, not RPC methods.

## Example

```bash
synapse --port 44104 lforge substrate orcha.pm.list_graphs '{"limit":5}'
synapse --port 44104 lforge substrate orcha.pm.graph_status \
  '{"graph_id":"<g>"}'
synapse --port 44104 lforge substrate orcha.pm.what_next \
  '{"graph_id":"<g>"}'
synapse --port 44104 lforge substrate orcha.pm.why_blocked \
  '{"graph_id":"<g>","ticket_id":"t5"}'
synapse --port 44104 lforge substrate orcha.pm.get_node_log \
  '{"graph_id":"<g>","node_id":"<n>"}'
```

## Source

- `activation.rs` — RPC method surface + non-RPC helper methods used by
  Orcha (`save_ticket_map`, `log_node_event`, etc.)
- `storage.rs` — SQLite persistence + `PmStorageConfig`
- `mod.rs` — module exports
