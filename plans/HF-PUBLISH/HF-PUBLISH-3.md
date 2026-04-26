---
id: HF-PUBLISH-3
title: "hyperforge.build.publish: post-publish workspace propagation (cargo update + rebuild + commit per consumer)"
status: Pending
type: implementation
blocked_by: [HF-PUBLISH-2]
unlocks: [HF-PUBLISH-4]
severity: Critical
target_repo: hyperforge
---

## Problem

After `hyperforge.build.publish --execute` publishes a chain of crates, workspace consumers of those crates don't automatically pick up the new versions — their Cargo.lock files still pin the old versions. Today this required a human to `cd <consumer> && cargo update -p <crate>` manually. The 2026-04-22 run left substrate broken: plexus-registry 0.1.4 + hyperforge 4.1.3 were published, but substrate's Cargo.lock stayed on plexus-registry 0.1.3 + hyperforge 4.0.3 (old crates.io versions with pre-0.5 transitive deps), causing the compile failure that blocked everything until I manually ran `cargo update`.

This ticket adds a post-publish propagation phase: for each successfully-published crate, hyperforge walks workspace consumers, runs `cargo update -p <crate>` + `cargo build`, and commits the lock update (or reports failure for HF-PUBLISH-4 to handle).

## Context

- Repo: `/Users/shmendez/dev/controlflow/hypermemetic/hyperforge/`
- Version: 4.2.0 (continues HF-PUBLISH-2's bump).
- Touches: `src/hubs/build/mod.rs` (the `publish` method), new helpers in `src/hubs/build/propagate.rs`.

## Required behavior

### Propagation phase structure

After each successful publish (inner loop of the existing publish chain), fire the propagation step. Pseudocode:

```rust
for package in plan {
    match publish_package(package).await {
        Ok(published_version) => {
            yield HyperforgeEvent::PublishStep { success: true, ... };
            // NEW: propagate.
            propagate_to_consumers(package, published_version, &opts).await;
        }
        Err(e) => { ... }
    }
}
```

`propagate_to_consumers(crate_name, version, opts)`:

1. **Enumerate** workspace consumers — every workspace crate with `crate_name` in its `[dependencies]`, `[dev-dependencies]`, or `[build-dependencies]`.

2. **Skip self-updates** — if consumer C is also in the publish plan, it'll handle its own updates when C publishes.

3. **For each remaining consumer C**:
   - `cd C`.
   - `cargo update -p <crate_name>` → pulls latest `crate_name` into C's Cargo.lock.
   - `cargo build --all-targets -p C` → verifies C still compiles.
   - If green:
     - Stage C's Cargo.lock.
     - Commit with message: `chore(C): bump <crate_name> to <version> [HF-PUBLISH propagation]`.
     - Emit `HyperforgeEvent::PropagateStep { consumer: C, crate: crate_name, status: "propagated" }`.
   - If red (build fails):
     - `git restore Cargo.lock` to revert.
     - Emit `HyperforgeEvent::PropagateStep { consumer: C, crate: crate_name, status: "held_back", error: <compile error summary> }`.
     - HF-PUBLISH-4 picks up failure recovery.
   - If pin range doesn't accept the new version (semver break):
     - Emit a clear warning with a pin-bump suggestion.
     - HF-PUBLISH-4's consumer-pin-bump flow handles.

### Opt-out flags

- `--no-propagate` — skip the propagation phase entirely. Lock files stay as-is.
- `--no-commit` — run propagation but don't commit Cargo.lock changes (leaves workspace dirty for human review).

### Integration with existing `publish`

Add a `propagate_consumers` helper method to `BuildHub`. The `publish` method's existing inner loop gets a single new call after each successful publish. Failure-path (propagate red) doesn't abort the publish chain — later crates still publish and attempt their own propagation.

## Risks

| Risk | Mitigation |
|---|---|
| Propagation is slow on large workspaces — `cargo build` per consumer per publish. | Parallelize consumer builds (each consumer is independent). Cache target/ between propagation runs. |
| A consumer's `cargo update -p X` ripples to other crates unexpectedly. | Use `--precise <version>` when possible to constrain update scope. Post-update, `cargo tree -d` confirms no new duplicates. |
| Auto-committing lock file updates is controversial. | `--no-commit` is the escape hatch. Commit messages are clearly tagged `[HF-PUBLISH propagation]` so they're easy to audit / revert. |
| A consumer's build was already broken pre-propagation. | Pre-flight (HF-PUBLISH-2) caught this case: dep graph uniqueness + metadata checks. If those pass and the consumer still breaks, HF-PUBLISH-4 handles. |
| Propagation during a partial-chain failure (network error mid-publish). | Each successful publish independently propagates; partial progress is safe. Failed publishes don't propagate (the crate isn't on the registry yet, so `cargo update` would pull the old version). |

## What must NOT change

- Pre-flight behavior from HF-PUBLISH-2.
- Publish chain dep-tree ordering.
- Auto-bump logic.
- `--dry-run` behavior (propagation also runs in dry-run: `cargo update --dry-run` + `cargo build` but no commit).

## Acceptance criteria

1. `cargo build -p hyperforge` + `cargo test -p hyperforge` green.
2. Scenario replay: publish `plexus-registry 0.1.4` in a workspace where substrate pins `registry = "0.1.0"` (caret). Propagation to substrate:
   - `cd substrate && cargo update -p plexus-registry` pulls 0.1.4.
   - `cargo build -p plexus-substrate` green.
   - Commit `chore(plexus-substrate): bump plexus-registry to 0.1.4 [HF-PUBLISH propagation]`.
3. Running with `--no-propagate` skips the phase entirely.
4. Running with `--no-commit` does propagation + build but doesn't commit (workspace shows dirty Cargo.locks).
5. Failed propagation (consumer build red) emits `HyperforgeEvent::PropagateStep { status: "held_back", error }` and doesn't abort the publish chain.

## Completion

PR against hyperforge. Status flipped Complete when the scenario replay works end-to-end and documented failure paths emit the right events.
