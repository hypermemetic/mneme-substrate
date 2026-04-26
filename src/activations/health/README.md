# health

Check hub health and uptime — manual-impl reference activation.

## Overview

Health is the reference implementation for an activation written **without**
the `#[plexus_macros::activation]` macro. It hand-implements `Activation`,
`ChildRouter`, and a jsonrpsee RPC trait directly. Use it as the diff target
when comparing macro-generated output to the equivalent hand-written form,
and when investigating what the macro is actually producing.

Health tracks a single piece of runtime state — the hub's start time — and
exposes it through a `check` subscription that yields a `HealthEvent::Status`
with uptime and a wall-clock timestamp. The `schema` method is auto-surfaced
by the substrate's standard schema routing and lets callers retrieve the
full plugin schema or a single-method schema via `{"method": "name"}`.

## Namespace

`health` — invoked via `synapse <backend> health.<method>`.

## Methods

| Method | Params | Returns | Description |
|---|---|---|---|
| `check` | — | `Stream<Item=HealthEvent>` | Check the health status of the hub and return uptime. |
| `schema` | `method?: String` | `Stream<Item=SchemaResult>` | Get the plugin schema, or a single method schema when `method` is provided. |

## Composition

Health has no dependencies on other activations and is constructed as
`Health::new()`.

## Example

```bash
synapse --port 44104 lforge substrate health.check
synapse --port 44104 lforge substrate health.schema '{"method":"check"}'
```

## Source

- `activation.rs` — hand-written `Activation` / `HealthRpcServer` / `ChildRouter`
- `methods.rs` — `HealthMethod` enum (the `Activation::Methods` assoc type)
- `types.rs` — `HealthEvent` / `HealthStatus` wire types
- `mod.rs` — module exports
