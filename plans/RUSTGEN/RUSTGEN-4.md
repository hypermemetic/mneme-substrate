---
id: RUSTGEN-4
title: "hub-codegen Rust: transport.rs generation (WebSocket client, parity with TS transport.ts)"
status: Pending
type: implementation
blocked_by: [RUSTGEN-3, RUSTGEN-S02]
unlocks: [RUSTGEN-5]
severity: High
target_repo: hub-codegen
---

## Problem

The current Rust backend emits `src/client.rs` which bundles the `PlexusClient` struct + WebSocket transport + JSON-RPC framing + stream unwrapping into a single file. RUSTGEN-3 extracts rpc framing into `src/rpc.rs`. This ticket extracts the WebSocket transport into `src/transport.rs`, leaving `src/client.rs` thin (or eliminating it entirely in favor of `lib.rs` re-exports).

The TypeScript backend's `transport.ts` is the equivalent: owns the WebSocket connection, request-id generation, and the Raw-level send-and-collect methods. `transport.rs` for Rust is the same concept, using the dependency pinned by RUSTGEN-S02.

## Context

**TS reference:** `hub-codegen/src/generator/typescript/transport.rs` — 245 lines. Emits `transport.ts` with `WebSocketTransport` class: constructor takes a URL, manages reconnection, exposes `request(method, params)` returning an async iterable of `PlexusStreamItem`.

**RUSTGEN-S02 decision** (pinned before this ticket): which WebSocket / JSON-RPC dep shape to use. Three options: status quo `tokio-tungstenite`, `jsonrpsee`, or bundled hand-rolled framing on top of `tokio-tungstenite`. This ticket codifies the pinned choice.

**Emitted file shape** (subject to S02 decision):

```rust
// src/transport.rs — auto-generated
use crate::rpc::build_request;
use crate::types::PlexusStreamItem;
use anyhow::{anyhow, Result};
use futures::{stream::{Stream, StreamExt}, SinkExt};
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio_tungstenite::{connect_async, tungstenite::Message};

#[derive(Clone)]
pub struct PlexusClient {
    url: String,
    next_id: Arc<AtomicU64>,
}

impl PlexusClient {
    pub fn new(url: impl Into<String>) -> Self {
        Self { url: url.into(), next_id: Arc::new(AtomicU64::new(1)) }
    }

    pub(crate) async fn call_stream(&self, method: &str, params: Value)
        -> Result<Pin<Box<dyn Stream<Item = Result<PlexusStreamItem>> + Send>>>
    { /* open ws, send request, yield stream items */ }
}
```

Key differences from the current `client.rs`:

- `call_single` is GONE — moves to `rpc.rs` as a free function consuming a stream.
- `call_stream` stays — it's transport-layer (opens the WebSocket and yields raw `PlexusStreamItem` events).
- Request ID generation is state on the client (`AtomicU64`), not a hardcoded `id: 1`.
- Cleaner separation: transport = "open WS, frame requests, yield raw stream items". rpc = "consume raw stream items, produce typed results".

**Current bug surfaced by this split:** the existing `call_stream` hardcodes `id: 1` for every request. Two concurrent calls get the same id — which JSON-RPC multiplexers may reject. The atomic counter fixes this.

## Required behavior

Emit `src/transport.rs` and update `lib.rs` to re-export `PlexusClient`:

| Concern | Contract |
|---|---|
| `PlexusClient::new(url)` | Constructs a client bound to the given WebSocket URL. Generates a new atomic counter for request IDs. |
| `PlexusClient::clone()` | Cheap clone (Arc-shared id counter). Two clones see the same counter — id collisions across clones don't happen. |
| `call_stream(method, params)` | Opens a WebSocket to `self.url`, sends a JSON-RPC request with a fresh monotonic id, returns a stream that yields `PlexusStreamItem` parsed from each inbound text message. Terminates on Close or on `Done`/`Error`. |
| Stream termination | On `PlexusStreamItem::Done` or `PlexusStreamItem::Error`, the stream yields the item and then terminates. No items after terminals. |
| WebSocket errors | Handshake failure returns `Err(...)` from `call_stream` BEFORE the stream is returned. Mid-stream errors emit `Err(anyhow!(...))` as a stream item and terminate. |
| Request ID | Monotonic `u64` starting at 1, incremented per `call_stream`. Shared across clones of the same `PlexusClient`. |

