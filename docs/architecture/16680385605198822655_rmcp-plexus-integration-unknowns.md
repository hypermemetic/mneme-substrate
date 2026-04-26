# RMCP-Plexus Integration: Unknowns and Design Considerations

## Executive Summary

This document analyzes the feasibility and unknowns of using the official MCP Rust SDK (`rmcp`) as the MCP layer in front of Plexus, replacing our hand-rolled `src/mcp/` implementation. Plexus is the activation system (bash, arbor, cone, claudecode) and cannot be altered; we need a bridge layer.

### Status: ✅ VALIDATED

All critical unknowns have been resolved. A working stub server (`examples/rmcp_mcp_server.rs`) demonstrates the integration pattern and has been verified working with Claude Code. **Remaining work**: Integrate Plexus into the example, then move to production.

## Current State

### What We Have
- **Plexus**: Activation system with streaming responses (`PlexusStream`)
- **Hand-rolled MCP layer** (`src/mcp/`): Incomplete implementation
  - `McpInterface`: routes methods, handles state machine
  - `McpState`: Uninitialized → Initializing → Ready
  - MCP-4 through MCP-8 implemented (initialize, initialized, ping, tools/list)
  - MCP-9 (`tools/call`) stub only - not streaming
- **HTTP Transport**: Custom axum router at `/mcp`
- **Streaming Model**: Plexus returns `PlexusStream = Pin<Box<dyn Stream<Item = PlexusStreamItem>>>`

### What RMCP Provides
- Complete MCP protocol types (`model/`)
- `ServerHandler` trait with `call_tool`, `list_tools`, etc.
- `#[tool]` and `#[tool_router]` macros for defining tools
- `StreamableHttpService`: tower-compatible HTTP transport with SSE
- Session management, authentication, progress notifications
- Full 2025-03-26 spec compliance with Streamable HTTP

## Critical Unknowns

### 1. Streaming Impedance Mismatch ✅ RESOLVED

**The core challenge**: RMCP tools return `Result<CallToolResult, McpError>`, but Plexus returns `PlexusStream`.

```rust
// RMCP expectation (single result)
async fn call_tool(&self, params: CallToolRequestParam) -> Result<CallToolResult, McpError>

// Plexus reality (streaming)
async fn call(&self, method: &str, params: Value) -> Result<PlexusStream, PlexusError>
```

#### Resolution: Dual-Channel Streaming via Notifications

RMCP provides **two notification mechanisms** callable during tool execution:

1. **`notifications/progress`** - For progress updates with numeric progress
   ```rust
   // From rmcp/src/model.rs:856-869
   pub struct ProgressNotificationParam {
       pub progress_token: ProgressToken,
       pub progress: f64,
       pub total: Option<f64>,
       pub message: Option<String>,  // String only
   }
   ```

2. **`notifications/message`** (logging) - For structured data
   ```rust
   // From rmcp/src/model.rs:1055-1066
   pub struct LoggingMessageNotificationParam {
       pub level: LoggingLevel,
       pub logger: Option<String>,
       pub data: Value,  // Full JSON value - can hold PlexusStreamEvent!
   }
   ```

#### Key Finding: RequestContext.peer Available During call_tool

From `rmcp/src/service/server.rs:386-416`, the `Peer<RoleServer>` provides:

```rust
impl Peer<RoleServer> {
    // Progress notifications (line 410)
    pub async fn notify_progress(&self, params: ProgressNotificationParam) -> Result<(), ServiceError>;

    // Logging notifications - STRUCTURED DATA (line 411)
    pub async fn notify_logging_message(&self, params: LoggingMessageNotificationParam) -> Result<(), ServiceError>;
}
```

The `RequestContext` passed to `call_tool` contains:
- `ctx.peer: Peer<RoleServer>` - for sending notifications during execution
- `ctx.meta: Meta` - contains `progressToken` from client request
- `ctx.ct: CancellationToken` - for cancellation propagation

#### Implementation Pattern (Truly Unbuffered)

