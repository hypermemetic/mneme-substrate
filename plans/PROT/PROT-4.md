---
id: PROT-4
title: "plexus-transport 0.3.0: bump plexus-core dep to 0.6"
status: Pending
type: implementation
blocked_by: [PROT-2]
unlocks: [PROT-7, PROT-8]
severity: High
target_repo: plexus-transport
---

## Problem

plexus-transport pins `plexus-core = "0.5"`. PROT-2 bumps plexus-core to 0.6.0 with a breaking removal of `SchemaResult::Method`. Transport code that references `SchemaResult` directly (if any) must adapt. Consumers of plexus-transport (substrate, hyperforge) expect it to depend on plexus-core 0.6.x.

## Context

- Repo: `/Users/shmendez/dev/controlflow/hypermemetic/plexus-transport/`
- Version: 0.2.1 → 0.3.0 (breaking via transitive dep bump).
- Files to edit:
  - `Cargo.toml` — `[dependencies]` and `[dev-dependencies]` bump.
  - `src/` — audit for any `SchemaResult::Method` reference. Likely none, but verify.

## Required behavior

1. **Bump** in `Cargo.toml`:
   ```
   plexus-core = { path = "../plexus-core", version = "0.6" }
   ```
   (both `[dependencies]` and `[dev-dependencies]` sections).

2. **Audit** source for any `SchemaResult` pattern match or variant reference. Most transport code is pass-through (wire-level JSON), so unlikely to have direct references. If found, migrate per PROT-2's resolution (either `SchemaResult` is flattened to `PluginSchema`, or becomes a type alias — read PROT-2's landed commit to see which).

3. **Build + test** green on plexus-core 0.6.

4. **Version bump** plexus-transport: 0.2.1 → 0.3.0.

5. **Tag** `plexus-transport-v0.3.0` locally.

## Risks

| Risk | Mitigation |
|---|---|
| plexus-transport's test fixtures hard-code `SchemaResult::Method`. | Grep first. Migrate if found. |
| Transport-internal types that wrap `SchemaResult` exist. | Same audit. The `fetchSchemaAt`-equivalent on the Rust side (if any) may need to adapt. |
| Bumping plexus-core triggers cascading compile errors in transport's macro usage (plexus-macros references). | plexus-macros 0.6 lands in PROT-3 (parallel, but downstream of PROT-2 like this ticket). Confirm transport builds against 0.6.x of BOTH before landing. |

## What must NOT change

- plexus-transport's public API surface (WebSocket, HTTP/SSE transports).
- Wire format beyond the PROT-1-pinned unified schema response.
- Backend discovery behavior.
- The transport server builder signature that substrate/hyperforge depend on.

## Acceptance criteria

1. `cargo build -p plexus-transport` green.
2. `cargo test -p plexus-transport` green.
3. plexus-transport `Cargo.toml` pins `plexus-core = "0.6"`. Version is `0.3.0`. Tag `plexus-transport-v0.3.0` exists locally.
4. `grep -rn 'SchemaResult::Method' plexus-transport/src/` returns zero results.

## Completion

PR against plexus-transport. Status flipped to Complete when PROT-7 and PROT-8 (substrate and hyperforge) both build against it.
