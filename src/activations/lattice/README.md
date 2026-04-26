# lattice

DAG execution engine — manages graph topology and drives topological execution.

## Overview

Lattice is a general-purpose directed-acyclic-graph runtime. It owns graph
topology (nodes, edges, spec, status) and the event-sourced log that drives
execution: as nodes complete or fail, successors whose predecessors are all
satisfied transition to `Ready`, and a long-lived `execute` stream emits a
sequenced `LatticeEventEnvelope` for each state change. The caller — most
commonly Orcha — interprets typed `NodeSpec`s (`Task`, `Scatter`, `Gather`,
`SubGraph`) and performs the actual work, signalling back via
`node_complete` / `node_failed`.

Tokens are typed (`TokenColor::Ok` vs error, with optional payloads) and
edges can carry a `condition` that filters tokens by color — this is how
error-handling branches are wired in Orcha-compiled ticket graphs.

The `execute` stream is reconnectable. Passing `after_seq = <last seen>`
replays every event past that sequence number and then streams live,
so consumers can disconnect and re-attach without losing data; the stream
closes on `GraphDone` or `GraphFailed`.

## Namespace

`lattice` — invoked via `synapse <backend> lattice.<method>`.

## Methods

### Graph construction

| Method | Params | Returns | Description |
|---|---|---|---|
| `create` | `metadata: Value` | `Stream<Item=CreateResult>` | Create an empty graph. |
| `add_node` | `graph_id: GraphId, spec: NodeSpec, node_id: Option<NodeId>` | `Stream<Item=AddNodeResult>` | Add a typed node (task/scatter/gather/subgraph). |
| `add_edge` | `graph_id: GraphId, from_node_id: NodeId, to_node_id: NodeId, condition: Option<EdgeCondition>` | `Stream<Item=AddEdgeResult>` | Add a dependency edge, optionally filtered by token color. |
| `create_child_graph` | `parent_id: String, metadata: Value` | `Stream<Item=CreateChildGraphResult>` | Create a child graph for use with a `SubGraph` node spec. |

### Execution

| Method | Params | Returns | Description |
|---|---|---|---|
| `execute` | `graph_id: GraphId, after_seq: Option<u64>` | `Stream<Item=LatticeEventEnvelope>` | Start execution (or reconnect/replay). Stream closes on `GraphDone`/`GraphFailed`. |
| `node_complete` | `graph_id: GraphId, node_id: NodeId, output: Option<NodeOutput>` | `Stream<Item=NodeUpdateResult>` | Signal a node completed successfully; route its token(s) to successors. |
| `node_failed` | `graph_id: GraphId, node_id: NodeId, error: String` | `Stream<Item=NodeUpdateResult>` | Signal a node failed — triggers `GraphFailed`. |
| `cancel` | `graph_id: GraphId` | `Stream<Item=CancelResult>` | Cancel a running graph. |

### Introspection

| Method | Params | Returns | Description |
|---|---|---|---|
| `get` | `graph_id: GraphId` | `Stream<Item=GetGraphResult>` | Get graph state and all its nodes. |
| `list` | — | `Stream<Item=ListGraphsResult>` | List all graphs. |
| `get_node_inputs` | `graph_id: GraphId, node_id: NodeId` | `Stream<Item=GetNodeInputsResult>` | Raw input tokens arriving on all inbound edges. |
| `get_child_graphs` | `parent_id: String` | `Stream<Item=GetChildGraphsResult>` | List all child graphs of a parent graph. |

## Storage

- Backend: SQLite
- Config: `LatticeStorageConfig` with `db_path`.
- Schema: graphs, nodes, edges, and an append-only event log keyed by `seq`
  per graph. See `src/activations/lattice/storage.rs`.

## Composition

- `Orcha`, `Chaos`, and `Pm` all consume `Arc<LatticeStorage>` directly
  (obtained from `lattice.storage()`) rather than routing through
  Plexus-RPC. Orcha's `GraphRuntime` wraps the storage with the typed
  graph-runtime API.

## Example

```bash
synapse --port 44104 lforge substrate lattice.create '{"metadata":{}}'
synapse --port 44104 lforge substrate lattice.list
synapse --port 44104 lforge substrate lattice.execute \
  '{"graph_id":"<g>","after_seq":0}'
```

## Source

- `activation.rs` — RPC method surface
- `storage.rs` — SQLite persistence, event log, execution driver, and
  `LatticeStorageConfig`
- `types.rs` — `NodeSpec`, `NodeOutput`, `Token`, `TokenColor`,
  `EdgeCondition`, `LatticeEvent`, `LatticeEventEnvelope`, `GraphStatus`,
  `NodeStatus`, result enums
- `mod.rs` — module exports
