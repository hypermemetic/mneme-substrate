# Plexus Hash Cache Invalidation

## Summary

Every JSON-RPC response from substrate now includes a `plexus_hash` field at the top level. This enables frontends to detect schema changes during normal operation without extra round-trips.

## Response Format

All responses now have this structure:

```json
{
  "plexus_hash": "7a1a43920ee194e1",
  "type": "data",
  "provenance": ["arbor"],
  "content_type": "arbor.event",
  "data": { ... }
}
```

The `plexus_hash` is a 16-character hex string derived from all activation namespaces, versions, and method names.

## What Changes the Hash

- Activation added or removed
- Activation version changed
- Methods added/removed/renamed in any activation

## What Does NOT Change the Hash

- Schema enrichment details (format, description, required fields)
- Internal activation implementation changes
- Data content changes

## Frontend Integration

```haskell
-- Check hash on every response
handleResponse :: Response -> IO ()
handleResponse resp = do
    cachedHash <- readCacheFile "plexus_hash"
    let currentHash = plexusHash resp

    when (cachedHash /= currentHash) $ do
        -- Schema changed - refresh cache
        refreshSchemaCache currentHash

    processData (responseData resp)
```

## Dedicated Endpoint

For explicit queries, use `plexus_hash`:

```json
{"jsonrpc":"2.0","id":1,"method":"plexus_hash","params":[]}
```

Response:
```json
{
  "plexus_hash": "7a1a43920ee194e1",
  "type": "data",
  "content_type": "plexus.hash",
  "data": { "hash": "7a1a43920ee194e1" }
}
```

## Implementation Details

### PlexusContext (src/plexus/context.rs)

Global singleton storing the hash:

```rust
static PLEXUS_CONTEXT: OnceLock<PlexusContext> = OnceLock::new();

impl PlexusContext {
    pub fn init(hash: String);  // Called once at plexus build
    pub fn hash() -> String;    // Get hash from anywhere
}
```

### PlexusStreamItem (src/plexus/types.rs)

Restructured to include hash:

```rust
// Inner event enum
pub enum PlexusStreamEvent {
    Progress { ... },
    Data { provenance, content_type, data },
    Error { ... },
    Done { provenance },
}

// Wrapper with hash
pub struct PlexusStreamItem {
    pub plexus_hash: String,
    #[serde(flatten)]
    pub event: PlexusStreamEvent,
}
```

### Hash Computation (src/plexus/plexus.rs)

```rust
pub fn compute_hash(&self) -> String {
    // Build: "namespace:version:method1,method2;namespace2:version2:..."
    // Sort alphabetically for determinism
    // Return 16-char hex of hash
}
```

## Testing

```bash
# Any endpoint returns the hash
echo '{"jsonrpc":"2.0","id":1,"method":"health_check","params":[]}' | \
  websocat ws://127.0.0.1:4444 | jq '.plexus_hash'

# Dedicated endpoint
echo '{"jsonrpc":"2.0","id":1,"method":"plexus_hash","params":[]}' | \
  websocat ws://127.0.0.1:4444 | jq '.data.hash'
```

## Related

- Schema enrichment: `16680896323511347711_schema-enrichment-and-cache-invalidation.md`
- Self-documenting RPC: `16680998353176467711_self-documenting-rpc.md`
