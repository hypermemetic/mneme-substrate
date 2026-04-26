---
id: ST-5
title: "Migrate Lattice to use typed IDs"
status: Pending
type: implementation
blocked_by: [ST-2]
unlocks: [ST-10]
severity: High
target_repo: plexus-substrate
---

## Problem

Lattice defines its own stringly-typed identifiers:

```rust
pub type GraphId = String;
pub type NodeId = String;
```

These cross the boundary into Orcha (`cancel_graph(graph_id)`, `on_node_ready(node_id)`) and into every DAG edge. Two `String` parameters of the same underlying type are swap-compatible — the compiler cannot catch a `NodeId` passed where a `GraphId` was expected, nor detect that Orcha and Lattice use divergent alias definitions that happen to coincide today.

## Context

Lattice lives under `src/activations/lattice/`. Current type aliases (from `lattice/types.rs:6`):

```rust
pub type GraphId = String;
pub type NodeId = String;
```

Files owned by this ticket (exclusive write):

- `src/activations/lattice/activation.rs`
- `src/activations/lattice/types.rs`
- `src/activations/lattice/storage.rs`
- any other `src/activations/lattice/*.rs`

`lattice::NodeId` is distinct from `arbor::NodeId` — arbor's is a UUID-based node identifier for the handle tree; lattice's is a string identifier for a DAG node. ST-2 defines `NodeId` as the lattice concept (string). Arbor keeps its existing `ArborId`-aliased `NodeId`.

The ST-2 foundation provides `crate::types::{GraphId, NodeId}`.

## Required behavior

Input/output table for every changed public signature in Lattice:

| Current signature | New signature |
|---|---|
| `pub type GraphId = String;` in `types.rs` | Removed; re-exported from `crate::types` (optionally with `#[deprecated]` at `lattice::GraphId` for merge-order safety — mirror ST-4's approach) |
| `pub type NodeId = String;` in `types.rs` | Removed; re-exported from `crate::types::NodeId` |
| `Token`, `TokenColor`, `TokenPayload` fields — if any `String` field is actually an ID, wrap it | Unchanged (Token's `name: String` in `TokenColor::Named` is human vocabulary, not a domain ID) |
| `Graph`, `Node`, `Edge` structs with `id: String` or `graph_id: String` or `node_id: String` | `GraphId` / `NodeId` as appropriate |
| `fn create_graph(graph_id: String, ...) -> ...` | `fn create_graph(graph_id: GraphId, ...) -> ...` |
| `fn add_node(graph_id: String, node_id: String, ...) -> ...` | `fn add_node(graph_id: GraphId, node_id: NodeId, ...) -> ...` |
| Ready-event stream types carrying `String` IDs | `GraphId` / `NodeId` |
| HashMaps keyed on stringly-typed graph or node IDs | Keyed on `GraphId` or `NodeId` |

Storage boundary: SQLite columns stay `TEXT`. Rust reads `String` and wraps via `NodeId::new(...)` / `GraphId::new(...)` at the storage layer.

Handle-style opaque strings used for routing (not identity) remain `String`.

## Risks

- **`lattice::GraphId` / `lattice::NodeId` imports ripple into Orcha.** Orcha currently writes `use crate::activations::lattice::{GraphId, NodeId}` or similar. After this ticket, Orcha imports from `crate::types` instead. Per ST-4's plan, Orcha also migrates these — coordination handled by `#[deprecated]` re-exports at `lattice::GraphId` / `lattice::NodeId` during the transition.
- **`Token`'s nested `Handle` type** imports from `plexus_core::types::Handle` — unchanged; handles are opaque strings for routing.
- **`TokenColor::Named { name: String }` is application vocabulary, not an identifier.** Do NOT wrap in a newtype.

## What must NOT change

- Wire format for every Lattice RPC method — byte-identity.
- SQLite schema.
- DAG semantics — graph/node/edge logic is invariant.
- `Token`, `TokenColor`, `TokenPayload`, `GatherStrategy` shape (except fields that are IDs becoming newtypes with `#[serde(transparent)]`).

## Acceptance criteria

1. `cargo build -p plexus-substrate` succeeds.
2. `cargo test -p plexus-substrate` succeeds.
3. `pub type GraphId = String;` and `pub type NodeId = String;` no longer exist in `lattice/types.rs` (replaced by `#[deprecated]` re-exports of `crate::types::GraphId` / `crate::types::NodeId`).
4. Grep audit: no bare `String` parameter in any public function in `src/activations/lattice/` represents a `GraphId` or `NodeId`.
5. A unit test in Lattice constructs a minimal `Graph` / `Node` / `Edge` and round-trips through serde; the resulting JSON compares byte-identical against a committed pre-migration fixture.

## Completion

Implementor delivers:

- A commit modifying only files under `src/activations/lattice/`.
- A committed JSON fixture `tests/fixtures/lattice_graph_wire.json` for the serde comparison.
- `cargo build -p plexus-substrate` and `cargo test -p plexus-substrate` green.
- Ticket status flipped from `Ready` → `Complete`.
- ST-10 notified.
