# Generic Bidirectional JSON-RPC Implementation Plan

**Date:** 2026-02-12
**Status:** Planning
**Supersedes:** BIDIR-1 through BIDIR-10 (enhances with generic-first design)

## Executive Summary

This plan implements **generic bidirectional JSON-RPC communication** in the Plexus RPC ecosystem. Unlike the original BIDIR plans which proposed fixed `RequestType`/`ResponsePayload` enums, this plan implements **generic `BidirChannel<Req, Resp>`** from the start.

**Key Enhancement Over Original BIDIR Plans**:

| Original Approach | Generic-First Approach |
|------------------|------------------------|
| Fixed `RequestType` enum | `BidirChannel<Req, Resp>` generic over any serializable types |
| All activations use same request types | Each activation can define domain-specific request types |
| Basic confirm/prompt/select only | Standard patterns via type aliases + custom domain types |
| No composition support | Type-safe stream piping: `cmd1 \| cmd2` |

**Benefits**:
1. **Type-safe domain-specific requests** - Define custom request/response types per activation
2. **Stream composition via pipes** - `synapse cmd1 | synapse cmd2` with compile-time type checking
3. **Standard interactive patterns** - Convenient type aliases for common UI patterns
4. **Cross-language code generation** - TypeScript and Haskell clients with full type safety
5. **Cleaner architecture** - No need for separate "simple" and "advanced" versions

## Background

### Current Situation

**Existing Planning**:
- Complete epic breakdown in `BIDIR-1` through `BIDIR-10`
- Architecture doc: `docs/architecture/16678188726020813567_bidirectional-streaming.md`
- Claims "Implementation Status: Complete" but no code exists

**Actual Status**:
- No `feature/bidirectional-streaming` branch in any repo
- No implementation code found in plexus-core, plexus-substrate, or plexus-transport
- One documentation commit (Dec 30, 2025) added architecture doc only

**Conclusion**: Bidirectional work was planned and designed but never implemented.

### Repository Structure

- **plexus-core**: Core infrastructure (Activation trait, DynamicHub, schema generation)
- **plexus-substrate**: Reference server implementation (Arbor, Cone, activations)
- **plexus-transport**: Transport layer implementations (WebSocket, HTTP/SSE, stdio)
- **plexus-macros**: Procedural macros for activation generation

## Problem Statement

**Goal**: Enable **type-safe** server-to-client requests during streaming RPC method execution.

**Current Limitation**: Plexus RPC is unidirectional (server → client streaming only). Activations cannot request user input or confirmation mid-execution.

**Desired State**: Generic bidirectional communication where:
- Server can send **custom-typed requests** to client during stream execution
- Client responds with **type-checked responses**
- Activations can **compose via pipes** with bidirectional interaction
- Works over both MCP and WebSocket transports
- Fully backward compatible with existing unidirectional methods
- Standard interactive patterns (confirm/prompt/select) available via type aliases
- Custom domain-specific request types supported for specialized workflows

**Example Use Cases**:
```bash
# Standard interactive pattern
synapse delete-files --interactive  # Uses StandardBidirChannel

# Custom typed requests in pipeline
synapse generate-images | synapse watermark-images --interactive
# watermark-images uses BidirChannel<ImageRequest, ImageResponse>

# Multi-stage composition
synapse list-repos | synapse filter-repos | synapse clone-repos
# Each stage can ask domain-specific questions with full type safety
```

## Architecture Overview

### Generic Channel Design

```rust
// Generic from the start
pub struct BidirChannel<Req, Resp>
where
    Req: Serialize + DeserializeOwned + Send + 'static,
    Resp: Serialize + DeserializeOwned + Send + 'static,
{
    stream_tx: mpsc::Sender<PlexusStreamItem>,
    pending: Arc<Mutex<HashMap<String, oneshot::Sender<Resp>>>>,
    bidirectional_supported: bool,
    provenance: Vec<String>,
    plexus_hash: String,
    _phantom_req: PhantomData<Req>,
}

impl<Req, Resp> BidirChannel<Req, Resp> {
    pub async fn request(&self, req: Req) -> Result<Resp, BidirError> { ... }
}
```

### Standard UI Patterns (Type Aliases)

```rust
// Convenient types for common interactive patterns
#[derive(Serialize, Deserialize, JsonSchema)]
pub enum StandardRequest {
    Confirm { message: String, default: Option<bool> },
    Prompt { message: String, default: Option<String>, placeholder: Option<String> },
    Select { message: String, options: Vec<SelectOption>, multi_select: bool },
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub enum StandardResponse {
    Confirmed(bool),
    Text(String),
    Selected(Vec<String>),
    Cancelled,
}

pub type StandardBidirChannel = BidirChannel<StandardRequest, StandardResponse>;

// Convenience methods
impl BidirChannel<StandardRequest, StandardResponse> {
    pub async fn confirm(&self, message: &str) -> Result<bool, BidirError> { ... }
    pub async fn prompt(&self, message: &str) -> Result<String, BidirError> { ... }
    pub async fn select(&self, message: &str, options: Vec<SelectOption>) -> Result<Vec<String>, BidirError> { ... }
}
```

### Custom Domain Types

```rust
// Example: Image processing pipeline
#[derive(Serialize, Deserialize, JsonSchema)]
pub enum ImageRequest {
    ConfirmOverwrite { path: String },
    ChooseQuality { options: Vec<u8> },
    SkipLowRes { width: u32, height: u32, threshold: u32 },
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub enum ImageResponse {
    Confirmed(bool),
    Quality(u8),
    Skip(bool),
}

pub type ImageBidirChannel = BidirChannel<ImageRequest, ImageResponse>;

// Usage in activation
#[hub_method(
    description = "Process images with quality selection",
    bidirectional(request = "ImageRequest", response = "ImageResponse")
)]
pub async fn process_images(
    &self,
    ctx: &ImageBidirChannel,
    input: impl Stream<Item = ImageData>,
) -> impl Stream<Item = ProcessedImage> {
    stream! {
        pin_mut!(input);
        while let Some(img) = input.next().await {
            if img.width < 800 {
                let skip = ctx.request(ImageRequest::SkipLowRes {
                    width: img.width,
                    height: img.height,
                    threshold: 800,
                }).await?;

                if matches!(skip, ImageResponse::Skip(true)) {
                    continue;
                }
            }

            let quality = ctx.request(ImageRequest::ChooseQuality {
                options: vec![80, 90, 100],
            }).await?;

            if let ImageResponse::Quality(q) = quality {
                yield compress(img, q);
            }
        }
    }
}
```

## How It Works

### 1. Request Flow

```
Activation Method (Server)
  │
  ├─ ctx.request(ImageRequest::ChooseQuality { ... }).await
  │   │
  │   ├─ Generate unique request_id: "req-123"
  │   ├─ Create oneshot channel for response
  │   ├─ Store channel in pending map: pending["req-123"] = tx
  │   ├─ Serialize request to JSON
  │   ├─ Send PlexusStreamItem::Request via stream
  │   └─ BLOCK waiting for response (timeout: 30s)
  │
  └─ (continues when response arrives)

Transport Layer
  │
  ├─ Receives PlexusStreamItem::Request from stream
  ├─ Forwards to client (MCP notification or WebSocket message)
  │
  └─ Client receives request, user interacts, sends response

Transport Layer
  │
  ├─ Receives response from client
  ├─ Deserializes JSON to Resp type
  ├─ Calls ctx.handle_response(request_id, response_data)
  │   │
  │   ├─ Looks up pending["req-123"]
  │   ├─ Sends response through oneshot channel
  │   └─ Removes from pending map
  │
  └─ This unblocks ctx.request().await in activation

Activation Method (Server)
  │
  ├─ ctx.request() returns Ok(ImageResponse::Quality(90))
  └─ Continues execution with response
```

### 2. Transport Mappings

**MCP Transport** (uses logging notification workaround):
```json
// Server sends request
{
  "method": "notifications/logging",
  "params": {
    "level": "info",
    "logger": "plexus.bidir",
    "data": {
      "type": "request",
      "request_id": "req-123",
      "request_data": { "ChooseQuality": { "options": [80, 90, 100] } },
      "timeout_ms": 30000
    }
  }
}

// Client responds via tool call
{
  "method": "tools/call",
  "params": {
    "name": "_plexus_respond",
    "arguments": {
      "request_id": "req-123",
      "response_data": { "Quality": 90 }
    }
  }
}
```

**WebSocket Transport** (natural bidirectional):
```json
// Server sends request in subscription
{
  "jsonrpc": "2.0",
  "method": "process_images",
  "params": {
    "subscription": "sub-456",
    "result": {
      "type": "request",
      "request_id": "req-123",
      "request_data": { "ChooseQuality": { "options": [80, 90, 100] } }
    }
  }
}

// Client responds via RPC call
{
  "jsonrpc": "2.0",
  "method": "plexus_respond",
  "params": {
    "subscription_id": "sub-456",
    "request_id": "req-123",
    "response_data": { "Quality": 90 }
  }
}
```