```rust
impl ServerHandler for PlexusMcpBridge {
    async fn call_tool(
        &self,
        request: CallToolRequest,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        // 1. Extract progress token from client request
        let progress_token = ctx.meta.get_progress_token();

        // 2. Start Plexus stream
        let stream = self.plexus.call(&request.params.name, request.params.arguments).await?;

        // 3. Stream each event via notifications - NO BUFFERING
        let mut step = 0u64;
        let mut had_error = false;

        tokio::pin!(stream);
        while let Some(item) = stream.next().await {
            // Check cancellation on each iteration
            if ctx.ct.is_cancelled() {
                return Err(McpError::internal_error("Cancelled", None));
            }

            match &item.event {
                PlexusStreamEvent::Progress { message, percentage, .. } => {
                    if let Some(token) = &progress_token {
                        ctx.peer.notify_progress(ProgressNotificationParam {
                            progress_token: token.clone(),
                            progress: percentage.map(|p| p as f64).unwrap_or(step as f64),
                            total: None,
                            message: Some(message.clone()),
                        }).await.ok();
                    }
                }

                PlexusStreamEvent::Data { data, content_type, provenance } => {
                    // Stream data immediately - client receives via notification
                    ctx.peer.notify_logging_message(LoggingMessageNotificationParam {
                        level: LoggingLevel::Info,
                        logger: Some("plexus.stream".into()),
                        data: serde_json::json!({
                            "type": "data",
                            "content_type": content_type,
                            "data": data,
                            "provenance": provenance,
                            "plexus_hash": &item.plexus_hash,
                        }),
                    }).await.ok();
                    // NO push to buffer - data already delivered
                }

                PlexusStreamEvent::Error { error, recoverable, .. } => {
                    ctx.peer.notify_logging_message(LoggingMessageNotificationParam {
                        level: LoggingLevel::Error,
                        logger: Some("plexus.stream".into()),
                        data: serde_json::json!({
                            "type": "error",
                            "error": error,
                            "recoverable": recoverable,
                        }),
                    }).await.ok();

                    if !recoverable {
                        had_error = true;
                        break;
                    }
                }

                PlexusStreamEvent::Done { .. } => break,

                PlexusStreamEvent::Guidance { .. } => {
                    ctx.peer.notify_logging_message(LoggingMessageNotificationParam {
                        level: LoggingLevel::Warning,
                        logger: Some("plexus.guidance".into()),
                        data: serde_json::to_value(&item.event).unwrap_or_default(),
                    }).await.ok();
                }
            }
            step += 1;
        }

        // 4. Return minimal completion marker - all data already streamed via notifications
        if had_error {
            Ok(CallToolResult::error(vec![Content::text(
                "Stream completed with errors - see notifications for details"
            )]))
        } else {
            Ok(CallToolResult::success(vec![Content::text(
                format!("Stream completed: {} events delivered via notifications", step)
            )]))
        }
    }
}
```

#### Key Insight: CallToolResult as Completion Marker

The `CallToolResult` is **not** the data carrier - it's just the JSON-RPC response that signals "request complete". All real data flows through notifications:

```
Client                              Server
  |                                    |
  |--- tools/call (with progressToken) -->
  |                                    |
  |<-- notifications/progress (step 0) |
  |<-- notifications/message (data 0)  |
  |<-- notifications/progress (step 1) |
  |<-- notifications/message (data 1)  |
  |<-- notifications/message (data 2)  |
  |    ...                             |
  |<-- CallToolResult (completion)     |
  |                                    |
```

The final `CallToolResult` contains only:
- Success/error status
- Event count for debugging
- No buffered data

#### Why This Works

1. **SSE Delivery**: RMCP's `StreamableHttpService` sends notifications as SSE events
2. **Non-blocking**: `notify_*` methods are async but we `.ok()` to ignore send errors
3. **Structured Data**: `LoggingMessageNotification.data` is `Value`, not just `String`
4. **Token Correlation**: Client sends `progressToken` in `_meta`, we echo it back
5. **Cancellation**: `ctx.ct` allows cooperative cancellation mid-stream

