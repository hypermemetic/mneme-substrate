# Schema-Intrinsic Hashing

## Overview

The schema coalgebra now carries its own cache invalidation hashes. Every method and plugin has a `hash` field computed from its definition, enabling precise invalidation at any granularity.

## Before

```
┌─────────────────────────────────────────┐
│ plexus_schema() → PluginSchema          │ ← No hashes
├─────────────────────────────────────────┤
│ plexus_hash() → compute_hash()          │ ← Separate computation
├─────────────────────────────────────────┤
│ plexus_list_activations() → flat list   │ ← Redundant
└─────────────────────────────────────────┘
```

## After

```
┌─────────────────────────────────────────┐
│ plexus_schema() → PluginSchema          │
│   ├── hash: "53ff863d..."               │ ← Recursive hash
│   ├── methods[].hash                    │ ← Per-method hash
│   └── children[].hash                   │ ← Per-child hash
├─────────────────────────────────────────┤
│ plexus_hash() → schema.hash             │ ← Same source
└─────────────────────────────────────────┘
```

`list_activations` removed - use `schema()` for all introspection.

## Hash Computation

### Method Hashes (Compile-Time)

Generated in `hub-macro` at compile time from the method definition:

```rust
// hub-macro/src/codegen/method_enum.rs
fn compute_method_hash(method: &MethodInfo) -> String {
    let mut hasher = DefaultHasher::new();

    method.method_name.hash(&mut hasher);
    method.description.hash(&mut hasher);

    for param in &method.params {
        param.name.to_string().hash(&mut hasher);
        quote!(#ty).to_string().hash(&mut hasher);  // Type as string
        param.description.hash(&mut hasher);
    }

    if let Some(item_ty) = &method.stream_item_type {
        quote!(#item_ty).to_string().hash(&mut hasher);
    }

    format!("{:016x}", hasher.finish())
}
```

**Inputs hashed**:
- Method name
- Description
- Parameter names, types (stringified), descriptions
- Return type (stringified)

**Change any of these** → hash changes → recompilation required anyway.

### Plugin Hashes (Runtime)

Computed when `PluginSchema::leaf()` or `PluginSchema::hub()` is called:

```rust
// src/plexus/schema.rs
impl PluginSchema {
    fn compute_hash(methods: &[MethodSchema], children: Option<&[PluginSchema]>) -> String {
        let mut hasher = DefaultHasher::new();

        for m in methods {
            m.hash.hash(&mut hasher);  // Roll up method hashes
        }

        if let Some(kids) = children {
            for c in kids {
                c.hash.hash(&mut hasher);  // Roll up child hashes
            }
        }

        format!("{:016x}", hasher.finish())
    }
}
```

**Inputs hashed**:
- All method hashes
- All child plugin hashes (recursive)

## Hash Propagation

```
echo.echo (method)     hash: 4e46b3d080d82a46
echo.once (method)     hash: f096f200c5c87712
    ↓ combine
echo (plugin)          hash: eb051e8e68a4094c

health.check (method)  hash: f8c33abd8ecc3405
    ↓ combine
health (plugin)        hash: c5bb0bb85c4f2907

plexus.call            hash: b0b0c4e48964c798
plexus.hash            hash: 3ec771e3435b1492
plexus.schema          hash: f7b808e8f07be2ca
    ↓ combine with children
plexus (root)          hash: 53ff863dfdec6392
```

## Client Usage

### Full Cache Invalidation

```haskell
-- Check if anything changed
oldHash <- getCachedHash
newHash <- callPlexusHash
when (oldHash /= newHash) $
    refetchEverything
```

### Granular Invalidation

```haskell
-- Only refetch changed plugins
oldSchema <- getCachedSchema
newSchema <- callPlexusSchema

for_ (zip (children oldSchema) (children newSchema)) $ \(old, new) ->
    when (hash old /= hash new) $
        invalidatePlugin (namespace new)
```

### Method-Level Invalidation

```haskell
-- Detect changed methods
for_ (methods newPlugin) $ \method ->
    case lookup (name method) oldMethodHashes of
        Just oldHash | oldHash /= hash method ->
            invalidateMethodCache (name method)
        Nothing ->
            -- New method
            pure ()
```

## Wire Format

```json
{
  "namespace": "echo",
  "version": "1.0.0",
  "description": "Echo messages back",
  "hash": "eb051e8e68a4094c",
  "methods": [
    {
      "name": "echo",
      "description": "Echo a message back",
      "hash": "4e46b3d080d82a46",
      "params": { ... },
      "returns": { ... }
    }
  ],
  "children": null
}
```

## Properties

1. **Deterministic**: Same definition → same hash (across builds)
2. **Hierarchical**: Change propagates up the tree
3. **Granular**: Invalidate at method, plugin, or root level
4. **Intrinsic**: Hash is part of the schema, not a separate endpoint
5. **Compile-time**: Method hashes computed during macro expansion

## Files Changed

| File | Change |
|------|--------|
| `src/plexus/schema.rs` | Added `hash` to `MethodSchema` and `PluginSchema` |
| `src/plexus/plexus.rs` | Removed `list_activations`, simplified `compute_hash()` |
| `hub-macro/src/codegen/method_enum.rs` | Added `compute_method_hash()` |

## Migration

**Before**:
```rust
MethodSchema::new(name, description)
```

**After**:
```rust
MethodSchema::new(name, description, hash)
```

The hub-macro handles this automatically. Manual implementations need to compute a hash.
