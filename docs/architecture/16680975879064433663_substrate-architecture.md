# Substrate Architecture

## Overview

Substrate is a self-documenting JSON-RPC 2.0 server that provides a unified interface for multiple capabilities (activations). It implements a layered architecture where communication concerns are separated from business logic.

```
┌─────────────────────────────────────────────────────────────┐
│  Substrate (Communication Layer)                            │
│  - Speaks JSON-RPC 2.0 over WebSocket                       │
│  - Handles parse errors, protocol guidance                  │
│  - Knows HOW to communicate                                 │
├─────────────────────────────────────────────────────────────┤
│  Plexus (Business Logic Layer)                              │
│  - Routes to activations                                    │
│  - Knows WHAT is available (schema, methods)                │
│  - No knowledge of wire format                              │
├─────────────────────────────────────────────────────────────┤
│  Activations (Capabilities)                                 │
│  - health, bash, arbor, cone                                │
│  - Pure domain logic                                        │
└─────────────────────────────────────────────────────────────┘
```

**Key insight**: The plexus is ON the substrate. The substrate makes communication possible.

## Project Structure

```
src/
├── main.rs                 # RPC server entry point
├── lib.rs                  # Library exports
├── plexus/                 # Routing & coordination layer
│   ├── plexus.rs          # Plexus struct & Activation trait
│   ├── path.rs            # Provenance tracking for call chains
│   ├── types.rs           # PlexusStreamItem unified stream type
│   └── schema.rs          # Self-documenting RPC schemas
├── activations/           # Capabilities / domain logic
│   ├── health/            # System health checker
│   ├── bash/              # Shell command executor
│   ├── arbor/             # Tree-based context storage
│   └── cone/              # LLM conversation manager
└── plugin_system/         # RPC integration layer
    ├── types.rs           # ActivationStreamItem trait
    └── conversion.rs      # IntoSubscription for RPC streaming
```

## Layer Details

### Substrate Layer (Communication)

**File**: `src/main.rs`

The substrate is the communication foundation—a WebSocket server speaking JSON-RPC 2.0:

```rust
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let plexus = Plexus::new()
        .register(Health::new())
        .register(Bash::new())
        .register(Arbor::new(ArborConfig::default()).await?)
        .register(Cone::new(ConeStorageConfig::default()).await?);

    let module = plexus.into_rpc_module()?;
    let server = Server::builder().build("127.0.0.1:4444").await?;
    server.start(module);
}
```

Responsibilities:
- WebSocket server on `ws://127.0.0.1:4444`
- JSON-RPC 2.0 protocol handling via `jsonrpsee`
- Protocol-level error responses with guidance

### Plexus Layer (Routing & Discovery)

**File**: `src/plexus/plexus.rs`

The plexus is the nerve center that routes calls and provides schema discovery:

```rust
pub struct Plexus {
    activations: HashMap<String, Arc<dyn Activation>>,
    pending_rpc: Vec<Box<dyn FnOnce() -> Methods + Send>>,
}

impl Plexus {
    pub fn register<A: Activation + Clone>(self, activation: A) -> Self;
    pub async fn call(&self, method: &str, params: Value) -> Result<PlexusStream, PlexusError>;
    pub fn into_rpc_module(self) -> Result<RpcModule<()>>;
}
```

Key operations:
- **Registration**: Activations register at startup with their namespace
- **Routing**: Parses `namespace.method` and dispatches to the correct activation
- **Schema Discovery**: `plexus_schema` subscription returns all activations and methods
- **RPC Conversion**: Converts all activations to JSON-RPC methods

### Activation Trait

Every capability implements the `Activation` trait:

```rust
#[async_trait]
pub trait Activation: Send + Sync + 'static {
    fn namespace(&self) -> &str;          // e.g., "health"
    fn version(&self) -> &str;            // e.g., "1.0.0"
    fn description(&self) -> &str;
    fn methods(&self) -> Vec<&str>;       // e.g., ["check", "execute"]
    fn method_help(&self, method: &str) -> Option<String>;
    fn enrich_schema(&self) -> Schema;    // Self-documenting schema

    async fn call(&self, method: &str, params: Value)
        -> Result<PlexusStream, PlexusError>;

    fn into_rpc_methods(self) -> Methods;
}
```

### PlexusError

**File**: `src/plexus/plexus.rs`

Semantic errors without RPC knowledge:

```rust
pub enum PlexusError {
    ActivationNotFound(String),
    MethodNotFound { activation: String, method: String },
    InvalidParams(String),
    ExecutionError(String),
}
```

These are converted to guided JSON-RPC errors by a thin wrapper (see [Self-Documenting RPC](./16680998353176467711_self-documenting-rpc.md)).

## Activations

### Health

**Namespace**: `health`
**Methods**: `check`
**Purpose**: System health and uptime monitoring

```rust
pub struct Health {
    start_time: Instant,
}

// Emits: HealthEvent { status, uptime_seconds, timestamp }
```

### Bash

**Namespace**: `bash`
**Methods**: `execute`
**Purpose**: Shell command execution with streaming output

```rust
pub struct Bash {
    executor: BashExecutor,
}

// Emits: BashEvent::Stdout | BashEvent::Stderr | BashEvent::Exit
```

The executor spawns bash processes and streams stdout/stderr line-by-line.

### Arbor

**Namespace**: `arbor`
**Methods**: `tree_create`, `tree_get`, `tree_list`, `node_create_text`, `node_create_branch`, `node_get`, `node_list`, etc.
**Purpose**: Tree-based context storage with ownership and lifecycle management

```rust
pub struct Arbor {
    storage: Arc<ArborStorage>,
}
```

Features:
- SQLite-based persistence
- Reference counting for ownership
- Configurable cleanup windows (scheduled → archived → purged)
- Background cleanup task