### 3. Stream Piping

```bash
synapse list-repos | synapse filter-repos | synapse clone-repos
```

```rust
// Type checking at composition time:
// list-repos output: Stream<Repo>
// filter-repos input: Stream<Repo> ✓
// filter-repos output: Stream<Repo>
// clone-repos input: Stream<Repo> ✓

// filter-repos can make bidirectional requests:
#[hub_method(bidirectional(request = "FilterRequest", response = "FilterResponse"))]
pub async fn filter_repos(
    &self,
    ctx: &BidirChannel<FilterRequest, FilterResponse>,
    input: impl Stream<Item = Repo>,
) -> impl Stream<Item = Repo> {
    stream! {
        pin_mut!(input);
        while let Some(repo) = input.next().await {
            if repo.archived {
                let include = ctx.request(
                    FilterRequest::IncludeArchived { repo_name: repo.name.clone() }
                ).await?;

                if !matches!(include, FilterResponse::Include(true)) {
                    continue;
                }
            }
            yield repo;
        }
    }
}
```

## Workstreams

### WS1: Assess Current Implementation Status 🔍

**Objective**: Determine what bidirectional work has already been completed

**Tasks**:
1. Review commit `7c6105c` in plexus-substrate (bidirectional SSE transport doc)
2. Search plexus-core for any bidirectional types
3. Search plexus-transport for bidirectional transport implementations
4. Determine if implementation exists but was merged to main

**Success Criteria**:
- [ ] Clear understanding of what code exists vs. what's still needed
- [ ] List of implemented components
- [ ] List of missing components
- [ ] Decision on whether to start fresh or continue existing work

**Estimated Scope**: Research and analysis (0.5 days)

**Dependencies**: None
**Unlocks**: WS2

---

### WS2: Generic Core Types (plexus-core)

**Objective**: Implement generic bidirectional channel types in plexus-core

**Location**: `plexus-core/src/plexus/bidirectional/types.rs`

**Components**:

1. **Generic Stream Item Variant** - Extend `PlexusStreamItem`:
   ```rust
   pub enum PlexusStreamItem {
       Data { /* ... */ },
       Progress { /* ... */ },
       Error { /* ... */ },
       Done { /* ... */ },
       Request {
           request_id: String,
           request_data: Value,  // Serialized Req type
           timeout_ms: u64,
       },
   }
   ```

2. **Error Types**:
   ```rust
   #[derive(Debug, Clone)]
   pub enum BidirError {
       NotSupported,
       Timeout,
       Cancelled,
       TypeMismatch { expected: String, got: String },
       Serialization(String),
       Transport(String),
       UnknownRequest,
       ChannelClosed,
   }
   ```

3. **Standard Request/Response Types** (convenience, not required):
   ```rust
   #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
   pub enum StandardRequest {
       Confirm { message: String, default: Option<bool> },
       Prompt { message: String, default: Option<String>, placeholder: Option<String> },
       Select { message: String, options: Vec<SelectOption>, multi_select: bool },
   }

   #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
   pub enum StandardResponse {
       Confirmed(bool),
       Text(String),
       Selected(Vec<String>),
       Cancelled,
   }

   #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
   pub struct SelectOption {
       pub value: String,
       pub label: String,
       pub description: Option<String>,
   }

   // Type alias for convenience
   pub type StandardBidirChannel = BidirChannel<StandardRequest, StandardResponse>;
   ```

**Success Criteria**:
- [ ] Generic design supports any Serialize + DeserializeOwned types
- [ ] StandardRequest/StandardResponse cover common UI patterns
- [ ] Clear documentation on defining custom types
- [ ] Examples show domain-specific request/response types
- [ ] Unit tests for serialization/deserialization
- [ ] Type aliases make common cases ergonomic

**Estimated Scope**: 1 day

**Dependencies**: WS1
**Unlocks**: WS3, WS4, WS5

---

### WS3: Generic Channel Implementation (plexus-core)

**Objective**: Implement generic `BidirChannel<Req, Resp>` for type-safe bidirectional requests

**Location**: `plexus-core/src/plexus/bidirectional/channel.rs`

**Components**:

1. **Generic BidirChannel**:
   ```rust
   pub struct BidirChannel<Req, Resp>
   where
       Req: Serialize + DeserializeOwned + Send + 'static,
       Resp: Serialize + DeserializeOwned + Send + 'static,
   {
       stream_tx: mpsc::Sender<PlexusStreamItem>,
       pending: Arc<Mutex<HashMap<String, oneshot::Sender<Resp>>>>,
       bidirectional_supported: bool,
       provenance: Vec<String>,
       plexus_hash: String,
       _phantom_req: PhantomData<Req>,
   }
   ```

2. **Core Generic Methods**:
   ```rust
   impl<Req, Resp> BidirChannel<Req, Resp> {
       pub fn new(
           stream_tx: mpsc::Sender<PlexusStreamItem>,
           bidirectional_supported: bool,
           provenance: Vec<String>,
           plexus_hash: String,
       ) -> Self { ... }

       pub async fn request(&self, req: Req) -> Result<Resp, BidirError> {
           if !self.bidirectional_supported {
               return Err(BidirError::NotSupported);
           }

           let request_id = Uuid::new_v4().to_string();
           let (tx, rx) = oneshot::channel();

           self.pending.lock().unwrap().insert(request_id.clone(), tx);

           let request_data = serde_json::to_value(&req)
               .map_err(|e| BidirError::Serialization(e.to_string()))?;

           self.stream_tx.send(PlexusStreamItem::Request {
               request_id: request_id.clone(),
               request_data,
               timeout_ms: 30000,
           }).await?;

           match timeout(Duration::from_millis(30000), rx).await {
               Ok(Ok(resp)) => Ok(resp),
               Ok(Err(_)) => Err(BidirError::Cancelled),
               Err(_) => Err(BidirError::Timeout),
           }
       }

       pub async fn request_with_timeout(
           &self,
           req: Req,
           timeout_duration: Duration,
       ) -> Result<Resp, BidirError> { ... }

       pub fn is_bidirectional(&self) -> bool {
           self.bidirectional_supported
       }

       pub fn handle_response(
           &self,
           request_id: String,
           response_data: Value,
       ) -> Result<(), BidirError> {
           let tx = self.pending.lock().unwrap()
               .remove(&request_id)
               .ok_or(BidirError::UnknownRequest)?;

           let resp: Resp = serde_json::from_value(response_data)
               .map_err(|e| BidirError::TypeMismatch {
                   expected: std::any::type_name::<Resp>().to_string(),
                   got: e.to_string(),
               })?;

           tx.send(resp).map_err(|_| BidirError::ChannelClosed)?;
           Ok(())
       }
   }
   ```

3. **Convenience Methods for StandardBidirChannel**:
   ```rust
   impl BidirChannel<StandardRequest, StandardResponse> {
       pub async fn confirm(&self, message: &str) -> Result<bool, BidirError> {
           let resp = self.request(StandardRequest::Confirm {
               message: message.to_string(),
               default: None,
           }).await?;

           match resp {
               StandardResponse::Confirmed(b) => Ok(b),
               StandardResponse::Cancelled => Err(BidirError::Cancelled),
               _ => Err(BidirError::TypeMismatch {
                   expected: "Confirmed".into(),
                   got: format!("{:?}", resp),
               }),
           }
       }

       pub async fn prompt(&self, message: &str) -> Result<String, BidirError> { ... }
       pub async fn select(&self, message: &str, options: Vec<SelectOption>) -> Result<Vec<String>, BidirError> { ... }
   }
   ```

4. **Generic Fallback Pattern**:
   ```rust
   pub struct BidirWithFallback<Req, Resp> {
       channel: Arc<BidirChannel<Req, Resp>>,
       fallback_fn: Box<dyn Fn(&Req) -> Resp + Send + Sync>,
   }

   impl<Req, Resp> BidirWithFallback<Req, Resp> {
       pub fn new(
           channel: Arc<BidirChannel<Req, Resp>>,
           fallback: impl Fn(&Req) -> Resp + Send + Sync + 'static,
       ) -> Self { ... }

       pub async fn request(&self, req: Req) -> Resp {
           match self.channel.request(req.clone()).await {
               Ok(resp) => resp,
               Err(_) => (self.fallback_fn)(&req),
           }
       }
   }
   ```

**Success Criteria**:
- [ ] Generic BidirChannel compiles with any Req/Resp types
- [ ] Request/response correlation works via unique IDs
- [ ] Type mismatches caught at compile time for activation code
- [ ] Type mismatches caught at runtime for transport layer
- [ ] Timeout mechanism works correctly
- [ ] Fallback pattern handles NotSupported gracefully
- [ ] StandardBidirChannel has ergonomic helper methods
- [ ] Unit tests cover:
  - [ ] Happy path with custom types
  - [ ] Type mismatch errors
  - [ ] Timeout scenarios
  - [ ] Fallback patterns
  - [ ] Concurrent requests

