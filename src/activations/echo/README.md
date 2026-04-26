# echo

Echo messages back — demonstrates plexus-macros usage.

## Overview

Echo is the minimal reference activation in substrate. It demonstrates the
`#[plexus_macros::activation]` / `#[plexus_macros::method]` pattern in the
smallest possible surface area: three methods, no storage, no parent context,
no handle system. Use it as a template when scaffolding a new leaf activation,
and as a smoke-test target when validating a new backend wiring.

Event types (`EchoEvent`) are plain domain types — no special traits required
beyond the standard `Serialize`/`Deserialize`/`JsonSchema` derives. The macro
handles `wrap_stream(…)` at the call site.

## Namespace

`echo` — invoked via `synapse <backend> echo.<method>`.

## Methods

| Method | Params | Returns | Description |
|---|---|---|---|
| `echo` | `message: String, count: u32` | `Stream<Item=EchoEvent>` | Echo a message back the specified number of times (0 treated as 1). |
| `once` | `message: String` | `Stream<Item=EchoEvent>` | Echo a message once. |
| `ping` | — | `Stream<Item=EchoEvent>` | Ping — returns a `Pong` response. |

## Composition

Echo has no dependencies on other activations and no parent context.

## Example

```bash
synapse --port 44104 lforge substrate echo.echo '{"message":"hi","count":3}'
synapse --port 44104 lforge substrate echo.ping
```

## Source

- `activation.rs` — RPC method surface
- `types.rs` — `EchoEvent` wire type
- `mod.rs` — module exports
