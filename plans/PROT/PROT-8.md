---
id: PROT-8
title: "hyperforge 5.0.0: rebuild against plexus-core 0.6; schema drill-down works; HF-AUDIT-3 closes"
status: Pending
type: implementation
blocked_by: [PROT-3, PROT-4]
unlocks: [PROT-10]
severity: Critical
target_repo: hyperforge
---

## Problem

Hyperforge is the consumer where HF-AUDIT-3 manifests: `synapse lforge hyperforge build` fails because child-accessor `.schema` dispatch returns a wrong-shape response. PROT-3's macro fix resolves the root cause. This ticket rebuilds hyperforge against the fix, bumps to 5.0.0, and verifies drill-down works.

Also: post-PROT, hyperforge's two remaining `#[allow(deprecated)]` TODO(HF-IR) markers from HF-0 become stale (the deprecated `hub` flag and associated warnings are removed in PROT-3). This ticket removes those TODO markers — they no longer suppress any warning.

## Context

- Repo: `/Users/shmendez/dev/controlflow/hypermemetic/hyperforge/`
- Version: 4.1.2 → 5.0.0 (major — wire compat break via transitive plexus-protocol).
- Local `.cargo/config.toml` with `[patch.crates-io]` — previously removed in the autonomous cleanup run. Re-add only if plexus-core 0.6 / plexus-macros 0.6 / plexus-transport 0.3 aren't yet published when this ticket runs; otherwise rely on crates.io resolution.

## Required behavior

1. **Bump pins** in `Cargo.toml`:
   ```
   plexus-core = "0.6"
   plexus-macros = "0.6"
   plexus-transport = "0.3"
   ```

2. **Rebuild**: `cargo build --all-targets` at hyperforge root. Expected: green.

3. **Remove stale `#[allow(deprecated)]`**: HF-0 added two at `src/hub.rs:476` and `src/hubs/repo.rs:105` (on `#[plexus_macros::activation(... hub)]`). HF-CLEAN already dropped the `hub` arg. PROT-3 removes the deprecation warning source. Any `#[allow(deprecated)]` referencing TODO(HF-IR) that's now moot → remove the attribute AND the TODO comment.

4. **Clippy/lint**: `cargo clippy --all-targets -- -D warnings` green. The HF-CLEAN lint posture is preserved.

5. **Runtime verification** — THE HF-AUDIT-3 REPRODUCER:
   - Kill any running hyperforge process (e.g., `kill <pid>` for the old binary).
   - Rebuild release: `cargo build --release --bin hyperforge`.
   - Start: `./target/release/hyperforge --port 44104 --no-register --no-secrets`.
   - Synapse (from PROT-6, version 4.0.0+) invokes:
     - `synapse lforge hyperforge build` → should render BuildHub's schema tree.
     - `synapse lforge hyperforge build dirty path=/Users/shmendez/dev/controlflow/hypermemetic/ all_git=true` → should stream dirty-repo events.
     - `synapse lforge hyperforge workspace` → renders WorkspaceHub tree.
     - `synapse lforge hyperforge repo` → renders RepoHub tree.

6. **Version bump** hyperforge: 4.1.2 → 5.0.0.

7. **Tag** `hyperforge-v5.0.0` locally.

8. **Flip HF-AUDIT-3 status to Complete** — the bug is fixed. Update the ticket with the root cause and reference PROT-3 as the fix.

## Risks

| Risk | Mitigation |
|---|---|
| Hyperforge's 7 activations (HyperforgeHub, WorkspaceHub, RepoHub, BuildHub, ImagesHub, ReleasesHub, AuthHub) each use `#[plexus_macros::activation]`. A macro codegen change in PROT-3 may surface latent issues. | Build + test suite must pass. 272 tests pre-existed (HF-CLEAN). All must still pass. |
| `.cargo/config.toml` was removed; if crates.io still lacks plexus-core 0.6 when this ticket runs, build fails. | Only run after PROT-10's publish step for PROT-2/3/4 OR restore `[patch.crates-io]` with local paths for 0.6. |
| Child-schema drill-down exposes a second latent bug we haven't caught. | Expected — this is the verification step. If any drill-down fails with a different error, file as HF-AUDIT-4 rather than force-passing. |
| The running hyperforge process uses an older binary (PID 18804 from the autonomous run). | Kill explicitly, rebuild, restart. Verify port 44104 unambiguously serves the new binary. |

## What must NOT change

- Hyperforge's CLI arg grammar.
- Hyperforge's activation namespaces.
- Hyperforge's method semantics.
- Zero-warning, zero-deprecated posture from HF-CLEAN — no regressions in warning count.
- `.gitignore` — unless the `.cargo/config.toml` pattern needs re-adding.

## Acceptance criteria

1. `cargo build --all-targets` at hyperforge root green.
2. `cargo test` at hyperforge root green (272+ tests).
3. `cargo clippy --all-targets -- -D warnings` green.
4. `cargo tree -d` shows single version of each plexus-* crate.
5. `grep -rn 'TODO(HF-IR)' hyperforge/src/` returns zero results.
6. Runtime: `synapse lforge hyperforge build` (via synapse 4.0.0+ against fresh hyperforge-v5.0.0 binary) successfully renders BuildHub's schema tree.
7. Runtime: `synapse lforge hyperforge build dirty path=... all_git=true` streams dirty-repo events correctly.
8. HF-AUDIT-3 ticket flipped to Complete with a reference to this ticket and PROT-3.
9. hyperforge `Cargo.toml` version is `5.0.0`. Tag `hyperforge-v5.0.0` exists locally.

## Completion

PR against hyperforge. Status flipped to Complete at PROT-10's final e2e verification.