#### Client Request Format

Client must include progress token to receive progress notifications:
```json
{
    "jsonrpc": "2.0",
    "id": 1,
    "method": "tools/call",
    "params": {
        "name": "bash.execute",
        "arguments": { "command": "ls -la" }
    },
    "_meta": { "progressToken": "unique-token-123" }
}
```

#### Remaining Questions - All Resolved

- **Claude Code Support**: ✅ VERIFIED - MCP is working when connecting through Claude. Every plexus tool coming out of RPC supports the exact same streaming interface - this is a fundamental design of plexus.

- **Backpressure**: ✅ RESOLVED (Client Responsibility) - This is a client issue. Server maintains moderately sized buffers (100-200 messages). Design target: 1,000,000 users who all fail to consume at once shouldn't cost more than 1 GB of storage RAM.

- **Error Recovery**: ✅ RESOLVED (Acceptable) - Current pattern ignores notification send failures via `.ok()`. This is acceptable for streaming because:
  1. Notifications are fire-and-forget by design in MCP
  2. If client disconnects mid-stream, we don't want to block/error the server-side processing
  3. The final `CallToolResult` will still be attempted, giving client a completion signal if reconnected
  4. Failed notifications indicate client issues (disconnect, backpressure), not server bugs

### 2. Tool Registration Model ✅ RESOLVED

**UNKNOWN 2.1**: How to register Plexus activations as RMCP tools without using `#[tool]` macro?

#### Resolution: Direct `ServerHandler` Implementation

**Decision**: Implement `ServerHandler` directly without macros. Macro-based routing removes the control we need. This is a thin routing layer so we can afford to avoid abstractions when they would increase the abstraction burden. We don't want to force ourselves through abstractions when the execution path becomes more complex.

```rust
impl ServerHandler for PlexusMcpBridge {
    async fn call_tool(&self, params: CallToolRequestParam, ctx: RequestContext)
        -> Result<CallToolResult, McpError>
    {
        let stream = self.plexus.call(&params.name, params.arguments).await?;
        // Stream via notifications (see Section 1)
    }

    async fn list_tools(&self, ...) -> Result<ListToolsResult, McpError> {
        let schemas = self.plexus.list_full_schemas();
        // Transform to RMCP Tool format
    }
}
```

**Why this works**:
- More flexible, less magic
- Manually implement schema transformation
- Full control over routing logic
- Demonstrated working in `examples/rmcp_mcp_server.rs`

### 3. Schema Transformation ⏳ PENDING PLEXUS INTEGRATION

**UNKNOWN 3.1**: Is our schema format compatible with RMCP's expectations?

Plexus generates schemas via `schemars`:
```rust
ActivationFullSchema {
    namespace: "bash",
    methods: [MethodSchemaInfo {
        name: "execute",
        params: Some(schemars::Schema { ... }),
        returns: Some(schemars::Schema { ... }),
    }]
}
```

RMCP expects:
```rust
Tool {
    name: "bash.execute".into(),
    description: Some("...".into()),
    input_schema: ToolInputSchema { ... }, // Arc<serde_json::Value>
}
```

**Status**: Manual schema construction demonstrated working in `examples/rmcp_mcp_server.rs`. Will validate `schemars` → RMCP transformation when Plexus is integrated.

**Key questions remaining**:
- Does `schemars::Schema` serialize to the same JSON as RMCP expects?
- What about `required` fields handling?
- How does RMCP handle nested `$defs`?

### 4. Transport Integration ✅ RESOLVED

**UNKNOWN 4.1**: How to integrate RMCP's `StreamableHttpService` with our existing axum setup?

#### Resolution: Demonstrated Working

RMCP's `StreamableHttpService` integrates cleanly with axum:

```rust
let service = StreamableHttpService::new(
    || Ok(StubMcpHandler::new()),
    LocalSessionManager::default().into(),
    StreamableHttpServerConfig::default(),
);
let router = axum::Router::new().nest_service("/mcp", service);
axum::serve(listener, router).await
```