**Estimated Scope**: 2 days

**Dependencies**: WS2
**Unlocks**: WS4, WS5, WS6, WS7

---

### WS4: Macro Support for Generic Bidirectional Channels (plexus-macros)

**Objective**: Update `#[hub_method]` macro to support generic bidirectional channels

**Location**: `plexus-macros/src/parse.rs`, `plexus-macros/src/codegen/`

**Components**:

1. **Attribute Parsing** - Support multiple syntax forms:
   ```rust
   // Standard interactive UI (uses StandardBidirChannel)
   #[hub_method(description = "Simple wizard", bidirectional)]

   // Custom types (fully specified)
   #[hub_method(
       description = "Image processor",
       bidirectional(request = "ImageRequest", response = "ImageResponse")
   )]

   // Infer from context parameter type
   #[hub_method(description = "Inferred")]
   pub async fn process(&self, ctx: &BidirChannel<MyReq, MyResp>, ...) { }
   ```

2. **Type Extraction** - Extract Req/Resp from:
   - Explicit `bidirectional(request = "X", response = "Y")` attribute
   - Parameter type `ctx: &BidirChannel<X, Y>`
   - Default to `StandardRequest/StandardResponse` if just `bidirectional`

3. **Schema Generation** - Capture generic type information:
   ```json
   {
     "method": "process_images",
     "bidirectional": {
       "enabled": true,
       "request_type": {
         "name": "ImageRequest",
         "schema": { /* JSON schema */ }
       },
       "response_type": {
         "name": "ImageResponse",
         "schema": { /* JSON schema */ }
       }
     }
   }
   ```

**Success Criteria**:
- [ ] Macro accepts all three syntax forms
- [ ] Generated code passes correct generic BidirChannel
- [ ] Type inference works from parameter types
- [ ] Schema includes request/response type information
- [ ] Schema generation works with schemars for custom types
- [ ] Non-bidirectional methods still work (backward compatible)
- [ ] Compile-time errors for mismatched types
- [ ] Integration tests:
  - [ ] StandardBidirChannel usage
  - [ ] Custom generic types
  - [ ] Type inference

**Estimated Scope**: 2 days

**Dependencies**: WS2, WS3
**Unlocks**: WS6, WS7, WS8, WS11, WS12

---

### WS5: Helper Functions and Patterns (plexus-core)

**Objective**: Provide ergonomic patterns for common bidirectional use cases

**Location**: `plexus-core/src/plexus/bidirectional/helpers.rs`

**Components**:

1. **Error Conversion**:
   ```rust
   pub fn bidir_error_message(err: &BidirError) -> String {
       match err {
           BidirError::NotSupported => "Bidirectional communication not supported".into(),
           BidirError::Timeout => "Request timed out waiting for response".into(),
           BidirError::Cancelled => "Request was cancelled by user".into(),
           BidirError::TypeMismatch { expected, got } =>
               format!("Type mismatch: expected {}, got {}", expected, got),
           BidirError::Serialization(e) => format!("Serialization error: {}", e),
           BidirError::Transport(e) => format!("Transport error: {}", e),
           BidirError::UnknownRequest => "Unknown request ID".into(),
           BidirError::ChannelClosed => "Response channel closed".into(),
       }
   }
   ```

2. **Timeout Configurations**:
   ```rust
   #[derive(Debug, Clone)]
   pub struct TimeoutConfig {
       pub confirm: Duration,
       pub prompt: Duration,
       pub select: Duration,
       pub custom: Duration,
   }

   impl TimeoutConfig {
       pub fn quick() -> Self {
           Self {
               confirm: Duration::from_secs(10),
               prompt: Duration::from_secs(10),
               select: Duration::from_secs(10),
               custom: Duration::from_secs(10),
           }
       }

       pub fn normal() -> Self {
           Self {
               confirm: Duration::from_secs(30),
               prompt: Duration::from_secs(30),
               select: Duration::from_secs(30),
               custom: Duration::from_secs(30),
           }
       }

       pub fn patient() -> Self {
           Self {
               confirm: Duration::from_secs(60),
               prompt: Duration::from_secs(60),
               select: Duration::from_secs(60),
               custom: Duration::from_secs(60),
           }
       }
   }
   ```

3. **Testing Utilities**:
   ```rust
   /// Create a test bidirectional channel for unit tests
   pub fn create_test_bidir_channel<Req, Resp>() -> (
       Arc<BidirChannel<Req, Resp>>,
       mpsc::Receiver<PlexusStreamItem>
   )
   where
       Req: Serialize + DeserializeOwned + Send + 'static,
       Resp: Serialize + DeserializeOwned + Send + 'static,
   {
       let (tx, rx) = mpsc::channel(32);
       let channel = Arc::new(BidirChannel::new(
           tx,
           true,  // bidirectional_supported
           vec!["test".into()],
           "test-hash".into(),
       ));
       (channel, rx)
   }

   /// Create a channel that auto-responds based on a function
   pub fn auto_respond_channel<Req, Resp>(
       responses: impl Fn(&Req) -> Resp + Send + Sync + 'static
   ) -> Arc<BidirChannel<Req, Resp>>
   where
       Req: Serialize + DeserializeOwned + Send + 'static,
       Resp: Serialize + DeserializeOwned + Send + 'static,
   { ... }
   ```

**Success Criteria**:
- [ ] Helper functions compile and work correctly
- [ ] Documentation with usage examples
- [ ] Test utilities make testing easier
- [ ] Examples in doc comments

**Estimated Scope**: 0.5 days

**Dependencies**: WS2, WS3
**Unlocks**: WS8, WS9

---

### WS6: MCP Transport Integration (plexus-substrate)

**Objective**: Map bidirectional protocol to MCP notifications and tools

**Location**: `plexus-substrate/src/mcp_bridge.rs`

**Challenge**: MCP is fundamentally request→response, so bidirectional requires workaround:
1. Server sends Request as `logging` notification
2. Client responds via `_plexus_respond` tool call
3. Server correlates response by `request_id`

**Components**:

1. **Request as Notification**:
   ```rust
   // In MCP bridge, when streaming encounters Request item
   if let PlexusStreamItem::Request { request_id, request_data, timeout_ms } = item {
       // Send as logging notification
       send_notification(LoggingNotification {
           level: LogLevel::Info,
           logger: Some("plexus.bidir".into()),
           data: Some(json!({
               "type": "request",
               "request_id": request_id,
               "request_data": request_data,
               "timeout_ms": timeout_ms,
           })),
       });
   }
   ```

2. **Response Tool**:
   ```rust
   #[derive(Debug, Clone, Serialize, Deserialize)]
   pub struct PlexusRespondParams {
       pub request_id: String,
       pub response_data: Value,
   }

   // Register as MCP tool
   pub struct PlexusRespondTool;

   impl Tool for PlexusRespondTool {
       fn name(&self) -> &str { "_plexus_respond" }
       fn description(&self) -> &str {
           "Respond to a bidirectional request from the server"
       }

       async fn execute(&self, params: Value) -> Result<Value, ToolError> {
           let params: PlexusRespondParams = serde_json::from_value(params)?;

           // Look up active subscription by request_id
           let subscription = find_subscription_by_request_id(&params.request_id)?;

           // Forward response to BidirChannel
           subscription.bidir_channel.handle_response(
               params.request_id,
               params.response_data,
           )?;

           Ok(json!({ "status": "ok" }))
       }
   }
   ```

3. **Subscription State Management**:
   ```rust
   struct ActiveSubscription {
       subscription_id: String,
       bidir_channel: Arc<BidirChannel<Value, Value>>,  // Erased types for storage
       pending_requests: HashMap<String, /* subscription_id */>,
   }
   ```

**Success Criteria**:
- [ ] Request notifications sent correctly
- [ ] `_plexus_respond` tool registered and callable
- [ ] Response correlation works
- [ ] Timeout handling works
- [ ] Integration test with MCP client
- [ ] Works with both StandardBidirChannel and custom types

**Estimated Scope**: 2 days

**Dependencies**: WS3, WS4
**Unlocks**: WS8

---

### WS7: WebSocket Transport Integration (plexus-substrate or plexus-transport)

**Objective**: Implement bidirectional protocol over WebSocket using jsonrpsee

**Location**: `plexus-substrate/src/rpc.rs` or `plexus-transport/src/websocket.rs`

**Advantage**: WebSocket is naturally bidirectional, cleaner than MCP

**Components**:

1. **Request in Subscription**:
   ```rust
   // When streaming encounters Request item
   if let PlexusStreamItem::Request { request_id, request_data, .. } = item {
       // Send as part of subscription
       sink.send(json!({
           "type": "request",
           "request_id": request_id,
           "request_data": request_data,
       })).await?;
   }
   ```

2. **Response RPC Method**:
   ```rust
   #[rpc(server)]
   pub trait PlexusRpc {
       #[method(name = "plexus_respond")]
       async fn respond(
           &self,
           subscription_id: String,
           request_id: String,
           response_data: Value,
       ) -> Result<(), Error>;
   }

   impl PlexusRpcServer for PlexusRpcImpl {
       async fn respond(
           &self,
           subscription_id: String,
           request_id: String,
           response_data: Value,
       ) -> Result<(), Error> {
           let subscription = self.subscriptions.lock().unwrap()
               .get(&subscription_id)
               .ok_or_else(|| Error::internal_error("Unknown subscription"))?;

           subscription.bidir_channel.handle_response(request_id, response_data)
               .map_err(|e| Error::internal_error(format!("Response error: {}", e)))?;

           Ok(())
       }
   }
   ```

3. **Subscription Context**:
   ```rust
   struct SubscriptionContext {
       subscription_id: String,
       bidir_channel: Arc<dyn Any + Send + Sync>,  // Type-erased for storage
       sink: Arc<Mutex<SubscriptionSink>>,
   }
   ```

**Success Criteria**:
- [ ] Request items flow through WebSocket subscription
- [ ] `plexus_respond` RPC method works
- [ ] Response correlation works
- [ ] Multiple concurrent subscriptions don't interfere
- [ ] Integration test with jsonrpsee client
- [ ] Works with both StandardBidirChannel and custom types

**Estimated Scope**: 2 days

**Dependencies**: WS3, WS4
**Unlocks**: WS8

---

### WS8: Example Interactive Activation (plexus-substrate)

**Objective**: Build complete interactive activations demonstrating bidirectional patterns

**Location**: `plexus-substrate/src/activations/interactive/`

**Components**:

1. **Standard Interactive Activation**:
   ```rust
   pub struct InteractiveActivation;

   #[hub_method(description = "Multi-step setup wizard", bidirectional)]
   pub async fn wizard(
       &self,
       ctx: &StandardBidirChannel,
   ) -> impl Stream<Item = WizardEvent> + Send + 'static {
       stream! {
           yield WizardEvent::Started;

           // Step 1: Get project name
           let name = match ctx.prompt("Enter project name:").await {
               Ok(n) => n,
               Err(BidirError::NotSupported) => {
                   yield WizardEvent::Error { message: "Interactive mode required".into() };
                   return;
               }
               Err(e) => {
                   yield WizardEvent::Error { message: bidir_error_message(&e) };
                   return;
               }
           };
           yield WizardEvent::NameCollected { name: name.clone() };

           // Step 2: Select template
           let template = ctx.select(
               "Choose template:",
               vec![
                   SelectOption {
                       value: "minimal".into(),
                       label: "Minimal".into(),
                       description: Some("Bare-bones starter".into()),
                   },
                   SelectOption {
                       value: "full".into(),
                       label: "Full".into(),
                       description: Some("All features included".into()),
                   },
               ],
           ).await.unwrap_or_default();
           yield WizardEvent::TemplateSelected { template: template[0].clone() };

           // Step 3: Confirm
           if !ctx.confirm(&format!("Create '{}' with '{}' template?", name, template[0])).await.unwrap_or(false) {
               yield WizardEvent::Cancelled;
               return;
           }

           yield WizardEvent::Created { name, template: template[0].clone() };
           yield WizardEvent::Done;
       }
   }

   #[hub_method(description = "Delete files with confirmation", bidirectional)]
   pub async fn delete(
       &self,
       ctx: &StandardBidirChannel,
       paths: Vec<String>,
   ) -> impl Stream<Item = DeleteEvent> + Send + 'static {
       stream! {
           if !ctx.confirm(&format!("Delete {} files?", paths.len())).await.unwrap_or(false) {
               yield DeleteEvent::Cancelled;
               return;
           }

           for path in paths {
               // Simulate deletion
               yield DeleteEvent::Deleted { path };
           }

           yield DeleteEvent::Done;
       }
   }
   ```

2. **Custom Type Activation**:
   ```rust
   #[derive(Serialize, Deserialize, JsonSchema)]
   pub enum ImageRequest {
       ConfirmOverwrite { path: String },
       ChooseQuality { options: Vec<u8> },
   }

   #[derive(Serialize, Deserialize, JsonSchema)]
   pub enum ImageResponse {
       Confirmed(bool),
       Quality(u8),
   }

   #[hub_method(
       description = "Process images with quality selection",
       bidirectional(request = "ImageRequest", response = "ImageResponse")
   )]
   pub async fn process_images(
       &self,
       ctx: &BidirChannel<ImageRequest, ImageResponse>,
       paths: Vec<String>,
   ) -> impl Stream<Item = ProcessEvent> + Send + 'static {
       stream! {
           for path in paths {
               let quality = ctx.request(ImageRequest::ChooseQuality {
                   options: vec![80, 90, 100],
               }).await;

               match quality {
                   Ok(ImageResponse::Quality(q)) => {
                       yield ProcessEvent::Processed { path, quality: q };
                   }
                   Err(e) => {
                       yield ProcessEvent::Error { path, message: bidir_error_message(&e) };
                   }
                   _ => {
                       yield ProcessEvent::Skipped { path };
                   }
               }
           }

           yield ProcessEvent::Done;
       }
   }
   ```

3. **Event Types**:
   ```rust
   #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
   pub enum WizardEvent {
       Started,
       NameCollected { name: String },
       TemplateSelected { template: String },
       Created { name: String, template: String },
       Cancelled,
       Error { message: String },
       Done,
   }

   #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
   pub enum DeleteEvent {
       Deleted { path: String },
       Cancelled,
       Done,
   }

   #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
   pub enum ProcessEvent {
       Processed { path: String, quality: u8 },
       Skipped { path: String },
       Error { path: String, message: String },
       Done,
   }
   ```

**Success Criteria**:
- [ ] Activations compile and run
- [ ] Demonstrates all request types (confirm, prompt, select)
- [ ] Shows error handling patterns
- [ ] Shows fallback/degradation patterns
- [ ] Works over both MCP and WebSocket
- [ ] Can be called via plexus-protocol Haskell client
- [ ] Can be called via synapse CLI
- [ ] Custom types work end-to-end

**Estimated Scope**: 2 days

**Dependencies**: WS6, WS7
**Unlocks**: WS9, WS10, WS11

---

### WS9: Test Coverage (All Repos)

**Objective**: Comprehensive testing of bidirectional functionality

**Test Categories**:

1. **plexus-core Unit Tests** (`plexus-core/tests/bidirectional_tests.rs`):
   ```rust
   #[tokio::test]
   async fn test_generic_request_response() {
       #[derive(Serialize, Deserialize, PartialEq, Debug)]
       enum TestReq { Ask { question: String } }

       #[derive(Serialize, Deserialize, PartialEq, Debug)]
       enum TestResp { Answer { text: String } }

       let (channel, mut rx) = create_test_bidir_channel::<TestReq, TestResp>();

       let req_handle = tokio::spawn(async move {
           channel.request(TestReq::Ask { question: "foo?".into() }).await
       });

       // Receive request
       let item = rx.recv().await.unwrap();
       if let PlexusStreamItem::Request { request_id, request_data, .. } = item {
           let req: TestReq = serde_json::from_value(request_data).unwrap();
           assert_eq!(req, TestReq::Ask { question: "foo?".into() });

           // Send response
           channel.handle_response(
               request_id,
               serde_json::to_value(TestResp::Answer { text: "bar".into() }).unwrap(),
           ).unwrap();
       }

       let resp = req_handle.await.unwrap().unwrap();
       assert_eq!(resp, TestResp::Answer { text: "bar".into() });
   }

   #[tokio::test]
   async fn test_type_mismatch() { ... }

   #[tokio::test]
   async fn test_timeout() { ... }

   #[tokio::test]
   async fn test_concurrent_requests() { ... }

   #[tokio::test]
   async fn test_fallback_pattern() { ... }
   ```

2. **plexus-macros Integration Tests**:
   - [ ] Bidirectional macro expansion
   - [ ] Code generation correctness
   - [ ] Schema generation with custom types
   - [ ] Backward compatibility

3. **plexus-substrate Integration Tests**:
   ```rust
   #[tokio::test]
   async fn test_mcp_bidirectional_end_to_end() {
       // Start MCP server
       // Call interactive activation
       // Verify request notification sent
       // Send response via _plexus_respond tool
       // Verify activation continues correctly
   }

   #[tokio::test]
   async fn test_websocket_bidirectional_end_to_end() {
       // Start WebSocket server
       // Subscribe to interactive method
       // Verify request in subscription stream
       // Call plexus_respond RPC
       // Verify activation continues correctly
   }

   #[tokio::test]
   async fn test_multiple_concurrent_requests() { ... }

   #[tokio::test]
   async fn test_timeout_scenario() { ... }

   #[tokio::test]
   async fn test_cancellation_scenario() { ... }
   ```

