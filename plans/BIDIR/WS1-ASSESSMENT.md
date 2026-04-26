# WS1: Assessment of Current Bidirectional Implementation

**Date**: 2026-02-12
**Status**: Complete ✅

## Summary

**No bidirectional implementation exists.** The work is entirely greenfield.

## Findings

### Documentation Only (Commit 7c6105c)
- Added architecture doc: `16679659514785209599_bidirectional-sse-transport.md`
- Describes SSE transport architecture with "bidirectional via POST + SSE"
- **This is NOT the same as our generic BidirChannel plan**
- No implementation code was included

### Existing Infrastructure

**✅ plexus-core/src/plexus/types.rs**:
- `PlexusStreamItem` enum exists with 4 variants:
  - `Data` - data payload
  - `Progress` - progress updates
  - `Error` - error occurred
  - `Done` - stream completed
- **Missing**: `Request` variant for bidirectional requests
- `StreamMetadata` structure in place (provenance, plexus_hash, timestamp)

**✅ plexus-core module structure**:
```
src/plexus/
├── context.rs
├── dispatch.rs
├── errors.rs
├── guidance.rs
├── hub_context.rs
├── method_enum.rs
├── middleware.rs
├── mod.rs
├── path.rs
├── plexus.rs
├── schema.rs
├── streaming.rs
└── types.rs
```
- **Missing**: `bidirectional/` directory

**✅ plexus-macros**:
- `#[hub_method]` macro infrastructure exists
- Supports attributes like `description`, `streaming`
- **Missing**: `bidirectional` attribute support

### What Does NOT Exist

**❌ No bidirectional types**:
- No `BidirChannel<Req, Resp>`
- No `BidirError` enum
- No `StandardRequest/StandardResponse` types
- No `SelectOption` struct

**❌ No bidirectional code in any repo**:
```bash
# Searched all repos for: BidirChannel, bidirectional, RequestType, ResponsePayload
# Result: No matches in src/ directories
```

**❌ No transport integration**:
- No MCP `_plexus_respond` tool
- No WebSocket `plexus_respond` RPC method
- No Request stream item handling

**❌ No client support**:
- plexus-protocol: No bidirectional handler types
- synapse: No interactive handler
- hub-codegen: No bidirectional client generation

## Decision

**Start fresh with generic-first implementation** as planned in BIDIR-GENERIC.md.

The existing SSE transport doc is orthogonal - it describes a different architecture (HTTP POST for client→server, SSE for server→client). Our generic BidirChannel works within the existing streaming framework.

## What We Can Leverage

1. **PlexusStreamItem enum**: We'll add `Request` variant
2. **StreamMetadata**: Already has provenance and plexus_hash we need
3. **#[hub_method] macro**: Infrastructure exists, we'll add bidirectional attribute
4. **Transport layers**: WebSocket (jsonrpsee) and MCP bridge exist, we'll extend them

## Next Steps

Proceed to **WS2: Generic Core Types** - implement:
1. `BidirError` enum
2. `StandardRequest/StandardResponse` enums
3. `SelectOption` struct
4. Add `Request` variant to `PlexusStreamItem`

No blockers identified. Ready to implement!