**Verified**:
- ✅ RMCP's tower service works with axum infrastructure
- ✅ `LocalSessionManager` handles session state automatically
- ✅ Claude Code connects and functions correctly
- ⏳ Co-hosting with jsonrpsee WebSocket untested (but should work via separate routes)

### 5. State Machine Ownership ✅ RESOLVED

**UNKNOWN 5.1**: RMCP handles initialize/initialized internally - do we lose control?

#### Resolution: RMCP State Management is Sufficient

RMCP's internal state machine handles:
- Initialize → Initialized transition
- Request validation (methods only allowed after initialization)
- Session lifecycle

**We can still inject custom logic via `ServerHandler::initialize()`**:
```rust
impl ServerHandler for PlexusMcpBridge {
    async fn initialize(
        &self,
        request: InitializeRequestParam,
        context: RequestContext<RoleServer>,
    ) -> Result<InitializeResult, McpError> {
        // Custom validation, logging, setup here
        Ok(self.get_info())
    }
}
```

**Verified**: Claude Code initialization works correctly. RMCP's state machine doesn't conflict with our needs - we don't need to duplicate it.

### 6. Cancellation Model ✅ RESOLVED

**UNKNOWN 6.1**: How does RMCP's cancellation interact with Plexus streams?

#### Resolution: CancellationToken Integration Pattern

RMCP provides `ctx.ct: CancellationToken` which we check during stream iteration:

```rust
while let Some(item) = stream.next().await {
    // Check cancellation on each iteration
    if ctx.ct.is_cancelled() {
        return Err(McpError::internal_error("Cancelled", None));
    }
    // Process item...
}
```

**Verified in example**: The `stub.stream_count` tool demonstrates this pattern.

**Plexus integration**: Plexus streams already support `Drop`-based cleanup - when we return early due to cancellation, the stream is dropped and cleanup occurs automatically.

### 7. Type System Compatibility ✅ RESOLVED

**UNKNOWN 7.1**: schemars 0.8 vs 1.x compatibility

Our `Cargo.toml`:
```toml
schemars = { version = "1.1", features = ["derive", "uuid1"] }
```

RMCP's `Cargo.toml`:
```toml
schemars = { version = "1.0", optional = true, features = ["chrono04"] }
```

**Resolution**: Both use 1.x, compatible. RMCP's `Tool::input_schema` accepts `Arc<JsonObject>` which is just `Arc<serde_json::Map<String, Value>>` - we can convert schemars output via `serde_json::to_value()` → extract object.

## Architecture Options

### Chosen: Option B - RMCP as Full Handler

### Option A: RMCP as Transport Only

Keep our `McpInterface` but use RMCP's `StreamableHttpService` for HTTP transport.

```
┌─────────────────────────────────────────────────────────────┐
│                    RMCP Transport Layer                      │
│  StreamableHttpService → SSE streaming                       │
└───────────────────────────┬─────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────┐
│                    Our McpInterface                          │
│  handle() → route methods → state machine                    │
└───────────────────────────┬─────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────┐
│                        Plexus                                │
│  activations (bash, arbor, cone, claudecode)                 │
└─────────────────────────────────────────────────────────────┘
```

**Pros**: Minimal RMCP dependency, keep our logic
**Cons**: Still need to map our types to RMCP's transport types

### Option B: RMCP as Full Handler ✅ CHOSEN

Implement `ServerHandler` directly, delegate to Plexus.

```
┌─────────────────────────────────────────────────────────────┐
│                    RMCP Full Stack                           │
│  StreamableHttpService + ServerHandler                       │
└───────────────────────────┬─────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────┐
│                  PlexusMcpBridge                             │
│  impl ServerHandler for PlexusMcpBridge {                    │
│      call_tool() → plexus.call() → stream notifications      │
│      list_tools() → plexus.list_full_schemas() → transform   │
│  }                                                           │
└───────────────────────────┬─────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────┐
│                        Plexus                                │
│  activations (unmodified)                                    │
└─────────────────────────────────────────────────────────────┘
```

