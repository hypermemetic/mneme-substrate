---
id: RUSTGEN-3
title: "hub-codegen Rust: rpc.rs generation (JSON-RPC framing + stream helpers, parity with TS rpc.ts)"
status: Pending
type: implementation
blocked_by: [RUSTGEN-2]
unlocks: [RUSTGEN-4]
severity: High
target_repo: hub-codegen
---

## Problem

The current Rust backend inlines JSON-RPC framing and stream-unwrapping logic into `generator/rust/client.rs` as part of `generate_base_client()`. The TypeScript backend separates these concerns: `transport.ts` handles the WebSocket, `rpc.ts` handles JSON-RPC request framing and stream helpers (unwrapping `PlexusStreamItem` sequences into typed results).

The Rust backend must separate the same way. This ticket introduces `generator/rust/rpc.rs` as a codegen module that emits a `src/rpc.rs` file into generated output. The file contains:

- JSON-RPC request framing helpers (`fn build_request(id, method, params) -> serde_json::Value`).
- JSON-RPC response parsing helpers.
- Stream-item unwrap helpers (`fn unwrap_single_data<T>(stream) -> Result<T>`, `fn unwrap_all_data<T>(stream) -> Result<Vec<T>>`, `fn unwrap_stream<T>(stream) -> impl Stream<Item = Result<T>>`).

These helpers are NOT user-facing API — they're internal building blocks that per-namespace client methods (RUSTGEN-5) compose.

## Context

**TS reference:** `hub-codegen/src/generator/typescript/rpc.rs` — 227 lines. Emits `rpc.ts` with request framing, response parsing, and async-generator-based stream helpers. Particularly note the `unwrapSingleData`, `unwrapAllData`, `streamData` function equivalents — these are the shape Rust must match.

**Current inline location in Rust backend:** `generator/rust/client.rs`'s `generate_base_client()` fn, lines ~60-160 (the `call_stream` and `call_single` methods on `PlexusClient`). These are currently glued onto the `PlexusClient` struct itself; this ticket extracts them as free functions (or trait methods on a dedicated trait) into `rpc.rs`, leaving `transport.rs` responsible only for the WebSocket layer (RUSTGEN-4).

**Emitted file shape** (roughly, subject to RUSTGEN-S01's runtime-library-shape decision):

```rust
// src/rpc.rs — auto-generated
use crate::types::PlexusStreamItem;
use futures::Stream;
use anyhow::{anyhow, Result};
use std::pin::Pin;
use serde_json::{json, Value};

pub fn build_request(id: u64, method: &str, params: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })
}

pub async fn unwrap_single_data<T: serde::de::DeserializeOwned>(
    mut stream: Pin<Box<dyn Stream<Item = Result<PlexusStreamItem>> + Send>>,
) -> Result<T> { /* pulls first Data item; handles Error/Done/Progress */ }

pub async fn unwrap_all_data<T: serde::de::DeserializeOwned>(
    stream: Pin<Box<dyn Stream<Item = Result<PlexusStreamItem>> + Send>>,
) -> Result<Vec<T>> { /* collects all Data items until Done */ }

pub fn unwrap_stream<T: serde::de::DeserializeOwned + Send + 'static>(
    stream: Pin<Box<dyn Stream<Item = Result<PlexusStreamItem>> + Send>>,
) -> Pin<Box<dyn Stream<Item = Result<T>> + Send>> { /* async_stream-based unwrap */ }
```

**RUSTGEN-S01's output:** decides if this file's content is emitted inline (option A), or if this file becomes `pub use plexus_client_runtime::rpc::*;` re-exports (option B). The codegen module produces one or the other based on the pinned choice.

## Required behavior

Emit a file `src/rpc.rs` in every generated crate. Content:

| Concern | Contract |
|---|---|
| Request framing | `build_request(id, method, params)` produces valid JSON-RPC 2.0. `id` is monotonic per `PlexusClient` instance (source-of-id lives on the client in RUSTGEN-4). |
| Single-result unwrap | `unwrap_single_data::<T>(stream)` returns the first `Data.content` deserialized as `T`. On `Error` returns an error with message+code. On `Done` without any `Data`, returns an error `"Stream completed without data"`. `Progress` items are skipped. |
| Collected-result unwrap | `unwrap_all_data::<T>(stream)` accumulates all `Data.content` items deserialized as `T` until `Done` is received. On `Error` returns an error. Returns `Vec<T>`. |
| Streaming unwrap | `unwrap_stream::<T>(stream)` returns a `Stream<Item = Result<T>>`. Emits `Ok(T)` per `Data` item. Terminates on `Done`. Emits `Err(...)` and terminates on `Error`. `Progress` items are silently skipped. |
| Error passthrough | `PlexusStreamItem::Error { message, code, recoverable }` is formatted as `"Plexus error[<code>]: <message>"` in all unwrap paths. |
| Sorted imports | Import block sorted alphabetically. |
| Deterministic output | Two consecutive generator runs produce byte-identical `src/rpc.rs`. |

The `rpc.rs` file is IR-independent — its content doesn't depend on the schema. The codegen module emits a constant string (possibly templated by RUSTGEN-S01's runtime-library-shape choice).

