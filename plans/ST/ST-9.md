---
id: ST-9
title: "Registry + Mustache local newtypes: BackendUrl, TemplateId, PluginId wrappers"
status: Pending
type: implementation
blocked_by: [ST-2]
unlocks: [ST-10]
severity: Medium
target_repo: plexus-substrate
---

## Problem

Registry today stores `host`, `port`, `protocol` as separate loose fields on `BackendInfo`: `host: String`, `port: u16`, `protocol: String`. The audit flags this: invalid `protocol` values (e.g., `"http"` when only `"ws"` / `"wss"` are supported) silently deserialize. Mustache uses `TemplateId: String`, `method: String`, `plugin_id: Uuid` — no newtypes distinguish plugin UUIDs from other UUIDs, and `TemplateId` is swap-compatible with any other string.

ST-2 defines the canonical types (`BackendUrl` struct, `BackendProtocol` enum, `TemplateId` newtype). This ticket is the per-activation migration that consumes them.

## Context

Registry lives under `src/activations/registry/`. Mustache lives under `src/activations/mustache/`.

Registry's current `BackendInfo` shape (from `registry/types.rs`):

```rust
pub struct BackendInfo {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub protocol: String,  // "ws" or "wss" — should be BackendProtocol
    ...
    pub namespace: Option<String>,
    ...
}
```

Mustache's `TemplateInfo` (from `mustache/types.rs:9`):

```rust
pub struct TemplateInfo {
    pub id: String,              // → TemplateId
    pub plugin_id: Uuid,         // → PluginId newtype (local to Mustache)
    pub method: String,          // free-form method name, leave as String
    pub name: String,            // name, leave as String
    pub created_at: i64,
    pub updated_at: i64,
}
```

`PluginId` is not in the ST-2 pinned list — it's a Mustache-local concept. This ticket defines `PluginId(Uuid)` inside `mustache/types.rs` as a local newtype. It does NOT get promoted to `crate::types` because no other activation consumes it. If ST-2 later promotes it, that's a separate ticket.

Files owned by this ticket (exclusive write):

- `src/activations/registry/*`
- `src/activations/mustache/*`

## Required behavior

Registry input/output table:

| Current shape | New shape |
|---|---|
| `pub struct BackendInfo { pub host: String, pub port: u16, pub protocol: String, ... }` | Two options: (A) add a computed method `BackendInfo::backend_url(&self) -> Result<BackendUrl, BackendUrlParseError>` and KEEP the three flat fields for wire-format continuity; (B) change the struct to carry `url: BackendUrl` as a structured field, breaking wire format. Default: (A). Wire format stays byte-identical. Internal call sites that need the parsed form call `backend_url()`. |
| Registry's `register_backend(..., host: String, port: u16, protocol: String, ...)` RPC method | Accepts the same three parameters; internally constructs `BackendUrl::parse_fields(protocol, host, port)` (a static constructor added to `BackendUrl` in this ticket) that validates the protocol enum. Invalid protocol returns a structured error instead of being silently stored. |
| `pub struct BackendInfo { pub namespace: Option<String>, ... }` | Unchanged — namespace is a free-form routing label |
| Storage `bind(&protocol)` etc. | Unchanged (still writing flat columns) |

Mustache input/output table:

| Current shape | New shape |
|---|---|
| `pub struct TemplateInfo { pub id: String, pub plugin_id: Uuid, pub method: String, ... }` | `id: TemplateId` (imported from `crate::types`); `plugin_id: PluginId` (local newtype defined in `mustache/types.rs`); `method: String` unchanged |
| `MustacheError::TemplateNotFound(String)` | `MustacheError::TemplateNotFound(TemplateId)` |
| Any public function with `template_id: String` or `id: String` representing a template ID | `TemplateId` |
| Any public function with `plugin_id: Uuid` | `PluginId` |
| Storage `bind(&id)` for template IDs | `bind(id.as_str())` |

`PluginId` definition (local to Mustache):

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct PluginId(pub Uuid);
```

Plus `PluginId::new(Uuid) -> Self`, `PluginId::inner() -> &Uuid`, `Display`.

## Risks

- **`protocol: String` validation on write.** Today any string deserializes. After this ticket, `register_backend` rejects non-`ws` / non-`wss` protocols at the validation boundary. This is intentional — the audit flags the current behavior as incorrect — but it IS a behavior change observable to clients that were sending invalid protocols. Default: accept this change; it's strictly more correct. Alternative: accept-and-warn for one version before rejecting (behavioral lenience). Author chooses; document in commit.
- **`BackendInfo` wire format.** The struct serializes to JSON with the three flat fields. Clients parsing that JSON are unaffected by the internal `BackendUrl` computation. If option (B) were chosen (struct change), wire format breaks — hence default (A).
- **`PluginId` promotion.** If later work needs `PluginId` cross-activation, it graduates to `crate::types`. Not in this ticket.

## What must NOT change

- Wire format for every Registry and Mustache RPC method — byte-identity. `BackendInfo` JSON shape unchanged.
- SQLite schemas.
- `BackendSource` enum shape.
- Registry's `url()` method (the flat `protocol://host:port` form used in logs and client construction).
- Mustache's template rendering behavior.

## Acceptance criteria

1. `cargo build -p plexus-substrate` succeeds.
2. `cargo test -p plexus-substrate` succeeds.
3. A new method `BackendInfo::backend_url(&self) -> Result<BackendUrl, BackendUrlParseError>` exists and returns `Ok` for valid `ws`/`wss` protocols.
4. `register_backend` (or equivalent) with `protocol: "http"` returns a structured error (not silently stored).
5. Grep audit in `src/activations/registry/`: no bare `String` represents a protocol that should be `BackendProtocol`; the flat field remains but internal code paths that previously parsed it ad-hoc now go through `BackendUrl`.
6. Grep audit in `src/activations/mustache/`: `TemplateInfo.id` is `TemplateId`; `TemplateInfo.plugin_id` is `PluginId`; no bare `Uuid` represents a plugin identity in any public function or struct in Mustache.
7. Unit tests round-trip `BackendInfo` and `TemplateInfo` through serde with byte-identity compared against committed pre-migration fixtures.
8. Unit test for `BackendInfo::backend_url()` covers: valid `ws`, valid `wss`, invalid protocol (returns structured error).

## Completion

Implementor delivers:

- A commit modifying only files under `src/activations/registry/` and `src/activations/mustache/`.
- Committed JSON fixtures `tests/fixtures/registry_backend_info_wire.json`, `tests/fixtures/mustache_template_info_wire.json`.
- `cargo build -p plexus-substrate` and `cargo test -p plexus-substrate` green.
- Commit message documents the `protocol` validation-behavior choice (strict reject vs. accept-and-warn).
- Ticket status flipped from `Ready` → `Complete`.
- ST-10 notified.
