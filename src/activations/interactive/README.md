# interactive

Interactive methods demonstrating bidirectional communication.

## Overview

Interactive is the reference activation for bidirectional transports —
methods that issue server → client requests (prompts, selections,
confirmations) during streaming execution. It uses the
`plexus_core::plexus::bidirectional::StandardBidirChannel` API (`ctx.prompt`,
`ctx.select`, `ctx.confirm`) so clients on MCP, WebSocket, or any transport
with bidirectional support can satisfy requests mid-stream.

All methods use the `bidirectional` attribute on `#[plexus_macros::method]`.
When the transport does not support bidirectional requests
(`BidirError::NotSupported`), methods emit a descriptive error event and
exit cleanly — callers can detect the degraded mode rather than hanging.

## Namespace

`interactive` — invoked via `synapse <backend> interactive.<method>`.

## Methods

| Method | Params | Returns | Description |
|---|---|---|---|
| `wizard` | — | `Stream<Item=WizardEvent>` (bidir) | Multi-step setup wizard: prompts for a project name, offers a select of templates, then asks for confirmation. Demonstrates `prompt`/`select`/`confirm`. |
| `delete` | `paths: Vec<String>` | `Stream<Item=DeleteEvent>` (bidir) | Delete files with a single confirmation gate. Non-interactive transports decline for safety. |
| `confirm` | `message: String` | `Stream<Item=ConfirmEvent>` (bidir) | Ask a single yes/no question and emit `Confirmed` / `Declined` / `Error`. |

## Composition

Interactive has no storage and no dependencies on other activations — it is
entirely a demonstration of the bidirectional channel API.

## Example

```bash
# Requires a bidirectional transport (e.g. MCP)
synapse --port 44104 lforge substrate interactive.confirm '{"message":"Proceed?"}'
```

The wizard flow is documented with an ASCII sequence diagram in
`mod.rs` — see the module-level doc comment for the exact request/response
ordering over MCP.

## Source

- `activation.rs` — RPC method surface
- `types.rs` — `WizardEvent` / `DeleteEvent` / `ConfirmEvent` wire types
- `mod.rs` — module exports + bidirectional flow documentation
