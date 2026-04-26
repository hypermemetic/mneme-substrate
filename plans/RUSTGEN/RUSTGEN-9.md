---
id: RUSTGEN-9
title: "hub-codegen Rust: consumer-facing example crate (live substrate integration)"
status: Pending
type: implementation
blocked_by: [RUSTGEN-6]
unlocks: []
severity: Medium
target_repo: hub-codegen
---

## Problem

Tests verify codegen; an example verifies the end-user experience. A Rust consumer who runs `synapse-cc generate --backend rust` against a live substrate's introspection endpoint needs to be able to drop the output into their Cargo project and call activations with typed handles. This ticket ships a reference example crate that proves the full chain works:

1. Introspect substrate (or use a snapshotted IR).
2. Run hub-codegen with the Rust backend.
3. Import the generated crate from a consumer example.
4. Call a dynamic-child-gate method with the typed-handle pattern.
5. Get a typed result.

If this works for one end-user, it works for all end-users — this is the mechanical verification that epic Complete is meaningful.

## Context

**Target layout:**

```
hub-codegen/examples/rust_consumer/
  README.md               # how to run the example
  Cargo.toml              # workspace / dep on generated crate
  src/main.rs             # example code
  generated/              # committed generated output (regenerated on CI)
    Cargo.toml
    src/
      lib.rs
      types.rs
      rpc.rs
      transport.rs
      <ns>/client.rs
      <ns>/mod.rs
      <ns>/types.rs
  scripts/regen.sh        # script that re-runs hub-codegen against a snapshot IR
  fixtures/
    substrate_ir.json     # snapshot of substrate's IR (for regen without live substrate)
```

The `generated/` directory is committed. Regen runs a script; diff should be empty on HEAD.

**Example code shape** (`src/main.rs`):

```rust
use plexus_client::*;  // depends on the generated crate

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let client = PlexusClient::new("ws://localhost:8080");

    // Static child + method call
    let health = client.plexus.health().await?;
    println!("substrate health: {:?}", health);

    // Dynamic child gate + typed handle
    let mercury: CelestialBodyClient = client.solar.body.get("mercury").await?
        .ok_or_else(|| anyhow::anyhow!("mercury not found"))?;
    let info = mercury.info().await?;
    println!("mercury: {:?}", info);

    // Listable capability
    use futures::stream::StreamExt;
    let body_names: Vec<String> = client.solar.body.list().await
        .map(|r| r.unwrap())
        .collect()
        .await;
    println!("all bodies: {:?}", body_names);

    Ok(())
}
```

**Regeneration script:** `scripts/regen.sh` runs:

```bash
#!/usr/bin/env bash
set -e
cd "$(dirname "$0")/.."
cargo run --release -p hub-codegen -- \
  --input fixtures/substrate_ir.json \
  --output generated/ \
  --backend rust \
  --package-name plexus-client
```

**Live substrate integration:** the example runs against a substrate instance. For CI, start substrate as a background process, wait for ready, run the example, assert exit 0. For local dev, document in README how to run substrate first.

## Required behavior

Deliver the example crate with:

1. A committed `generated/` directory — the output of running hub-codegen against `fixtures/substrate_ir.json`.
2. A `src/main.rs` that exercises Rpc methods, static-child accessors, dynamic-child-gate `.get(name)`, and the `Listable` capability (`.list()`).
3. A `README.md` documenting:
   - How to regenerate (`./scripts/regen.sh`).
   - How to run against a live substrate (`cargo run` with substrate on default port).
   - What the expected output looks like on a healthy substrate.
4. A `scripts/regen.sh` that invokes hub-codegen with the correct args and overwrites `generated/`.
5. A CI step (or a separate test) that:
   - Starts substrate in the background.
   - Waits for it to be ready (`ws://localhost:<port>` accepting connections).
   - Runs `cargo run -p rust_consumer`.
   - Asserts exit 0.
   - Captures the stdout transcript.
6. A regen-drift check: running `./scripts/regen.sh` produces zero git-diff on `generated/` (the committed output matches the regenerated output). This is a CI assertion.

**Failure mode to avoid:** the example silently falling back to mock data. If substrate is unreachable, the example must exit non-zero with a clear error — not pretend-succeed. This is how we detect CI regressions in the codegen chain.

## Risks

| Risk | Mitigation |
|---|---|
| Substrate's IR changes over time; the snapshot drifts. | Regen the snapshot as part of substrate's version-bump workflow. Document in README. The snapshot's IR hash is part of the commit; when substrate changes, regen and commit. |
| Port conflicts in CI: substrate default port collides. | Use an ephemeral port bound via env var `SUBSTRATE_PORT`. The example reads `SUBSTRATE_WS_URL` env var to find the substrate. |
| Substrate startup time in CI causes flaky tests. | Health-check retry loop with 30s timeout. If substrate doesn't come up, report the substrate logs and fail cleanly. |
| Generated output depends on a specific `hub-codegen` version; old example + new hub-codegen produces drift. | Regen-drift check (acceptance 6) catches this. Fix: re-run regen, commit. |
| `CelestialBodyClient` — depends on substrate exposing `solar` activation with `body` dynamic-child gate. If substrate's activation set changes (solar removed), the example breaks. | Solar is a load-bearing demo activation in substrate; unlikely to be removed. If it is, this example follows and migrates to another demo. |

## What must NOT change

- hub-codegen codegen behavior — this ticket consumes, doesn't modify.
- TypeScript backend — unchanged.
- Test suite from RUSTGEN-8 — unchanged (this ticket adds an end-to-end example, not unit tests).
- Other examples in `examples/` — unchanged.
- Substrate activations — this ticket depends on substrate's existing activation set, doesn't require substrate changes.

## Acceptance criteria

1. `cargo build -p hub-codegen` succeeds (the example is a separate crate, not part of hub-codegen's lib build — but CI runs `cargo build --all` including the example).
2. `examples/rust_consumer/` directory exists with `Cargo.toml`, `src/main.rs`, `README.md`, `scripts/regen.sh`, `fixtures/substrate_ir.json`, and `generated/` subdirectory.
3. `cargo build -p rust_consumer` succeeds (compiles against the committed `generated/`).
4. Running `cargo run -p rust_consumer` against a live substrate on `ws://localhost:<port>` succeeds (exit 0) and prints a transcript showing: at least one successful Rpc call, at least one typed dynamic-child-gate `.get(name)` returning a typed client, and at least one `.list()` stream returning a non-empty `Vec<String>`.
5. The generated crate uses the typed `Child` associated type — NOT `serde_json::Value` — for every dynamic-child gate in the generated output. Verified by `grep -rn 'serde_json::Value' examples/rust_consumer/generated/src/` returning zero matches on any gate's `type Child =` line.
6. Running `./examples/rust_consumer/scripts/regen.sh` on HEAD produces zero git-diff on `examples/rust_consumer/generated/` (committed output matches regen).
7. `examples/rust_consumer/README.md` documents the regen workflow, the live-substrate run workflow, and a sample expected-output transcript.
8. If substrate is unreachable at example run time, the example exits non-zero with an error message naming the expected URL — not a silent fallback.

## Completion

PR against hub-codegen. CI green, including a CI step that starts substrate and runs the example end-to-end. PR description includes:

- `cargo run -p rust_consumer` transcript against a live substrate.
- Confirmation that `grep serde_json::Value` on gate `Child` types returns zero matches.
- The regen-drift check confirming committed == regenerated output.

Status flipped from `Ready` to `Complete` in the same commit.