4. **Cross-Language Tests**:
   - [ ] Haskell client (plexus-protocol) can respond to requests
   - [ ] TypeScript client can respond to requests
   - [ ] synapse CLI handles requests

**Success Criteria**:
- [ ] >90% code coverage for bidirectional code
- [ ] All edge cases tested
- [ ] No flaky tests
- [ ] CI passes
- [ ] Tests document expected behavior

**Estimated Scope**: 2 days

**Dependencies**: WS8
**Unlocks**: Release

---

### WS10: Documentation (All Repos)

**Objective**: Complete documentation for bidirectional functionality

**Documentation Items**:

1. **Architecture Doc** (`plexus-substrate/docs/architecture/<timestamp>_generic-bidirectional-streaming.md`):
   - [ ] Design decisions and rationale
   - [ ] Generic vs. fixed type approach explanation
   - [ ] Protocol flow diagrams
   - [ ] Transport mappings (MCP and WebSocket)
   - [ ] Usage patterns (standard and custom types)
   - [ ] Error handling guide
   - [ ] Type safety guarantees

2. **API Documentation** (Rustdoc):
   ```rust
   /// Generic bidirectional channel for type-safe server-to-client requests.
   ///
   /// # Type Parameters
   ///
   /// * `Req` - The request type sent from server to client
   /// * `Resp` - The response type sent from client to server
   ///
   /// # Examples
   ///
   /// ## Using StandardBidirChannel
   ///
   /// ```
   /// use plexus_core::StandardBidirChannel;
   ///
   /// #[hub_method(bidirectional)]
   /// async fn my_method(ctx: &StandardBidirChannel) {
   ///     if ctx.confirm("Continue?").await? {
   ///         // ...
   ///     }
   /// }
   /// ```
   ///
   /// ## Using Custom Types
   ///
   /// ```
   /// #[derive(Serialize, Deserialize, JsonSchema)]
   /// enum MyRequest { Ask { question: String } }
   ///
   /// #[derive(Serialize, Deserialize, JsonSchema)]
   /// enum MyResponse { Answer { text: String } }
   ///
   /// #[hub_method(bidirectional(request = "MyRequest", response = "MyResponse"))]
   /// async fn my_method(ctx: &BidirChannel<MyRequest, MyResponse>) {
   ///     let resp = ctx.request(MyRequest::Ask { question: "foo?" }).await?;
   ///     // ...
   /// }
   /// ```
   pub struct BidirChannel<Req, Resp> { ... }
   ```

3. **User Guides**:
   - [ ] **Getting Started**: How to write first bidirectional activation
   - [ ] **Standard Patterns**: Using StandardBidirChannel for confirm/prompt/select
   - [ ] **Custom Types**: Defining domain-specific request/response types
   - [ ] **Error Handling**: Handling BidirError variants
   - [ ] **Testing Guide**: How to test bidirectional methods
   - [ ] **Transport Considerations**: MCP vs WebSocket differences

4. **README Updates**:
   - [ ] `plexus-core/README.md`: Mention bidirectional support
   - [ ] `plexus-substrate/README.md`: Example bidirectional activation
   - [ ] `plexus-transport/README.md`: Document bidirectional protocol
   - [ ] `plexus-macros/README.md`: Document bidirectional macro syntax

**Success Criteria**:
- [ ] All public APIs documented
- [ ] Examples compile and run
- [ ] Architecture doc complete
- [ ] User can implement bidirectional method from docs alone
- [ ] Migration guide for upgrading from unidirectional

**Estimated Scope**: 1.5 days

**Dependencies**: WS8, WS9

---

### WS11: Haskell Client Library Updates (plexus-protocol)

**Objective**: Update plexus-protocol to support bidirectional requests from Haskell clients

**Location**: `plexus-protocol/src/Plexus/`

**Components**:

1. **Bidirectional Types**:
   ```haskell
   -- Core bidirectional types in Plexus/Types.hs
   data BidirError
     = BidirNotSupported
     | BidirTimeout
     | BidirCancelled
     | BidirTypeMismatch Text Text
     | BidirSerializationError Text
     | BidirTransportError Text
     deriving (Show, Eq, Generic)

   -- Standard request/response types for convenience
   data StandardRequest
     = Confirm { confirmMessage :: Text, confirmDefault :: Maybe Bool }
     | Prompt { promptMessage :: Text, promptDefault :: Maybe Text, promptPlaceholder :: Maybe Text }
     | Select { selectMessage :: Text, selectOptions :: [SelectOption], selectMulti :: Bool }
     deriving (Show, Eq, Generic)

   data StandardResponse
     = Confirmed Bool
     | TextResponse Text
     | Selected [Text]
     | Cancelled
     deriving (Show, Eq, Generic)

   data SelectOption = SelectOption
     { optionValue :: Text
     , optionLabel :: Text
     , optionDescription :: Maybe Text
     } deriving (Show, Eq, Generic)

   instance ToJSON StandardRequest
   instance FromJSON StandardRequest
   instance ToJSON StandardResponse
   instance FromJSON StandardResponse
   ```

2. **Client-Side Request Handler**:
   ```haskell
   -- Handler for bidirectional requests in Plexus/Client.hs
   type BidirHandler m req resp = req -> m (Either BidirError resp)

   -- Modify substrateRpc to accept optional bidirectional handler
   substrateRpcBidir
     :: (FromJSON req, ToJSON resp)
     => SubstrateConnection
     -> Text                                        -- Method name
     -> Value                                       -- Parameters
     -> Maybe (BidirHandler IO req resp)           -- Optional bidir handler
     -> Stream (Of PlexusStreamItem) IO ()

   -- When stream yields Request item:
   --   1. Deserialize request_data to req type
   --   2. Call handler to get response
   --   3. Send response via _plexus_respond or plexus_respond RPC
   ```

3. **Response Mechanism**:
   ```haskell
   -- Send response back to server in Plexus/Transport.hs
   sendBidirResponse
     :: SubstrateConnection
     -> Text        -- request_id
     -> Value       -- response_data
     -> IO ()

   -- For MCP transport:
   --   Call _plexus_respond tool
   -- For WebSocket transport:
   --   Call plexus_respond RPC method
   ```

4. **Integration with Existing API**:
   ```haskell
   -- Backward compatible: existing functions continue to work
   substrateRpc :: SubstrateConnection -> Text -> Value -> Stream (Of PlexusStreamItem) IO ()
   substrateRpc conn method params = substrateRpcBidir conn method params Nothing

   -- New: bidirectional-aware version
   -- Example usage in synapse:
   let handler = \req -> case req of
         Confirm msg _ -> do
           putStrLn $ "Confirm: " <> msg <> " [y/N]"
           response <- getLine
           pure $ Right $ Confirmed (response == "y")
         Prompt msg _ _ -> do
           putStrLn $ "Prompt: " <> msg
           response <- getLine
           pure $ Right $ TextResponse (T.pack response)
         Select msg opts _ -> do
           putStrLn $ "Select: " <> msg
           forM_ (zip [1..] opts) $ \(i, opt) ->
             putStrLn $ show i <> ". " <> optionLabel opt
           choice <- getLine
           let idx = read choice - 1
           pure $ Right $ Selected [optionValue (opts !! idx)]

   stream <- substrateRpcBidir conn "wizard" params (Just handler)
   ```

**Success Criteria**:
- [ ] BidirError types defined
- [ ] StandardRequest/StandardResponse types defined
- [ ] `substrateRpcBidir` function works with handlers
- [ ] Response sending works for both MCP and WebSocket
- [ ] Backward compatible with existing `substrateRpc`
- [ ] Unit tests for request/response flow
- [ ] Documentation with examples

**Estimated Scope**: 2 days

**Dependencies**: WS2, WS6, WS7
**Unlocks**: WS13 (synapse CLI)

---

### WS12: TypeScript Client Code Generation (hub-codegen)

**Objective**: Generate TypeScript client code with bidirectional support

**Location**: `hub-codegen/src/generators/typescript/`

**Components**:

1. **Bidirectional Type Generation**:
   ```typescript
   // Generated from ImageRequest/ImageResponse schemas
   export type ImageRequest =
     | { ConfirmOverwrite: { path: string } }
     | { ChooseQuality: { options: number[] } }

   export type ImageResponse =
     | { Confirmed: boolean }
     | { Quality: number }
   ```

2. **Method Signature Generation**:
   ```typescript
   // Generated client method for bidirectional activation
   export interface ProcessImagesMethod {
     /**
      * Process images with quality selection
      * @param paths - Image file paths to process
      * @param options - Bidirectional handler options
      */
     processImages(
       paths: string[],
       options: {
         onRequest: (req: ImageRequest) => Promise<ImageResponse>
       }
     ): AsyncIterable<ProcessEvent>
   }

   // Standard bidirectional method
   export interface WizardMethod {
     wizard(
       options?: {
         onRequest?: (req: StandardRequest) => Promise<StandardResponse>
       }
     ): AsyncIterable<WizardEvent>
   }
   ```

3. **Client Implementation Generation**:
   ```typescript
   // Generated implementation
   class PlexusClient {
     async *processImages(
       paths: string[],
       options: { onRequest: (req: ImageRequest) => Promise<ImageResponse> }
     ): AsyncIterable<ProcessEvent> {
       const subscription = await this.subscribe('process_images', { paths })

       for await (const item of subscription) {
         if (item.type === 'request') {
           // Handle bidirectional request
           const request = item.request_data as ImageRequest
           const response = await options.onRequest(request)

           // Send response back
           await this.respond(subscription.id, item.request_id, response)
         } else if (item.type === 'data') {
           yield item.data as ProcessEvent
         } else if (item.type === 'done' || item.type === 'error') {
           return
         }
       }
     }
   }
   ```

4. **Standard Handler Helpers**:
   ```typescript
   // Helper for standard interactive patterns
   export class TerminalBidirHandler {
     async handleRequest(req: StandardRequest): Promise<StandardResponse> {
       if ('Confirm' in req) {
         const answer = await this.confirm(req.Confirm.message)
         return { Confirmed: answer }
       }
       if ('Prompt' in req) {
         const text = await this.prompt(req.Prompt.message)
         return { TextResponse: text }
       }
       if ('Select' in req) {
         const selected = await this.select(req.Select.message, req.Select.options)
         return { Selected: selected }
       }
       return { Cancelled: null }
     }

     private async confirm(message: string): Promise<boolean> { /* ... */ }
     private async prompt(message: string): Promise<string> { /* ... */ }
     private async select(message: string, options: SelectOption[]): Promise<string[]> { /* ... */ }
   }
   ```

**Success Criteria**:
- [ ] TypeScript types generated for custom request/response enums
- [ ] Method signatures include bidirectional handler parameter
- [ ] Generated client code handles Request stream items
- [ ] Response mechanism works for both MCP and WebSocket
- [ ] Standard handler helpers provided
- [ ] Documentation generated with usage examples
- [ ] Tests verify generated code compiles and works

**Estimated Scope**: 2 days

**Dependencies**: WS4, WS12 (schema generation)
**Unlocks**: None (TypeScript clients can use bidirectional)

---

### WS13: synapse CLI Interactive Handler (synapse)

**Objective**: Implement interactive bidirectional request handling in synapse CLI

**Location**: `synapse/src/Synapse/`

**Components**:

1. **Interactive Handler Implementation**:
   ```haskell
   -- In Synapse/Bidir.hs (new module)
   module Synapse.Bidir where

   import qualified Plexus.Types as PT

   -- Terminal-based interactive handler for StandardRequest
   terminalBidirHandler :: PT.BidirHandler IO PT.StandardRequest PT.StandardResponse
   terminalBidirHandler req = case req of
     PT.Confirm msg defaultVal -> do
       let prompt = case defaultVal of
             Just True  -> msg <> " [Y/n] "
             Just False -> msg <> " [y/N] "
             Nothing    -> msg <> " [y/n] "
       liftIO $ putStr prompt
       liftIO $ hFlush stdout
       response <- liftIO getLine
       let answer = case T.toLower (T.strip (T.pack response)) of
             "y" -> True
             "yes" -> True
             "n" -> False
             "no" -> False
             "" -> fromMaybe False defaultVal
             _ -> fromMaybe False defaultVal
       pure $ Right $ PT.Confirmed answer

     PT.Prompt msg defaultVal placeholder -> do
       case placeholder of
         Just ph -> liftIO $ putStrLn $ "  (" <> ph <> ")"
         Nothing -> pure ()
       liftIO $ putStr $ msg <> " "
       case defaultVal of
         Just def -> liftIO $ putStr $ "[" <> def <> "] "
         Nothing -> pure ()
       liftIO $ hFlush stdout
       response <- liftIO getLine
       let text = if T.null (T.strip (T.pack response))
                  then fromMaybe "" defaultVal
                  else T.pack response
       pure $ Right $ PT.TextResponse text

     PT.Select msg opts multiSelect -> do
       liftIO $ putStrLn msg
       forM_ (zip [1..] opts) $ \(i, opt) -> do
         let desc = case PT.optionDescription opt of
               Just d -> " - " <> d
               Nothing -> ""
         liftIO $ putStrLn $ "  " <> show (i :: Int) <> ". " <> PT.optionLabel opt <> desc

       liftIO $ putStr $ if multiSelect
         then "Select (comma-separated): "
         else "Select: "
       liftIO $ hFlush stdout

       response <- liftIO getLine
       let indices = if multiSelect
             then map (read . T.unpack . T.strip) $ T.splitOn "," (T.pack response)
             else [read response]
       let selected = map (\i -> PT.optionValue (opts !! (i - 1))) indices
       pure $ Right $ PT.Selected selected
   ```

2. **Auto Handler Implementation**:
   ```haskell
   -- Auto-confirm handler (for --auto-confirm flag)
   autoBidirHandler :: Bool -> PT.BidirHandler IO PT.StandardRequest PT.StandardResponse
   autoBidirHandler autoConfirm req = case req of
     PT.Confirm msg defaultVal -> do
       let answer = if autoConfirm then True else fromMaybe False defaultVal
       liftIO $ putStrLn $ msg <> " [auto: " <> if answer then "yes" else "no" <> "]"
       pure $ Right $ PT.Confirmed answer

     PT.Prompt msg defaultVal _ -> do
       let text = fromMaybe "" defaultVal
       liftIO $ putStrLn $ msg <> " [auto: " <> text <> "]"
       pure $ Right $ PT.TextResponse text

     PT.Select msg opts _ -> do
       let selected = [PT.optionValue (head opts)]
       liftIO $ putStrLn $ msg <> " [auto: " <> head selected <> "]"
       pure $ Right $ PT.Selected selected
   ```

3. **CLI Integration**:
   ```haskell
   -- In Synapse/CLI.hs
   data SynapseOpts = SynapseOpts
     { optsBackend :: Text
     , optsMethod :: Text
     , optsParams :: Value
     , optsBidirMode :: BidirMode  -- NEW
     } deriving (Show)

   data BidirMode
     = BidirInteractive      -- Present prompts to user
     | BidirAutoConfirm      -- Auto-confirm all requests
     | BidirNone             -- Error on bidirectional requests
     deriving (Show, Eq)

   -- Parse --interactive and --auto-confirm flags
   bidirModeParser :: Parser BidirMode
   bidirModeParser =
     flag' BidirInteractive (long "interactive" <> help "Enable interactive bidirectional requests")
     <|> flag' BidirAutoConfirm (long "auto-confirm" <> help "Auto-confirm all bidirectional requests")
     <|> pure BidirNone

   -- Main execution with bidirectional support
   runMethod :: SynapseOpts -> IO ()
   runMethod opts = do
     conn <- connect (defaultConfig (optsBackend opts))

     let handler = case optsBidirMode opts of
           BidirInteractive -> Just terminalBidirHandler
           BidirAutoConfirm -> Just (autoBidirHandler True)
           BidirNone -> Nothing

     stream <- substrateRpcBidir conn (optsMethod opts) (optsParams opts) handler

     S.mapM_ print stream
   ```

4. **Usage Examples**:
   ```bash
   # Interactive mode - prompts user for all requests
   synapse plexus wizard --interactive

   # Auto-confirm mode - automatically confirms all requests
   synapse plexus delete --paths file1,file2,file3 --auto-confirm

   # Non-interactive mode - errors on bidirectional requests
   synapse plexus wizard  # Error: "Method requires --interactive or --auto-confirm"
   ```

**Success Criteria**:
- [ ] Terminal-based interactive handler works for StandardRequest
- [ ] Auto handler works with --auto-confirm flag
- [ ] CLI flags (--interactive, --auto-confirm) parsed correctly
- [ ] Error message when bidirectional method called without handler
- [ ] Works with both MCP and WebSocket transports
- [ ] User experience is intuitive and responsive
- [ ] Documentation with examples

**Estimated Scope**: 2 days

**Dependencies**: WS11 (plexus-protocol)
**Unlocks**: WS14 (pipe support)

---

### WS14: synapse Pipe Support (synapse)

**Objective**: Enable Unix-style pipes in synapse with bidirectional support

**Location**: `synapse/src/Synapse/Pipe.hs` (new module)

**Design**: Allow `synapse cmd1 | synapse cmd2` where cmd2's input type matches cmd1's output type

**Components**:

1. **Stream Piping in plexus-core**:
   ```rust
   /// Extension trait for composing activations
   pub trait StreamPipe: Stream {
       /// Pipe this stream into an activation that may make bidirectional requests
       fn pipe_bidir<Req, Resp, Out>(
           self,
           activation_method: impl Future<Output = impl Stream<Item = Out>>,
           bidir_handler: impl BidirHandler<Req, Resp>,
       ) -> impl Stream<Item = Out>
       where
           Self: Sized,
           Self::Item: Serialize,
           Req: Serialize + DeserializeOwned,
           Resp: Serialize + DeserializeOwned,
           Out: DeserializeOwned;
   }
   ```

2. **Bidirectional Request Handler**:
   ```rust
   /// Trait for handling bidirectional requests in a pipe
   pub trait BidirHandler<Req, Resp>: Send + Sync {
       async fn handle(&self, req: Req) -> Result<Resp, BidirError>;
   }

   /// Auto-handler (always returns default response)
   pub struct AutoBidirHandler<Req, Resp> {
       default_fn: Box<dyn Fn(&Req) -> Resp + Send + Sync>,
   }

   /// Interactive handler (prompts user via terminal/UI)
   pub struct InteractiveBidirHandler<Req, Resp> {
       renderer: Box<dyn RequestRenderer<Req> + Send + Sync>,
       parser: Box<dyn ResponseParser<Resp> + Send + Sync>,
   }
   ```

3. **synapse CLI Pipe Support**:
   ```bash
   # Simple pipe (no bidirectional interaction)
   synapse list-repos | synapse clone-repos

   # Interactive pipe (synapse handles all bidirectional requests)
   synapse generate-images --prompt "cats" | synapse watermark-images --interactive

   # Auto-confirm pipe (all requests auto-answered)
   synapse list-repos --include-archived | synapse delete-repos --auto-confirm
   ```

4. **Type Checking**:
   ```rust
   /// Validate pipe compatibility before executing
   async fn validate_pipe_compatibility(
       cmd1_schema: &MethodSchema,
       cmd2_schema: &MethodSchema,
   ) -> Result<(), PipeError> {
       let output_type = &cmd1_schema.output.item_type;
       let input_type = &cmd2_schema.input.item_type;

       if output_type != input_type {
           return Err(PipeError::TypeMismatch {
               produced: output_type.clone(),
               expected: input_type.clone(),
               suggestion: format!(
                   "Command '{}' outputs {}, but '{}' expects {}",
                   cmd1_schema.name, output_type,
                   cmd2_schema.name, input_type
               ),
           });
       }

       Ok(())
   }
   ```

**Success Criteria**:
- [ ] Stream composition works with matching types
- [ ] Type mismatches caught with clear error messages
- [ ] Bidirectional requests routed to correct handler
- [ ] synapse CLI can parse and execute pipe expressions
- [ ] Interactive handler presents requests to user via CLI
- [ ] Auto handler provides sensible defaults
- [ ] Tests cover:
  - [ ] Simple unidirectional pipes
  - [ ] Bidirectional pipes with interactive handler
  - [ ] Bidirectional pipes with auto handler
  - [ ] Type mismatch errors
  - [ ] Multi-stage pipes (A | B | C)

**Estimated Scope**: 3 days

**Dependencies**: WS3, WS4, WS8
**Unlocks**: None (feature complete)

---

**Components**:

1. **Pipe Detection and Parsing**:
   ```haskell
   -- Parse shell pipe syntax
   data PipeExpression
     = SingleCommand SynapseOpts
     | Pipe SynapseOpts PipeExpression
     deriving (Show)

   -- Example: "synapse list-repos | synapse filter-repos | synapse clone-repos"
   -- Parses to: Pipe (list-repos) (Pipe (filter-repos) (SingleCommand clone-repos))
   ```

2. **Type Compatibility Checking**:
   ```haskell
   -- Fetch schemas and validate compatibility
   validatePipe :: SubstrateConnection -> PipeExpression -> IO (Either PipeError ())
   validatePipe conn (Pipe cmd1 rest) = do
     schema1 <- fetchSchema conn (optsMethod cmd1)
     schema2 <- case rest of
       SingleCommand cmd2 -> fetchSchema conn (optsMethod cmd2)
       Pipe cmd2 _ -> fetchSchema conn (optsMethod cmd2)

     -- Check output of cmd1 matches input of cmd2
     let output1 = methodOutputType schema1
     let input2 = methodInputType schema2

     if output1 /= input2
       then pure $ Left $ TypeMismatch {
         produced = output1,
         expected = input2,
         suggestion = "Output type mismatch in pipe"
       }
       else validatePipe conn rest

   validatePipe _ (SingleCommand _) = pure $ Right ()
   ```

3. **Pipe Execution**:
   ```haskell
   -- Execute pipe by streaming output of cmd1 to input of cmd2
   executePipe :: SubstrateConnection -> PipeExpression -> BidirMode -> IO ()
   executePipe conn expr bidirMode = case expr of
     SingleCommand cmd -> runMethod cmd { optsBidirMode = bidirMode }

     Pipe cmd1 rest -> do
       -- Start cmd1 stream
       stream1 <- substrateRpcBidir conn (optsMethod cmd1) (optsParams cmd1) handler

       -- Collect output items (filter out Progress, Error, Done)
       let dataItems = stream1
             & S.filter (\item -> case item of
                 StreamData{} -> True
                 _ -> False)
             & S.map (\(StreamData _ _ _ dat) -> dat)

       -- Feed into cmd2 as input
       let cmd2 = case rest of
             SingleCommand c -> c
             Pipe c _ -> c

       -- Convert stream to JSON array for params
       items <- S.toList_ dataItems
       let cmd2Params = object ["input" .= items]

       -- Execute cmd2 with cmd1's output
       executePipe conn (modifyParams rest cmd2Params) bidirMode

     where
       handler = case bidirMode of
         BidirInteractive -> Just terminalBidirHandler
         BidirAutoConfirm -> Just (autoBidirHandler True)
         BidirNone -> Nothing
   ```

4. **CLI Integration**:
   ```bash
   # Pipe syntax examples

   # Simple pipe (no bidirectional)
   synapse plexus list-repos | synapse plexus clone-repos

   # Interactive pipe (user answers prompts from filter-repos)
   synapse plexus list-repos | synapse plexus filter-repos --interactive

   # Auto-confirm pipe
   synapse plexus list-repos | synapse plexus delete-repos --auto-confirm

   # Multi-stage pipe
   synapse plexus list-repos \
     | synapse plexus filter-repos --interactive \
     | synapse plexus clone-repos --auto-confirm
   ```

**Success Criteria**:
- [ ] Pipe expression parsing works
- [ ] Type compatibility checking works via schema
- [ ] Clear error messages for type mismatches
- [ ] Stream data flows from cmd1 to cmd2
- [ ] Bidirectional requests handled in pipe context
- [ ] Multi-stage pipes work (A | B | C)
- [ ] Tests cover:
  - [ ] Simple unidirectional pipes
  - [ ] Bidirectional pipes with interactive handler
  - [ ] Type mismatch errors
  - [ ] Multi-stage pipes

**Estimated Scope**: 3 days

**Dependencies**: WS13 (synapse interactive handler)
**Unlocks**: None (feature complete for client side)

---

### WS15: Schema Generation for Generic Types (plexus-core)

**Objective**: Generate JSON schemas for custom request/response types and enable client code generation

**Location**:
- `plexus-core/src/plexus/schema.rs`
- `hub-codegen/src/generators/typescript/bidirectional.ts`
- `plexus-protocol/codegen/` (new)

**Components**:

1. **Schemars Integration**:
   ```rust
   // Custom types must derive JsonSchema
   #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
   #[schemars(description = "Image processing request")]
   pub enum ImageRequest {
       #[schemars(description = "Confirm overwriting existing file")]
       ConfirmOverwrite { path: String },

       #[schemars(description = "Choose compression quality")]
       ChooseQuality {
           #[schemars(description = "Available quality levels (0-100)")]
           options: Vec<u8>,
       },
   }
   ```

2. **Schema Endpoint Enhancement**:
   ```rust
   // GET /schema/process_images
   {
     "method": "process_images",
     "description": "Process images with interactive quality selection",
     "bidirectional": {
       "enabled": true,
       "request_type": {
         "name": "ImageRequest",
         "schema": {
           "description": "Image processing request",
           "oneOf": [
             {
               "type": "object",
               "required": ["ConfirmOverwrite"],
               "properties": {
                 "ConfirmOverwrite": {
                   "description": "Confirm overwriting existing file",
                   "type": "object",
                   "required": ["path"],
                   "properties": { "path": { "type": "string" } }
                 }
               }
             },
             // ...
           ]
         }
       },
       "response_type": { /* ... */ }
     }
   }
   ```

3. **TypeScript Client Generation**:
   ```typescript
   // Generated from schema
   export type ImageRequest =
     | { ConfirmOverwrite: { path: string } }
     | { ChooseQuality: { options: number[] } }

   export type ImageResponse =
     | { Confirmed: boolean }
     | { Quality: number }

   export interface ProcessImagesMethod {
       input: AsyncIterable<ImageData>
       request: ImageRequest
       response: ImageResponse
       output: AsyncIterable<ProcessedImage>

       call(
           input: AsyncIterable<ImageData>,
           options: {
               onRequest: (req: ImageRequest) => Promise<ImageResponse>
           }
       ): AsyncIterable<ProcessedImage>
   }
   ```

4. **Haskell Client Generation**:
   ```haskell
   -- Generated from schema
   data ImageRequest
     = ConfirmOverwrite { path :: Text }
     | ChooseQuality { options :: [Word8] }
     deriving (Show, Eq, Generic)

   data ImageResponse
     = Confirmed Bool
     | Quality Word8
     deriving (Show, Eq, Generic)

   -- Client usage
   processImages
     :: BidirHandler IO ImageRequest ImageResponse
     -> Stream (Of ImageData) IO ()
     -> IO (Stream (Of ProcessedImage) IO ())
   ```

**Success Criteria**:
- [ ] Custom types with `#[derive(JsonSchema)]` have schemas generated
- [ ] Schema endpoint includes bidirectional type information
- [ ] TypeScript codegen creates correct types and client methods
- [ ] Haskell codegen creates correct types and client methods
- [ ] Runtime validation catches invalid requests/responses
- [ ] Documentation shows how to define custom types
- [ ] Tests cover:
  - [ ] Schema generation for various enum shapes
  - [ ] Schema validation
  - [ ] TypeScript client generation
  - [ ] Haskell client generation
  - [ ] Cross-language compatibility

