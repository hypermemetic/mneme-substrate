---
id: HF-DEBUG
title: "hyperforge: add Debug to adapter/hub types (re-enable missing_debug_implementations lint)"
status: Pending
type: implementation
blocked_by: [HF-CLEAN]
unlocks: []
severity: Low
target_repo: hyperforge
---

## Problem

HF-CLEAN landed `[lints.rust]` with `missing_debug_implementations = "allow"` because hyperforge had 31 public types that wrap non-Debug foreign values (`reqwest::Client`, `Arc<dyn AuthProvider>`, `bollard::Docker`, etc.). Fixing them cleanly would require:

1. Adding `std::fmt::Debug` as a supertrait on `AuthProvider` so `Arc<dyn AuthProvider>` gains `Debug`, OR implementing `Debug` manually for each adapter/hub wrapper.
2. `#[derive(Debug)]` on every internal struct not covered above.

That's a mechanical but wider sweep than HF-CLEAN's warning-cleanup scope warranted.

## Required behavior

1. Promote `Debug` to a supertrait on `pub trait AuthProvider`. Confirm downstream impls (`YamlAuthProvider`, `KeychainAuthProvider`, test mocks) all satisfy it. For forge-port objects (`dyn ForgePort`, `dyn RegistryPort`, `dyn ReleasePort`) do the same if any are held in `Arc<dyn ...>` fields of types we want `Debug` on.
2. Add `#[derive(Debug)]` to the 31 types flagged by the lint (see HF-CLEAN Phase 3 build output).
3. For types that genuinely can't derive (e.g., holding `reqwest::Client` which is Debug-impl in recent reqwest — verify), use `#[derive(Debug)]`. Where a wrapped value truly isn't Debug, implement `Debug` manually with a stub (`write!(f, "TypeName {{ .. }}")`).
4. Flip `missing_debug_implementations` from `"allow"` to `"warn"` in hyperforge's `Cargo.toml` `[lints.rust]`.
5. `cargo clippy --all-targets -- -D warnings` stays green.

## Acceptance criteria

1. Every public struct in `hyperforge/src/**` implements `Debug` (via derive or manual).
2. `missing_debug_implementations` is `"warn"` in `Cargo.toml`, not `"allow"`.
3. `cargo build --all-targets` and `cargo clippy --all-targets -- -D warnings` both exit 0.
4. `cargo test` still passes.
5. Bump hyperforge patch version (4.1.x → 4.1.x+1) for the Debug additions.

## Notes

This is purely hygiene — no functional change. Can ship independently of HF-IR/HF-DC. Small PR, local tag, no crates.io push (same release discipline as HF-CLEAN).