**Where the dep pin applies:** the `use tokio_tungstenite::*;` line (or `use jsonrpsee::*;` — pinned by S02) is the only dep-specific surface. The public API (`PlexusClient::new`, `call_stream` signature) is dep-agnostic.

**lib.rs update:**

```rust
pub mod transport;
pub mod rpc;
pub mod types;
// ... namespace modules

pub use transport::PlexusClient;
```

`src/client.rs` is **deleted** (or kept as a shim with `pub use crate::transport::*;` for one release cycle if backward compat is needed — but since no external consumer exists, just delete).

## Risks

| Risk | Mitigation |
|---|---|
| `tokio_tungstenite::connect_async` returns `(WebSocketStream, Response)` — the second element is unused but moving between versions could change the tuple. | Pin `tokio-tungstenite = "0.21"` in generated Cargo.toml (RUSTGEN-7). |
| Concurrent `call_stream` from two tasks on the same `PlexusClient` open two separate WebSocket connections. This may or may not be desired (current code does this). Substrate supports many concurrent connections, so it works, but it's inefficient. | Out of scope for this ticket. Connection pooling / multiplexing is a future optimization (likely RL epic). Pin the current per-call-WS behavior explicitly; document it in a comment. |
| `jsonrpsee` (if S02 picks it) owns id generation internally — the atomic counter on the client is redundant. | The codegen path for S02-B conditionally omits the atomic counter when jsonrpsee is pinned. |
| Generated `transport.rs` drifts from the TS `transport.ts` on semantic details (reconnection, idle-timeout). | Out of scope. Reconnection is not in the TS transport either. Match the TS transport's observable behavior only; don't add features. |

## What must NOT change

- `PlexusStreamItem` wire format — unchanged.
- TypeScript `transport.ts` emission — unchanged.
- `GenerationResult` structure — unchanged. Files map gains `src/transport.rs`, loses `src/client.rs`.
- The public `PlexusClient::new(url)` signature. This is the one API surface a consumer directly depends on. If a consumer uses `PlexusClient::new("ws://...")` against the old backend, the same code works against the new backend — same module path (`crate::PlexusClient` via `lib.rs` re-export).
- Stream semantics: `Data → Data → Done` still terminates after `Done`.

## Acceptance criteria

1. `cargo build -p hub-codegen` and `cargo test -p hub-codegen` succeed.
2. A generated crate from a fixture IR contains `src/transport.rs` with `PlexusClient` struct and `call_stream` method. `src/client.rs` is absent.
3. The generated crate compiles (`cargo check` on the generated output).
4. A consumer fixture test constructs `PlexusClient::new("ws://localhost:99999")` (unreachable port), calls `call_stream`, and asserts the error is a connection failure, not a panic.
5. Two `call_stream` invocations on the same `PlexusClient` (or its clones) use different request IDs. Verified via a mock transport that captures sent messages and asserts `id` increments.
6. The generated crate's `lib.rs` re-exports `pub use transport::PlexusClient;`.
7. Two consecutive generator runs produce byte-identical `src/transport.rs`.
8. RUSTGEN-S02's pinned dep set appears in the generated `use` statements at the top of `transport.rs` (e.g., `use tokio_tungstenite::...` if S02-A; `use jsonrpsee::...` if S02-B).
9. The `id: 1` hardcode is gone — a `grep 'id: 1' generated_output/` returns zero matches for the request construction.

## Completion

PR against hub-codegen. CI green. PR description includes before/after diff showing `client.rs` deletion, `transport.rs` creation, and lib.rs re-export change. Golden fixtures reblessed for file-layout change (relocation-only; no semantic change apart from id-counter introduction). Status flipped from `Ready` to `Complete` in the same commit.