**Estimated Scope**: 2 days

**Dependencies**: WS4
**Unlocks**: WS12 (TypeScript codegen)

---

## Dependency Graph

```
                  WS1 (Assess)
                       │
                       ▼
                  WS2 (Generic Types)
                       │
            ┌──────────┼──────────┐
            ▼          ▼          ▼
          WS3        WS4        WS5
       (Generic   (Generic   (Helpers)
        Channel)   Macro)
            │          │          │
            │          ▼          │
            │        WS15         │
            │      (Schema Gen)   │
            │          │          │
            │          ▼          │
            │        WS12         │
            │       (TS Codegen)  │
            │                     │
            └──────────┼──────────┘
                       │
            ┌──────────┴──────────┐
            ▼                     ▼
          WS6                   WS7
         (MCP)              (WebSocket)
            │                     │
            └──────────┬──────────┘
                       │
                ┌──────┴──────┐
                ▼             ▼
              WS8          WS11
           (Example)    (plexus-protocol)
                │             │
                ▼             ▼
              WS9          WS13
            (Tests)      (synapse CLI)
                │             │
                ▼             ▼
              WS10         WS14
             (Docs)     (synapse Pipes)
```

## Critical Path

**Minimum Viable** (basic server-side bidirectional):
```
WS1 → WS2 → WS3 → WS6 → WS8 → WS9 → WS10
```
Estimated: 12 days

