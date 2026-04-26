---
id: HF-PUBLISH-1
title: "HF-PUBLISH epic — hyperforge.build.publish: pre-flight validation + workspace upgrade propagation"
status: Epic
type: epic
blocked_by: []
unlocks: [HF-PUBLISH-S01, HF-PUBLISH-2, HF-PUBLISH-3, HF-PUBLISH-4, HF-PUBLISH-5, HF-PUBLISH-6]
target_repo: hyperforge
---

## Goal

End state: `hyperforge.build.publish --execute` is a **complete, self-validating, self-propagating** workspace-upgrade operation. Running it (a) fails safe before publishing anything if downstream consumers would break, and (b) after successful publish, automatically updates every workspace consumer's lock file to pull the new versions, rebuilds each, and commits the lock updates — leaving the workspace in a green, coherent state.

Today's gaps the user observed running the 13-package publish on 2026-04-22:

- **gitvm** failed publish because its `Cargo.toml` lacks `description` + `license`. Should have been a pre-flight failure, not a 10-minutes-in surprise.
- **plexus-codegen-rust** collided on a timestamp-hash version (`0.1.20260125170504-32e46854`). Should have either regenerated the timestamp or detected the collision pre-publish.
- **plexus-substrate**'s transitive deps resolved to old `hyperforge 4.0.3` + `plexus-registry 0.1.3` + `plexus-transport 0.1.8` post-publish-removal-of-patches. `cargo build -p plexus-substrate` failed. Fix required manual `cargo update` + rebuild. Hyperforge published new versions of those transitive deps but didn't propagate them to substrate's Cargo.lock — substrate stayed broken until a human intervened.
- **Stale pins** like `plexus-comms`'s `plexus-core = "^0.3"` weren't flagged by publish pre-flight — just failed mid-chain.
- **Version regressions** (e.g., local `0.1.0` < published `0.3.6` on plexus-ir) failed mid-chain. Pre-flight should catch.
- **Tag-already-exists warnings** appeared when tags were pushed in a prior manual run (plexus-protocol-v0.5.0.0, hyperforge-v4.1.3). Pipeline should recognize "tag present on remote, nothing to do" as clean, not warn.

## Phase structure

| Phase | Ticket | Scope |
|---|---|---|
| 0. Spike | HF-PUBLISH-S01 | Pin the exact pre-flight check set and the propagation algorithm. Binary-pass. |
| 1. Pre-flight validation | HF-PUBLISH-2 | Every check runs before any `cargo publish` / `cabal upload` is attempted. If ANY check fails, the whole pipeline aborts with a complete diagnostic report. Zero packages published. |
| 2. Post-publish propagation | HF-PUBLISH-3 | After each successful publish of crate X, hyperforge walks every workspace consumer of X, runs `cargo update -p X`, rebuilds, verifies, commits lock file update. |
| 3. Failure recovery | HF-PUBLISH-4 | When a consumer fails to rebuild post-update, revert its lock file change, file a ticket, flag the consumer as needing manual attention. Keep the publish result — don't retroactively yank. |
| 4. End-to-end integration | HF-PUBLISH-5 | Integration test harness: scripted workspace scenarios (clean, broken-pin, version-regression, metadata-missing) verify each branch of the pipeline behaves correctly. |
| 5. Patch management | HF-PUBLISH-6 | `patch_add` / `patch_remove` / `patch_list` methods with pre-removal safety check. Prevents the 2026-04-22 failure mode where removing patches silently broke substrate. Runnable via `synapse lforge hyperforge build patch_*`. |

## Proposed pre-flight checks (ratified in HF-PUBLISH-S01)

For each package targeted for publish:

