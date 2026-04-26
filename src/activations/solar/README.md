# solar

Solar system — demonstrates nested plugin hierarchy (hub activation).

## Overview

Solar is the reference **hub activation** — an activation that has children
of its own. It models a toy solar system: Sol (the star) has 8 planets as
configured children, and several planets carry moons as nested children
(e.g. `solar.earth.luna.info`, `solar.jupiter.io.info`). The hierarchy is
built once at `Solar::new()` from an in-memory `CelestialBody` tree (see
`celestial.rs`) and exposes it to Plexus via a dynamic `#[child]` gate.

Solar is used by substrate tests and docs to validate nested-routing
behavior: `DynamicHub` routing resolves the `body` child gate to a
`CelestialBodyActivation`, and that activation's own schema surfaces its
moons as further children.

### Notable quirks

- Solar keeps a hand-written `plugin_children()` override so each planet's
  `ChildSummary.hash` is a deterministic digest of the planet's own
  sub-schema (the macro synthesizes empty-hash summaries for dynamic
  children). The override is annotated `#[deprecated(since="0.5.0")]`
  and `#[plexus_macros::removed_in("0.6")]` — tracked for removal once
  hash computation moves to the runtime (HASH-1).

## Namespace

`solar` — invoked via `synapse <backend> solar.<method>`, or drill down via
`synapse <backend> solar.<body>[.<moon>].<method>`.

## Methods

| Method | Params | Returns | Description |
|---|---|---|---|
| `observe` | — | `Stream<Item=SolarEvent>` | Get an overview of the solar system (star, planet count, moon count, total bodies). |
| `info` | `path: String` | `Stream<Item=SolarEvent>` | Get detailed info about a body at `path` (e.g. `"earth"`, `"jupiter.io"`, `"saturn.titan"`). |

## Children

| Child | Kind | list method | search method | Description |
|---|---|---|---|---|
| `body` | dynamic | `body_names` | `body` | Look up a celestial body by name (case-insensitive). Returns a `CelestialBodyActivation` that carries the body's own schema + methods + nested children. |

The 8 configured top-level bodies are: `Mercury`, `Venus`, `Earth`, `Mars`,
`Jupiter`, `Saturn`, `Uranus`, `Neptune`. `Earth` has 1 moon (Luna),
`Mars` has 2, `Jupiter` has 4 (Galilean), `Saturn` has 3, `Uranus` has 1,
`Neptune` has 1.

## Composition

Solar has no storage and no external dependencies. Each `CelestialBody`
hand-implements `Activation` + `ChildRouter` in `celestial.rs`, giving the
hierarchy its recursive schema surface.

## Example

```bash
synapse --port 44104 lforge substrate solar.observe
synapse --port 44104 lforge substrate solar.info '{"path":"jupiter.io"}'

# Nested routing through the dynamic child gate
synapse --port 44104 lforge substrate solar.jupiter.info
synapse --port 44104 lforge substrate solar.earth.luna.info
```

## Source

- `activation.rs` — RPC method surface, dynamic `body` child gate, and
  deprecated `plugin_children()` override
- `celestial.rs` — `CelestialBody`, `CelestialBodyActivation`,
  `build_solar_system`, recursive `ChildRouter` impl for moons
- `types.rs` — `BodyType`, `SolarEvent`
- `mod.rs` — module exports