**Full Server + Client Stack** (with generics + all clients):
```
Server Path:
WS1 → WS2 → WS3 → WS4 → WS15 → [WS6 + WS7] → WS8 → WS9 → WS10
                            │
                            └──→ WS12 (TypeScript codegen)

Client Path:
WS2 + WS6 + WS7 → WS11 (plexus-protocol) → WS13 (synapse CLI) → WS14 (pipes)
```
Estimated: 26 days (with parallelization: 18 days)

**Parallelization Opportunities**:
- WS6 (MCP) and WS7 (WebSocket) can be done in parallel
- WS12 (TypeScript) can be done parallel with WS8 (examples)
- WS11 (plexus-protocol) can start once WS2 is done
- WS13 (synapse) can be done parallel with WS8/WS9

## Success Criteria

**Phase 1: Generic Core Infrastructure**
- [ ] Generic BidirChannel<Req, Resp> works with any types
- [ ] StandardBidirChannel provides ergonomic defaults
- [ ] Custom domain types work (e.g., ImageRequest/ImageResponse)
- [ ] Macro generates correct code for all type scenarios
- [ ] Type safety enforced at compile time

**Phase 2: Transport Integration**
- [ ] MCP transport handles generic request/response types
- [ ] WebSocket transport handles generic request/response types
- [ ] JSON serialization works for custom types
- [ ] Can test with real clients