Core types:
```rust
pub struct Tree {
    pub id: TreeId,
    pub root_node_id: NodeId,
    pub metadata: Option<Value>,
    pub ref_count: i64,
    pub state: ResourceState,  // Active, ScheduledForDeletion, Archived
}

pub struct Node {
    pub id: NodeId,
    pub tree_id: TreeId,
    pub node_type: NodeType,   // Text, Branch
    pub parent_id: Option<NodeId>,
    pub children: Vec<NodeId>,
    pub content: Option<String>,
}
```

### Cone

**Namespace**: `cone`
**Methods**: `create`, `get`, `list`, `delete`, `chat`, `set_head`, `registry`
**Purpose**: LLM conversation manager using Arbor for context storage

```rust
pub struct Cone {
    storage: Arc<ConeStorage>,
    llm_registry: Arc<ModelRegistry>,
}
```

Key concepts:
- A **Cone** is a conversation with an LLM
- Uses **Arbor trees** to maintain conversation context
- Maintains a **canonical head** position in the tree
- Supports branching conversations by setting head to different nodes

## Streaming Architecture

### PlexusStreamItem (Unified Stream Type)

**File**: `src/plexus/types.rs`

All activations produce streams that convert to this unified type:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", content = "data")]
pub enum PlexusStreamItem {
    Progress {
        provenance: Provenance,
        message: String,
        percentage: Option<f32>,
    },
    Data {
        provenance: Provenance,
        content_type: String,        // e.g., "bash.event"
        data: serde_json::Value,
    },
    Error {
        provenance: Provenance,
        error: String,
        recoverable: bool,
    },
    Done {
        provenance: Provenance,
    },
}
```

### Provenance (Call Chain Tracking)

**File**: `src/plexus/path.rs`

Tracks the chain of custody through nested activation calls:

```rust
pub struct Provenance {
    segments: Vec<String>,
}

impl Provenance {
    pub fn root(name: impl Into<String>) -> Self;
    pub fn extend(&self, name: impl Into<String>) -> Self;
}
```

Example:
- Direct call: `Provenance::root("bash")` → `["bash"]`
- Nested call: `bash.extend("subprocess")` → `["bash", "subprocess"]`

### ActivationStreamItem Trait

**File**: `src/plugin_system/types.rs`

Activation-specific stream items implement this trait:

```rust
pub trait ActivationStreamItem: Serialize + Send + 'static {
    fn content_type() -> &'static str;
    fn into_plexus_item(self, provenance: Provenance) -> PlexusStreamItem;
    fn is_terminal(&self) -> bool { false }
}
```

### IntoSubscription Trait

**File**: `src/plugin_system/conversion.rs`

Bridges activation streams to JSON-RPC subscriptions:

```rust
pub trait IntoSubscription: Send + 'static {
    type Item: ActivationStreamItem;

    async fn into_subscription(
        self,
        pending: PendingSubscriptionSink,
        provenance: Provenance,
    ) -> SubscriptionResult;
}
```

Blanket implementation converts any `Stream<Item = T: ActivationStreamItem>` to a JSON-RPC subscription.

## Schema System

**File**: `src/plexus/schema.rs`

The plexus provides rich JSON Schema with enrichment:

```rust
pub struct Schema {
    pub schema_version: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub schema_type: Option<serde_json::Value>,
    pub properties: Option<HashMap<String, SchemaProperty>>,
    pub required: Option<Vec<String>>,
    pub one_of: Option<Vec<Schema>>,
    pub defs: Option<HashMap<String, serde_json::Value>>,
}

pub trait Describe {
    fn describe(&self) -> Option<MethodEnrichment>;
}
```

Activations can enrich auto-derived schemas with additional metadata (e.g., `format: "uuid"` for UUID fields).

## Data Flow

```
Client Request
       │
       ▼
┌──────────────────────────────────────────────┐
│ Substrate: WebSocket + JSON-RPC 2.0          │
│ Parse request, validate protocol             │
└──────────────────────────────────────────────┘
       │
       ▼
┌──────────────────────────────────────────────┐
│ Plexus: Route to activation                  │
│ Parse "namespace.method", dispatch           │
└──────────────────────────────────────────────┘
       │
       ▼
┌──────────────────────────────────────────────┐
│ Activation: Execute domain logic             │
│ Return Stream<ActivationStreamItem>          │
└──────────────────────────────────────────────┘
       │
       ▼
┌──────────────────────────────────────────────┐
│ Plugin System: Convert to subscription       │
│ ActivationStreamItem → PlexusStreamItem      │
└──────────────────────────────────────────────┘
       │
       ▼
Client receives JSON-RPC subscription events
```

## Activation Reference

| Activation | Namespace | Methods | Storage |
|-----------|-----------|---------|---------|
| Health | `health` | `check` | In-memory |
| Bash | `bash` | `execute` | None (streaming) |
| Arbor | `arbor` | `tree_*`, `node_*` | SQLite |
| Cone | `cone` | `create`, `chat`, `set_head`, etc. | SQLite (via Arbor) |

## Key Files

| File | Purpose |
|------|---------|
| `src/main.rs` | Server startup, activation registration |
| `src/plexus/plexus.rs` | Plexus struct, Activation trait, routing |
| `src/plexus/types.rs` | PlexusStreamItem unified stream type |
| `src/plexus/path.rs` | Provenance tracking |
| `src/plexus/schema.rs` | JSON Schema types and enrichment |
| `src/plugin_system/types.rs` | ActivationStreamItem trait |
| `src/plugin_system/conversion.rs` | IntoSubscription trait |
| `src/activations/*/activation.rs` | Activation implementations |

## Related

- [Self-Documenting RPC](./16680998353176467711_self-documenting-rpc.md) - Error guidance design
