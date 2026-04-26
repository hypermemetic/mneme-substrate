---
id: RUSTGEN-7
title: "hub-codegen Rust: Cargo.toml + package manifest generation (parity with TS package.rs)"
status: Pending
type: implementation
blocked_by: [RUSTGEN-5]
unlocks: []
severity: Medium
target_repo: hub-codegen
---

## Problem

The current Rust backend emits a hardcoded `Cargo.toml` string in `generator/rust/mod.rs::generate_cargo_toml`. The TypeScript backend has a dedicated `package.rs` module that generates a more structured `package.json` with proper metadata, dependencies, versioning, and consumer-facing fields. The Rust backend needs a parallel: `generator/rust/package.rs` producing a well-formed `Cargo.toml`.

Goals for the generated `Cargo.toml`:

- Correct dependencies for whichever runtime-library shape RUSTGEN-S01 pinned (inline / sibling crate / vendored).
- Correct transport dep from RUSTGEN-S02 (tokio-tungstenite / jsonrpsee / bundled).
- Version string derived from IR metadata (or default `0.1.0`).
- Package name derived from IR / config (or default `plexus-client`).
- Appropriate `edition`, `description`, `repository` (if IR has a source field).
- Deterministic output (no timestamps / random ordering).

## Context

**TS reference:** `hub-codegen/src/generator/typescript/package.rs` — 108 lines. Generates `package.json` with `name`, `version`, `description`, `main`, `types`, `dependencies`, `devDependencies`. Dev deps include TypeScript compiler / test framework. Prod deps include the WebSocket lib.

**Current Rust output** (from `generator/rust/mod.rs::generate_cargo_toml`):

```toml
[package]
name = "plexus-client"
version = "0.1.0"
edition = "2021"
description = "Auto-generated Plexus client"

[dependencies]
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
tokio = { version = "1.0", features = ["full"] }
tokio-tungstenite = "0.21"
futures = "0.3"
anyhow = "1.0"
async-stream = "0.3"
thiserror = "1.0"

[dev-dependencies]
tokio-test = "0.4"
```

This is hardcoded — no IR-driven config, no runtime-library-shape variation, no name customization.

**What this ticket changes:**

1. Extract `generate_cargo_toml` into `generator/rust/package.rs`.
2. Accept a configuration source: IR metadata (new fields in `ir.ir_metadata` if needed, or a new `GenerationOptions` struct field). Name / version / description pulled from config; defaults to current hardcoded values if config absent.
3. Generate deps based on RUSTGEN-S01's pinned runtime-library shape:
   - Option A (inline): current dep set.
   - Option B (sibling crate): add `plexus-client-runtime = "0.1"` (or path dep in dev contexts).
   - Option C (vendored): add `plexus-transport = { path = "..." }`.
4. Generate transport deps based on RUSTGEN-S02's pinned transport choice:
   - Option A (tokio-tungstenite status quo): no change.
   - Option B (jsonrpsee): swap to `jsonrpsee = { version = "0.22", features = ["client-ws"] }`.
   - Option C (bundled): tokio-tungstenite + additional framing deps.
5. Add `[dev-dependencies]` entries for smoke tests (RUSTGEN-8 consumes these).

## Required behavior

Emit a file `Cargo.toml` at the top level of the generated crate. Content:

| Section | Contract |
|---|---|
| `[package]` | `name`, `version`, `edition`, `description`. Values sourced from IR config with documented defaults. |
| `[dependencies]` | Runtime deps determined by S01 + S02 pins + IR requirements. Always includes `serde`, `serde_json`, `anyhow`, `thiserror`, `tokio`, `futures`. Optionally `async-stream`. Transport dep from S02. Runtime-library dep from S01 (if sibling crate). |
| `[dev-dependencies]` | `tokio-test`. Additional test-only deps as needed for RUSTGEN-8 smoke tests. |
| (optional) `[features]` | If the runtime-library-shape supports feature-gating (e.g., `tls` feature), expose them. Out of scope unless S01 mandates. |

**Determinism:**

- Key order within each section is alphabetical.
- Section order is fixed: `[package]`, `[dependencies]`, `[dev-dependencies]`, `[features]`.
- No timestamps, no random identifiers, no IR hash in the version (version is `0.1.0` by default; IR hash is metadata).
- Dep version strings match the ones RUSTGEN-S01 / S02 pin.

**Config-driven name/version:** if `GenerationOptions` carries a `crate_name: Option<String>` field, use it; else default `plexus-client`. Same for `crate_version: Option<String>` (default `0.1.0`) and `description: Option<String>` (default `"Auto-generated Plexus client"`).

**Metadata comment:** the current backend embeds the IR hash in the description: `description = "Auto-generated Plexus client (hash: <hex>)"`. Preserve this behavior — the hash-in-description is how cache-hit detection works for cached regens. The hash is not in the `version` field.

## Risks

| Risk | Mitigation |
|---|---|
| Dependency version drift — `tokio-tungstenite` or `serde` bumps between this ticket and a future regen produce non-byte-identical output. | Pin version strings as constants in `package.rs` source. When the dep version changes, it's an explicit code edit, not environmental drift. |
| Runtime-library-shape pin from S01 isn't one of A/B/C. | S01 commits to one of the three or fails → replanning triggers; this ticket's content then reflects the replanning outcome. |
| `crate_name` from config collides with an existing crate on crates.io. | Not this ticket's concern. If the consumer wants to publish, they set `crate_name` to something they own. Default `plexus-client` is not reserved — consumers who publish must rename. |
| TOML emission: handwritten string vs `toml` crate. | Handwritten string. The output is small and deterministic; adding a `toml` dep for emission is overkill. Acceptance 5 pins byte-identity. |

## What must NOT change

- TypeScript backend `package.json` generation — unchanged.
- Per-namespace client / types / rpc / transport file generation — unchanged.
- `GenerationResult.files` map shape — unchanged. `Cargo.toml` remains a top-level entry.
- The hash-in-description behavior — preserved.
- `edition = "2021"` — unchanged (edition 2024 is a future consideration, outside this ticket).
- Consumers doing `cargo build` on the generated crate continue to work. Dep changes are additive or equivalent (same crate, same or newer version).

## Acceptance criteria

1. `cargo build -p hub-codegen` and `cargo test -p hub-codegen` succeed.
2. A fixture IR produces a `Cargo.toml` whose `[package]` name, version, description match the IR config (or defaults if unset).
3. The generated `Cargo.toml` `[dependencies]` section matches RUSTGEN-S01 + S02 pins. Verified by the dependency set listed in the golden fixture.
4. The generated crate compiles: `cargo build` on the generated crate (with a mock-mode transport that doesn't open a WS) succeeds.
5. Two consecutive generator runs produce byte-identical `Cargo.toml`.
6. `generator/rust/package.rs` exists and contains the generation logic (extracted from `mod.rs`).
7. `GenerationOptions` (or the equivalent config struct) accepts `crate_name`, `crate_version`, `description` fields with documented defaults; setting any of them reflects in the generated `Cargo.toml`.
8. The IR hash appears in the `description` field of the generated `Cargo.toml` when the IR carries a hash (preserves current behavior).

## Completion

PR against hub-codegen. CI green. PR description includes a sample generated `Cargo.toml` (before vs after). Golden fixtures reblessed if the content changes due to S01/S02 pinning. Status flipped from `Ready` to `Complete` in the same commit.