**Pros**: Full RMCP compliance, proven implementation
**Streaming solved**: Via `notify_logging_message()` - data flows unbuffered, `CallToolResult` is completion marker only

### Option C: Hybrid - RMCP Types, Custom Transport

Use RMCP's types (`model::*`) but keep our transport layer.

```rust
// Use RMCP types
use rmcp::model::{
    CallToolRequestParam, CallToolResult, Tool, ToolInputSchema,
    ServerInfo, ServerCapabilities,
};

// Our custom transport that streams
async fn handle_tools_call_sse(
    &self,
    request_id: Value,
    params: CallToolRequestParam,
) -> impl Stream<Item = SseEvent> {
    // Stream Plexus events as SSE
}
```

**Pros**: Standard types, custom streaming
**Cons**: Partial benefit, still custom code

## Recommended Investigation Steps

1. **Test RMCP progress notifications during tool execution**
   ```rust
   async fn call_tool(&self, params: CallToolRequestParam, ctx: RequestContext) {
       ctx.peer.notify_progress(ProgressNotificationParam { ... }).await;
   }
   ```

2. **Build minimal PlexusMcpBridge prototype**
   - Implement `ServerHandler` with simple buffering
   - Test with Claude Code as client
   - Measure latency impact

3. **Compare schema JSON output**
   ```bash
   # Our schema
   cargo run --bin substrate -- --stdio <<< '{"method":"plexus_activation_schema","params":["bash"]}'

   # RMCP tool schema
   # Compare Tool::input_schema format
   ```

4. **Test RMCP session management**
   - Stateful mode with session persistence
   - Stateless mode (one request per connection)
   - Impact on Claude Code reconnection

## Open Questions - All Resolved

1. **Streaming priority**: ✅ RESOLVED - Real-time unbuffered streaming via `notify_logging_message()`. No buffering.

2. **Protocol version**: ✅ RESOLVED - Using 2024-11-05 via `ProtocolVersion::LATEST`. RMCP handles version negotiation.

3. **Session management**: ✅ RESOLVED - Using `LocalSessionManager` which handles stateful sessions automatically. Claude Code works with this.

4. **Error handling**: ✅ RESOLVED - Map `PlexusError` → `McpError` variants:
   - `PlexusError::MethodNotFound` → `McpError::invalid_params(...)`
   - `PlexusError::Internal` → `McpError::internal_error(...)`
   - Rich guidance sent via `notify_logging_message()` before error return

## Next Steps

### ✅ Completed

1. **Added `rmcp` to dependencies** (dev-dependencies for now):
   ```toml
   rmcp = { version = "0.12", features = ["server", "transport-io", "transport-streamable-http-server"] }
   ```

2. **Created working stub server**: `examples/rmcp_mcp_server.rs`
   - Implements `ServerHandler` directly (no macros)
   - Demonstrates unbuffered streaming via `notify_logging_message()`
   - Uses `notify_progress()` for progress updates
   - Returns minimal `CallToolResult` as completion marker
   - Tested: initialize, tools/list, tools/call all working

### Remaining

3. **Integrate Plexus into `examples/rmcp_mcp_server.rs`**:
   - Replace `StubMcpHandler` with `PlexusMcpBridge` wrapping `Arc<Plexus>`
   - Transform `Plexus.list_full_schemas()` → `Vec<Tool>`
   - Stream `PlexusStreamEvent` → `notify_logging_message()`
   - Map `PlexusError` → `McpError`

4. **Move to production**: Once validated, move from example to `src/mcp/rmcp_bridge.rs`

5. **Replace main.rs transport**: Swap `mcp_router` with RMCP's `StreamableHttpService`

6. **Remove hand-rolled `src/mcp/`** once validated

## References

- [RMCP README](https://github.com/modelcontextprotocol/rust-sdk/tree/main/crates/rmcp)
- [MCP Specification 2025-03-26](https://modelcontextprotocol.io/specification/2025-03-26)
- [Current MCP Epic](../plans/MCP/MCP-1.md)
- [MCP Compatibility Spec](16680473255155665663_mcp-compatibility.md)