**Phase 3: Schema & Code Generation**
- [ ] Schema endpoint includes bidirectional type information
- [ ] JSON Schema generated for custom types
- [ ] TypeScript client generation works
- [ ] Haskell client generation works

**Phase 4: Examples & Validation**
- [ ] Examples demonstrate standard and custom types
- [ ] Test coverage >90%
- [ ] Documentation complete
- [ ] Production ready

**Phase 5: Client Libraries (WS11-WS13)**
- [ ] plexus-protocol supports bidirectional requests
- [ ] TypeScript clients have bidirectional handlers
- [ ] synapse CLI has interactive handler
- [ ] synapse CLI has auto-confirm mode
- [ ] Clear error messages when handler missing

**Phase 6: Piping & Composition (WS14)**
- [ ] synapse pipe parsing works
- [ ] Stream piping works with type checking
- [ ] synapse supports `cmd1 | cmd2` syntax
- [ ] Bidirectional requests routed correctly in pipes
- [ ] Multi-stage pipes work

**Overall System**
- [ ] Backward compatible with existing unidirectional methods
- [ ] Type-safe at compile time for activation authors
- [ ] Type-safe at runtime for transport layer
- [ ] Works across MCP and WebSocket transports
- [ ] Works across Haskell, TypeScript, and Rust clients
- [ ] Enables Unix-style composition of interactive activations

## Timeline

### Server-Side Only (WS1-WS10)
**Optimistic** (1 developer, focused work): 2.5 weeks
**Realistic** (1 developer, with other tasks): 4-5 weeks

### Full Stack (WS1-WS15)
**Optimistic** (1 developer, focused work): 5 weeks
**Realistic** (1 developer, with other tasks): 7-8 weeks
**With parallelization** (2 developers): 4 weeks
  - Developer 1: Server path (WS1-WS10, WS15)
  - Developer 2: Client path (WS11-WS14, parallel after WS2)

### By Workstream:
- WS1: 0.5 days (assessment)
- WS2: 1 day (generic types)
- WS3: 2 days (generic channel)
- WS4: 2 days (macro)
- WS5: 0.5 days (helpers)
- WS6: 2 days (MCP transport)
- WS7: 2 days (WebSocket transport)
- WS8: 2 days (examples)
- WS9: 2 days (tests)
- WS10: 1.5 days (docs)
- WS11: 2 days (plexus-protocol)
- WS12: 2 days (TypeScript codegen)
- WS13: 2 days (synapse CLI)
- WS14: 3 days (synapse pipes)
- WS15: 2 days (schema generation)

**Total**: 26 days sequential, ~18 days with parallelization

## Open Questions

1. **Type Erasure for Storage**: How to store `BidirChannel<Req, Resp>` with different generic parameters in transport layer state?
   - **Answer**: Store as `BidirChannel<Value, Value>` and rely on runtime deserialization

2. **Schema Evolution**: How to handle version compatibility when request/response types change?
   - **Answer**: Include version in schema, validate at runtime

3. **Client Libraries**: Should clients auto-generate handler stubs for custom types?
   - **Answer**: Yes, generate TypeScript/Haskell interfaces with handler signatures

4. **Pipe Type Checking**: Runtime vs compile-time type checking for pipes?
   - **Answer**: Runtime via schema comparison in synapse CLI

5. **Error Recovery**: What happens if bidirectional request fails mid-stream?
   - **Answer**: Activation receives BidirError, can yield error event and continue or abort

## Next Steps

1. **Immediate**: Execute WS1 (assessment) to understand current state
2. **After WS1**: Begin WS2 (generic types) implementation
3. **Communication**: Notify users of plexus-protocol and synapse about upcoming bidirectional support

## Migration from Original BIDIR Plans

This plan **supersedes** BIDIR-1 through BIDIR-10 with the following key changes:

| Original | Generic-First |
|----------|--------------|
| Fixed RequestType/ResponsePayload enums | Generic BidirChannel<Req, Resp> |
| All activations share same request types | Each activation can define custom types |
| Basic UI patterns only | Standard patterns + domain-specific types |
| No composition support | Type-safe stream piping |
| Manual client integration | Auto-generated client code |

Existing architecture docs remain valid for protocol flow and transport mappings - the enhancement is in the type system.