1. **Metadata completeness** — `Cargo.toml` has `description`, `license` (or `license-file`). For `cabal` packages, analogous: `synopsis`, `license`. (Missing metadata failed gitvm today.)
2. **Version not regressed** — `local_version >= published_latest`. (Caught plexus-ir / axon regressions.)
3. **Working tree clean** — no uncommitted `Cargo.lock` or source drift for the package's dir, unless `--allow-dirty` passed. (Caught the 5-dirty-Cargo.lock group.)
4. **Dep pin freshness** — any workspace-internal dep pinned at a version range that doesn't accept the currently-published latest is flagged. (Caught plexus-comms's `plexus-core = "^0.3"` vs 0.5.0.)
5. **Dep graph uniqueness** — `cargo tree -d` for the package has no duplicate plexus-* / workspace-internal crate versions. (Would have caught substrate's dual-version issue preemptively.)
6. **Consumer acceptance** — for each workspace consumer of a being-published package, check the consumer's pin range includes the new version. If not, either prompt for a pin bump OR flag as a breaking change requiring consumer author action.
7. **Tag collision** — for each package with a planned tag, check if the tag already exists on origin. If yes AND points at the target commit, treat as no-op (green). If yes AND points at a different commit, flag as conflict.

## Proposed post-publish propagation algorithm (ratified in HF-PUBLISH-S01)

For each successfully-published crate X (in dep order as the chain progresses):

1. **Enumerate workspace consumers** of X (via `cargo metadata` or Cargo.toml grep across workspace).
2. **For each consumer C**:
   a. `cd C && cargo update -p X` — pulls latest X into C's Cargo.lock.
   b. `cargo build -p C` — verifies C still compiles with the new X.
   c. **If green**: stage and commit Cargo.lock change with a descriptive message (`chore(C): bump X to <version> [HF-PUBLISH propagation]`).
   d. **If red**: see HF-PUBLISH-4 (failure recovery).
3. **Per-consumer version bump** — if the update was semver-breaking (pin had to change), auto-bump the consumer's own version (patch for lock-only updates, minor for pin changes).
4. **Report** each consumer's outcome (green-and-propagated / broken-and-flagged / pin-bump-needed) in the publish summary.

## Cross-cutting contracts pinned here

- **Two-phase commit.** Publish chain commits to crates.io/hackage only AFTER every pre-flight check passes for every package in the plan. No "some published, some failed" mid-execute states driven by lazy validation. Failed publishes mid-chain (e.g., network error on one crate) are the only acceptable interruption.
- **Idempotency.** Re-running `hyperforge.build.publish --execute` on a clean workspace is a no-op. Tags already present, versions already published, Cargo.lock already updated = no work.
- **Dry-run fidelity.** `hyperforge.build.publish` (no `--execute`) runs ALL pre-flight checks and propagation dry-runs. The dry-run output must predict the execute run exactly (modulo network-level failures).
- **Failure atomicity per-crate.** If a single crate's publish fails (e.g., missing metadata), subsequent crates in the chain that don't depend on the failing crate still proceed. If they DO depend on it, they're skipped (not failed).
- **Lock file commits opt-outable.** `--no-commit` skips the auto-commit of propagated lock files; the update still happens, but the workspace is left dirty for the human to review.

## What must NOT change

- The existing `hyperforge.build.publish` signature in ways that break CI scripts or existing invocations. Parameters are strictly additive (`--strict` for new pre-flight behavior, `--no-propagate` to opt out of consumer updates, etc.).
- Cargo.toml auto-bump behavior for author versions (HF-PUBLISH doesn't rewrite author-visible versions; that's existing `auto_bump` per-crate logic).
- Dep-tree ordering of publishes. HF-PUBLISH adds checks around the existing order; it doesn't reorder.
- Any `plexus_macros::*`, `plexus_core::*`, `synapse` behavior. HF-PUBLISH touches hyperforge only.

## Out of scope

- Multi-registry publishing (e.g., simultaneous crates.io + private registry). Current scope is single-registry-per-crate.
- Cross-workspace publishing (e.g., publishing from a parent monorepo into multiple sub-workspaces). Single workspace.
- Rollback of publishes (crates.io doesn't support un-publish; yank is out of scope too).
- Publishing to registries other than crates.io / hackage. If other registries are needed, file a follow-up.
- Retro-fitting this behavior into an older `hyperforge` version. Ships at the current 4.x line + forward.

## Dependency DAG

```
          HF-PUBLISH-S01 (spike: pin check set + algorithm)
                  │
                  ▼
          HF-PUBLISH-2 (pre-flight validation)
                  │
                  ▼
          HF-PUBLISH-3 (post-publish propagation)
                  │
                  ▼
          HF-PUBLISH-4 (failure recovery)
                  │
                  ▼
          HF-PUBLISH-5 (e2e integration test)
```

Strictly serial — each phase consumes the previous phase's output. Parallel fanout doesn't make sense here; they're layered concerns.

## Version bump

hyperforge 4.1.3 → 4.2.0 on the first HF-PUBLISH ticket that adds public surface (likely HF-PUBLISH-2). Subsequent HF-PUBLISH tickets contribute to 4.2.0 per `feedback_version_bumps_as_you_go.md`. Local tag `hyperforge-v4.2.0` when the final HF-PUBLISH ticket lands.

## Completion

Epic is Complete when:

- HF-PUBLISH-S01 through HF-PUBLISH-5 are all Complete.
- Re-running the 2026-04-22 scenario (dirty Cargo.locks + stale plexus-comms pin + version regressions on plexus-ir/axon + missing gitvm metadata) against the new pipeline produces a single pre-flight failure report, zero publishes attempted.
- A clean workspace with valid versions and pins produces a clean `--execute` run where every published crate auto-propagates to its consumers with verified green rebuilds.
- The earlier HF-AUDIT-3 / HF-0-style regressions (introduced by patch removal, dual-version dep graphs) are preempted by pre-flight check 5 (dep graph uniqueness).