`lib.rs` gains a `pub mod rpc;` declaration (or `pub use`s the re-exported runtime equivalents, per RUSTGEN-S01).

## Risks

| Risk | Mitigation |
|---|---|
| If RUSTGEN-S01 picks a sibling crate (option B), `rpc.rs` content is a re-export stub. If it picks inline (option A), it's the full impl. The codegen module must handle both. | Make the emission strategy configurable in a single flag read from the runtime-library-shape pin. Default: inline. |
| `async_stream` macro behavior changes across versions. | Pin `async-stream = "0.3"` in generated Cargo.toml (RUSTGEN-7). Spike-verify one fixture to confirm the macro expansion compiles. |
| Stream helper functions need `Send` bounds to interoperate with tokio tasks. | Acceptance 5 pins this: unwrap_stream's returned stream is `Send`. |
| `unwrap_single_data` on a stream that starts with `Progress` before `Data` — race? | No — `unwrap_single_data` iterates until it sees `Data` (ignoring `Progress`) OR terminal (`Error`/`Done`). Pin in acceptance 4. |

## What must NOT change

- TypeScript backend's `rpc.ts` emission — unchanged.
- `PlexusStreamItem` wire format — unchanged.
- `generate()` and `generate_with_options()` entry points — signature unchanged.
- The top-level `src/types.rs` content — unchanged from RUSTGEN-2.
- Generated crate's public API: consumers of the old `PlexusClient::call_single` / `call_stream` (if any exist) would see these methods move. **But** the old backend's `PlexusClient` was freshly introduced by this codegen path; no external consumer depends on the old location yet. Internal refactor only.
- Pre-IR schema byte-identity: same pre-IR input produces the same generated output, with the `rpc.rs` file added as a new file but no change to existing file contents besides the removal of inline helpers from `client.rs`. (`client.rs` may be renamed to `transport.rs` in RUSTGEN-4; this ticket leaves that for RUSTGEN-4.)

## Acceptance criteria

1. `cargo build -p hub-codegen` and `cargo test -p hub-codegen` succeed.
2. A generated crate from a fixture IR contains `src/rpc.rs` with the four functions `build_request`, `unwrap_single_data`, `unwrap_all_data`, `unwrap_stream` (or their re-export stubs if RUSTGEN-S01 picked the sibling-crate shape).
3. The generated crate compiles (`cargo check` on the generated output) with `rpc.rs` present and `client.rs`'s inline helpers removed.
4. A fixture consumer test (in-memory stream fixtures, no real substrate) verifies: `unwrap_single_data` on a stream `[Progress, Progress, Data(42), Done]` returns `Ok(42)`. `unwrap_single_data` on `[Error("x"), Done]` returns `Err` with message `"Plexus error: x"`. `unwrap_single_data` on `[Done]` returns `Err("Stream completed without data")`.
5. The stream returned by `unwrap_stream::<T>(...)` is `Send` — verified by a `fn requires_send<T: Send>(_: T)` check in the fixture consumer test.
6. Two consecutive generator runs produce byte-identical `src/rpc.rs`.
7. The generated crate's `lib.rs` declares `pub mod rpc;` (or equivalent re-exports).
8. `grep -rn 'async_stream::stream' src/client.rs` in the generated output returns zero matches after this ticket lands (helpers migrated).

## Completion

PR against hub-codegen. CI green. PR description includes before/after diff showing `client.rs` shrinking and `rpc.rs` appearing. Golden fixtures may need reblessing for the file-layout change; if so, the reblessed diff is limited to relocation of functions (no semantic change). Status flipped from `Ready` to `Complete` in the same commit.
