# registry

Backend discovery and registration service for Plexus hubs.

## Overview

Registry is the backend-discovery activation. Substrate embeds it by
re-exporting `Registry`/`RegistryStorageConfig`/`BackendInfo`/
`BackendSource`/`RegistryEvent` from the external `plexus-registry` crate
— the source lives there, not in this substrate module. This directory
is a thin glue layer: `mod.rs` imports `activation`/`storage`/`types` from
the upstream crate via Cargo's module system, and the substrate builder
registers `Registry::with_defaults()` alongside the native activations.

Registered backends carry a name, host, port, protocol (`ws`/`wss`), an
optional namespace for routing, a `BackendSource` tag (`Auto`, `File`,
`Manual`, `Env`), and health timestamps. The config file (loaded by
`RegistryStorage::load_config` during `Registry::new`) seeds the initial
set; `ping` updates the `last_seen` timestamp for liveness checks.

## Namespace

`registry` — invoked via `synapse <backend> registry.<method>`.

## Methods

| Method | Params | Returns | Description |
|---|---|---|---|
| `register` | `name: String, host: String, port: u16, protocol: Option<String>, description: Option<String>, namespace: Option<String>` | `Stream<Item=RegistryEvent>` | Register a new Plexus backend for discovery. Protocol defaults to `"ws"`. |
| `list` | `active_only: Option<bool>` | `Stream<Item=RegistryEvent>` | List registered backends (active-only by default). |
| `get` | `name: String` | `Stream<Item=RegistryEvent>` | Get information about a specific backend by name. |
| `update` | `name: String, host: Option<String>, port: Option<u16>, protocol: Option<String>, description: Option<String>, namespace: Option<String>` | `Stream<Item=RegistryEvent>` | Update an existing backend's connection info. |
| `delete` | `name: String` | `Stream<Item=RegistryEvent>` | Remove a backend from the registry. |
| `ping` | `name: String` | `Stream<Item=RegistryEvent>` | Update the `last_seen` timestamp for a backend. |
| `reload` | — | `Stream<Item=RegistryEvent>` | Reload backends from the config file. |

## Storage

- Backend: SQLite (owned by the upstream `plexus-registry` crate)
- Config: `RegistryStorageConfig` (upstream) — seeds from a config file on
  startup via `load_config`.

## Composition

- Provided by the upstream `plexus-registry` crate — this directory only
  re-exports the public symbols so substrate-internal code can reach them
  as `crate::activations::registry::…`. The substrate builder constructs
  it with `Registry::with_defaults()` and registers it with the hub.

## Example

```bash
synapse --port 44104 lforge substrate registry.list
synapse --port 44104 lforge substrate registry.register \
  '{"name":"my-backend","host":"127.0.0.1","port":44105}'
```

## Source

- `mod.rs` — re-exports `Registry`, `RegistryStorageConfig`,
  `BackendInfo`, `BackendSource`, `RegistryEvent`
- `types.rs` — substrate-local mirror of the wire types (the authoritative
  definitions live in the `plexus-registry` crate)
- Upstream: `plexus-registry` crate (see `Cargo.toml` dependency
  `registry = { package = "plexus-registry", version = "0.1.0" }`)
