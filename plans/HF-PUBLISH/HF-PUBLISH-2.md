---
id: HF-PUBLISH-2
title: "hyperforge.build.publish: pre-flight validation phase (metadata, version regression, uncommitted, pins, dep graph, consumers, tags)"
status: Pending
type: implementation
blocked_by: [HF-PUBLISH-S01]
unlocks: [HF-PUBLISH-3]
severity: Critical
target_repo: hyperforge
---

## Problem

HF-PUBLISH-1 enumerates 7 pre-flight checks the current `hyperforge.build.publish` doesn't run (or doesn't run as strictly as it should). The 2026-04-22 publish run failed 2 packages mid-chain because pre-flight didn't catch: (a) gitvm's missing `description` + `license`, (b) plexus-codegen-rust's timestamp-hash version collision. Several other checks would have preempted issues we saw today (plexus-comms stale pin, plexus-ir/axon version regression, workspace dirty state).

This ticket implements a single pre-flight validation phase that runs every check before any `cargo publish` / `cabal upload` fires. Full fail-fast: one diagnostic report enumerates every issue across every package. Zero publishes on any failure.

## Context

- Repo: `/Users/shmendez/dev/controlflow/hypermemetic/hyperforge/`
- Version: 4.1.3 → 4.2.0 (first HF-PUBLISH ticket adding public surface).
- Touches: `src/hubs/build/mod.rs` (the `publish` method), new helpers in `src/hubs/build/preflight.rs` (or similar).

## Required behavior

### Pre-flight phase structure

Before `publish` enters the cargo-publish chain loop, it calls a `run_preflight` helper that iterates every package in the publish plan and runs each check. Returns `Result<(), PreflightFailures>` where `PreflightFailures` is a structured error enumerating per-package + per-check failures.

### Each check (per HF-PUBLISH-S01's ratified set)

1. **Metadata completeness** — for each crate targeting crates.io:
   - `Cargo.toml` has non-empty `description`.
   - `Cargo.toml` has `license` OR `license-file`.
   - For hackage targets, analogous fields on the `.cabal` file.
   
   Failure mode: per-package diagnostic listing missing fields.

2. **Version not regressed** — for each crate:
   - `cargo search <name> --limit 1` returns the latest published version.
   - Compare with local `Cargo.toml` version via semver.
   - If local < latest published, FAIL.
   - If local == latest published, it's an auto-bump-or-skip case (handled by existing logic).
   - If local > latest published, proceed.
   
   Failure mode: listing local version vs. latest published.

3. **Working tree clean** — for each package's dir:
   - `git status --porcelain` returns empty.
   - If `--allow-dirty` passed, skip this check.
   
   Failure mode: list the dirty files per package.

4. **Dep pin freshness** — for each crate in the plan, check every `[dependencies]`, `[dev-dependencies]`, `[build-dependencies]` entry for workspace-sibling crates:
   - The pinned version range accepts the latest version of the sibling (either the version being published in this run, or the latest on crates.io).
   - If not, FAIL with a clear pin-bump suggestion.
   
   Example: plexus-comms `plexus-core = "^0.3"` vs. plexus-core 0.5.0 → FAIL with "bump plexus-comms's plexus-core pin to `"0.5"`".

5. **Dep graph uniqueness** — for each crate, `cargo tree -d` should show no duplicates of workspace-internal crates:
   - Run `cargo tree -d` in the crate's dir.
   - Filter output for plexus-* / hyperforge / hub-* / synapse (workspace-internal namespaces).
   - Any duplicates → FAIL.
   
   Failure mode: the tree output + the transitive path that causes the duplicate.

6. **Consumer acceptance** — for each crate being published (at its auto-bumped target version):
   - Walk workspace for consumers that depend on this crate.
   - For each consumer, verify its pin range includes the target version.
   - If not, either auto-suggest a pin bump OR FAIL (behavior gated by a flag; default = FAIL with suggestion; `--auto-bump-consumer-pins` would auto-apply).
   
   Failure mode: consumer list + pins that need updating.

7. **Tag collision** — for each planned tag (e.g., `plexus-core-v0.5.1`):
   - `git ls-remote --tags origin <tag>` — check presence.
   - If absent: OK, tag will be created.
   - If present and points at the target commit: OK, no-op.
   - If present and points at a DIFFERENT commit: FAIL (tag conflict).
   
   Failure mode: tag + local commit vs. remote commit.

### Integration into `publish`

Pseudocode sketch:

```rust
pub async fn publish(&self, ...) -> impl Stream<...> {
    stream! {
        // 1. Build the plan (existing logic — dep-tree ordered).
        let plan = self.build_publish_plan(...);

        // 2. NEW: Pre-flight.
        yield HyperforgeEvent::Info { message: "Pre-flight validation...".into() };
        let preflight_result = self.run_preflight(&plan, &opts).await;
        if let Err(failures) = preflight_result {
            for failure in failures {
                yield HyperforgeEvent::PreflightFailure { ... };
            }
            yield HyperforgeEvent::Error { message: "Pre-flight failed. Aborting.".into() };
            return;
        }
        yield HyperforgeEvent::Info { message: "Pre-flight passed.".into() };

        // 3. Existing publish chain.
        for package in plan {
            // ... existing cargo publish / cabal upload / tag logic
        }
    }
}
```

### New HyperforgeEvent variant

`HyperforgeEvent::PreflightFailure { package_name, check_name, message, suggestion: Option<String> }` — emitted for each failed check.

## Risks

| Risk | Mitigation |
|---|---|
| Pre-flight is slow on large workspaces — runs 7 checks per package. | Parallelize per-package checks (tokio::join_all). Cache `cargo search` results for the pipeline run. |
| `cargo search` rate-limited by crates.io. | Batch all version-regression queries into one cache pass at the top of pre-flight. |
| A check produces a false positive (e.g., dep graph duplicate that's actually fine due to feature flags). | Provide per-check opt-out flags (`--no-preflight-dep-graph-uniqueness`). Document each opt-out with a concrete justification. |
| Existing `--execute` users' CI workflows break. | New behavior is default-on at 4.2.0. Update hyperforge CHANGELOG + README. Major behavior change — document in release notes. |

## What must NOT change

- The dep-tree ordering of the publish chain itself.
- Auto-bump version logic per-crate.
- The tag-creation logic (post-publish).
- Behavior when `--execute` is not passed (dry-run should simulate pre-flight too).

## Acceptance criteria

1. `cargo build -p hyperforge` green.
2. `cargo test -p hyperforge` green, including new tests for each pre-flight check.
3. Running `hyperforge.build.publish` against the 2026-04-22 scenario (dirty Cargo.locks + stale plexus-comms pin + version regressions + missing gitvm metadata) produces a **single pre-flight failure report** covering all 9 issues, zero publishes attempted.
4. Running `hyperforge.build.publish` against a clean workspace (fix all issues) passes pre-flight and proceeds to publish.
5. Hyperforge bumped to 4.2.0; local tag `hyperforge-v4.2.0` created.

## Completion

PR against hyperforge. Status flipped Complete when the 2026-04-22 scenario produces the expected diagnostic and a clean workspace still publishes correctly.
