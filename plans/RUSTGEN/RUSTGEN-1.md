---
id: RUSTGEN-1
title: "Epic: hub-codegen Rust backend parity with TypeScript"
status: Epic
type: epic
blocked_by: []
unlocks: []
target_repo: hub-codegen
---

## Goal

Bring hub-codegen's Rust backend to feature parity with the TypeScript backend. When this epic lands, a Rust consumer can feed the same IR through hub-codegen and get a generated client crate whose capabilities are indistinguishable in shape from the TypeScript package the TS backend already ships: per-namespace client modules, typed tagged-union types, a JSON-RPC transport layer, streaming helpers, real typed `Child` associations on every `DynamicChild` gate (no more `serde_json::Value` stub), a generated `Cargo.toml`, smoke tests covering the same scenarios as the TS tests, and a runnable consumer-facing example crate that proves the output compiles and calls a live substrate end-to-end.

At epic Complete:

- `hub-codegen/src/generator/rust/` has siblings to every file in `hub-codegen/src/generator/typescript/` (types.rs → types.rs, rpc.rs → rpc.rs, transport.rs → transport.rs, namespaces.rs → namespaces.rs, package.rs → package.rs, tests.rs → tests.rs).
- Every `MethodRole::DynamicChild { .. }` gate in generated Rust output has `type Child = <real client struct>`. `serde_json::Value` as `Child` appears nowhere in generated output.
- A consumer example crate (in `hub-codegen/examples/rust_consumer/` or equivalent) compiles against generated output and calls a live substrate activation using typed handles. `cargo run` succeeds.
- `cargo test` in hub-codegen with the Rust backend enabled passes, with coverage that includes golden fixtures matching the TS backend's fixture scenarios (IR-7 deprecation rendering, IR-9 dynamic-child typed handles, static-child generation, per-namespace partitioning).

## Context

**Upstream landed:**

| Commit | Scope |
|---|---|
| IR-9 (`b4fea08` in hub-codegen) | TS backend: full `DynamicChild<T>` typed-handle generation. Rust backend: skeleton only — gate structs exist, `Child` type is `serde_json::Value`. |
| IR-13 | Superseded by this epic. IR-13 tried to complete the Rust wiring as a single ticket but got stopped mid-edit at ~400 lines. Root cause: the wiring depended on infrastructure (per-namespace Rust clients, transport, rpc helpers) that didn't exist. RUSTGEN builds that infrastructure first, then does the wiring. |

**TypeScript backend layout (parity target):**

```
hub-codegen/src/generator/typescript/
  mod.rs          # orchestration, plugin partitioning, filter logic
  types.rs        # per-namespace tagged unions, enums, core transport types
  rpc.rs          # JSON-RPC framing + stream helpers (generated into rpc.ts)
  transport.rs    # WebSocket transport (generated into transport.ts)
  namespaces.rs   # per-namespace client modules (the "big one")
  package.rs      # package.json generation
  tests.rs        # smoke tests + golden fixtures
```

**Current Rust backend layout:**

```
hub-codegen/src/generator/rust/
  mod.rs          # orchestration (minimal)
  types.rs        # core types only
  client.rs       # monolithic — base PlexusClient + per-namespace modules glued together
  tests.rs        # minimal
```

`rust/client.rs` must be split: the base transport layer moves to `rust/transport.rs`, the JSON-RPC / stream helper logic moves to `rust/rpc.rs`, and the per-namespace generation moves to `rust/namespaces.rs`, leaving `client.rs` thin (or deleted).

**Pinned from README:**

- Plexus RPC terminology (activation, hub activation, DynamicChild, etc.) is canonical.
- ST's domain newtypes (`SessionId`, `GraphId`, `NodeId`, `StreamId`, `ApprovalId`, `ToolUseId`, `TicketId`, `WorkingDir`, `ModelId`, `BackendUrl`, `TemplateId`) — when ST ships, any schema field carrying one of these types must flow through type generation and end up as the newtype, not a bare `String` / `Uuid` / etc. RUSTGEN does not introduce these newtypes (that's ST's ticket); RUSTGEN must not strip them.

## Dependency DAG

```
          RUSTGEN-S01           RUSTGEN-S02
        (runtime library     (WebSocket transport
         shape spike)         dependency spike)
              │                       │
              └────────────┬──────────┘
                           ▼
                       RUSTGEN-2
                     (types.rs gen)
                           │
                           ▼
                       RUSTGEN-3
                      (rpc.rs gen)
                           │
                           ▼
                       RUSTGEN-4
                   (transport.rs gen)
                           │
                           ▼
                       RUSTGEN-5
                 (namespaces.rs gen — the big one)
                           │
                           ▼
                       RUSTGEN-6
           (DynamicChild<T> real-Child wiring,
            absorbs IR-13's scope)
                           │
             ┌─────────────┼─────────────┐
             ▼             ▼             ▼
         RUSTGEN-7     RUSTGEN-8     RUSTGEN-9
         (Cargo.toml   (smoke tests  (consumer
          gen)          + golden       example
                        fixtures)      crate)
```

Phase gating:

- **Phase 0 — Spikes (parallel):** RUSTGEN-S01, RUSTGEN-S02. Independent investigations, different concerns.
- **Phase 1 — Infrastructure (serial):** RUSTGEN-2 → RUSTGEN-3 → RUSTGEN-4 → RUSTGEN-5. Each builds on imports from the previous file. Serial because each downstream file needs to import from the upstream file; parallelism is bounded by the import graph.
- **Phase 2 — Wiring:** RUSTGEN-6. Requires namespaces.rs generation from phase 1.
- **Phase 3 — Completion (parallel):** RUSTGEN-7, RUSTGEN-8, RUSTGEN-9. All independently testable once phase 2 lands.

File-boundary check: each RUSTGEN-2..7 writes to a distinct file in `hub-codegen/src/generator/rust/`. RUSTGEN-8 writes to `hub-codegen/tests/` plus golden fixtures. RUSTGEN-9 writes to `hub-codegen/examples/`. These are disjoint file sets; phase-3 tickets parallelize cleanly.

## Phase Breakdown

| Phase | Tickets | Concern |
|---|---|---|
| 0. Spikes | S01, S02 | Library shape + transport dep decisions |
| 1. Infrastructure | 2, 3, 4, 5 | Type / rpc / transport / namespace codegen |
| 2. Wiring | 6 | Replace `serde_json::Value` stub with real Child types |
| 3. Completion | 7, 8, 9 | Cargo.toml, tests, consumer example |

## Tickets

| ID | Summary | Target file(s) | Status |
|---|---|---|---|
| RUSTGEN-1 | This epic overview | — | Epic |
| RUSTGEN-S01 | Spike: Rust runtime library shape (inline / sibling crate / vendored) | spike/ dir + decision doc | Pending |
| RUSTGEN-S02 | Spike: WebSocket transport dependency choice + live roundtrip | spike/ dir + decision doc | Pending |
| RUSTGEN-2 | Rust `types.rs` generation parity | `generator/rust/types.rs` | Pending |
| RUSTGEN-3 | Rust `rpc.rs` generation parity | `generator/rust/rpc.rs` | Pending |
| RUSTGEN-4 | Rust `transport.rs` generation parity | `generator/rust/transport.rs` | Pending |
| RUSTGEN-5 | Rust per-namespace client module generation | `generator/rust/namespaces.rs` | Pending |
| RUSTGEN-6 | `DynamicChild<T>` real-Child wiring (absorbs IR-13) | `generator/rust/namespaces.rs` (update) | Pending |
| RUSTGEN-7 | Rust `Cargo.toml` + manifest generation | `generator/rust/package.rs` | Pending |
| RUSTGEN-8 | Rust smoke tests + golden fixtures parity with TS | `tests/rust_*` + golden dir | Pending |
| RUSTGEN-9 | Rust consumer-facing example crate | `examples/rust_consumer/` | Pending |

## Out of scope

- New IR features. This epic consumes IR v2.0 as-is; any IR change is another epic.
- TypeScript backend changes. The TS backend is the parity reference, not a modification target. If a parity delta reveals a TS bug, that's a separate TS-backend ticket.
- Non-Rust / non-TS target languages (Python, Go, etc.). Future per-language backends follow the pattern this epic establishes; they are not in scope here.
- Runtime library publication to crates.io. The consumer example may reference a local path or vendored copy; publishing a `plexus-client-runtime` crate (if that's the shape RUSTGEN-S01 picks) is a follow-up ticket.
- Breaking changes to the Rust backend's public `generate()` / `generate_with_options()` entry points. Signatures are stable; internal module layout changes.
- ST's domain newtype introduction. RUSTGEN must pass through newtypes that appear in schemas but does not create them.

## What must NOT change

- `hub-codegen`'s public CLI surface (`synapse-cc generate --backend rust`). No new required flags; any new flag is opt-in with a backward-compatible default.
- TypeScript backend output — byte-identical across the epic.
- IR v2.0 compatibility — the Rust backend continues to consume the same IR shape as TS.
- `GenerationResult` struct shape — still `files: HashMap<String, String>`, same warning / deprecation-warning fields.
- The `PlexusClient` struct's public constructor (`PlexusClient::new(url)`) — downstream consumers of the current Rust backend continue to work. Internal refactors across the epic may change module locations, but the re-export from `lib.rs` preserves the public path.
- Pre-IR schemas (no `MethodRole` field) continue to generate output equivalent to pre-epic Rust output. Byte-identity is the target; any deviation must be justified in the ticket that introduces it and blessed via reblessing the snapshot.

## Completion

Epic is Complete when:

- RUSTGEN-2 through RUSTGEN-9 are all `status: Complete`.
- `cargo build` and `cargo test` in hub-codegen pass with Rust-backend tests enabled.
- Golden fixtures exist for: IR-7 deprecation rendering (Rust), IR-9 typed-handle generation (Rust), static-child generation (Rust), per-namespace partitioning (Rust). Each fixture matches the shape of its TS counterpart.
- The consumer example crate (`examples/rust_consumer/` or as picked by RUSTGEN-9) builds AND runs against a live substrate, making at least one typed dynamic-child-gate call (`client.<hub>.<gate>.get(name).await?.<typed_method>().await?`) and asserting a non-trivial return value.
- A PR description diff shows: (a) `serde_json::Value` does not appear as the `Child` associated type in any generated file; (b) the file layout under `generator/rust/` mirrors `generator/typescript/`; (c) the example crate's `cargo run` output is captured as a transcript.
- `plans/README.md` is updated with a line in "Shipped" acknowledging RUSTGEN.
