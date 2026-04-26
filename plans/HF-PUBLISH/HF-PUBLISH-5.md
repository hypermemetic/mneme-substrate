---
id: HF-PUBLISH-5
title: "hyperforge.build.publish: end-to-end integration test harness"
status: Pending
type: implementation
blocked_by: [HF-PUBLISH-4]
unlocks: []
severity: High
target_repo: hyperforge
---

## Problem

HF-PUBLISH-2/3/4 add substantial new behavior to the publish pipeline. Unit tests cover individual pieces (metadata check, consumer enumeration, etc.), but an integration test exercising the FULL pipeline end-to-end — on realistic workspace scenarios — is needed to catch cross-phase regressions.

This ticket builds a scriptable test harness: set up a fake workspace with specific states (dirty, pin-stale, regression, broken-consumer, etc.), run hyperforge.build.publish, assert the outcome.

## Context

- Repo: `/Users/shmendez/dev/controlflow/hypermemetic/hyperforge/`.
- New file: `tests/integration/publish_pipeline_tests.rs`.
- Testing strategy: build a synthetic workspace under a tempdir, populated with a small handful of cargo workspaces + `.cargo/config.toml` files. Run hyperforge commands against it via library API (not synapse over-the-wire — too slow for tests).

## Required behavior

### Test scenarios

Each scenario sets up a fixture workspace and asserts publish behavior.

1. **Clean workspace, all-green**: 3 workspace crates with proper metadata, valid pins, clean trees. Run `publish --execute` against a fake registry (mock `cargo publish`). Assert 3 publishes + 3 propagations + 0 held-back.

2. **Missing metadata**: 1 crate without `description`. Run `publish`. Assert pre-flight fails with a `PreflightFailure` event naming the missing field. Zero publishes.

3. **Version regression**: 1 crate with `Cargo.toml` version `0.1.0` while registry has `0.3.0`. Assert pre-flight fails.

4. **Dirty working tree**: 1 crate with an uncommitted Cargo.lock. Assert pre-flight fails (or passes if `--allow-dirty`).

5. **Stale pin**: consumer pins `^0.3` while publishable version is `0.5`. Assert pre-flight surfaces a pin-bump suggestion.

6. **Dep graph duplicate**: workspace has both direct and transitive versions of a crate. Assert pre-flight fails with the tree excerpt.

7. **Consumer held-back**: publish succeeds, but a consumer fails to rebuild post-propagation (simulated via broken source). Assert:
   - Consumer's Cargo.lock reverted.
   - New HF-AUDIT-N ticket file created under a fake `plans/HF-AUDIT/`.
   - `HyperforgeEvent::HeldBack` emitted.
   - End-of-run summary lists the held-back.

8. **Auto-bump consumer pin**: consumer pin rejects new version; `--auto-bump-consumer-pins` flag triggers pin edit + version bump + re-propagation.

9. **Tag already exists, same commit**: idempotent re-run, should be a no-op.

10. **Tag already exists, different commit**: should fail with tag conflict.

### Test harness helpers

```rust
pub struct TestWorkspace { tempdir: TempDir, ... }

impl TestWorkspace {
    pub fn new() -> Self { ... }
    pub fn with_crate(self, name, version, deps, metadata) -> Self { ... }
    pub fn with_consumer(self, consumer_name, depends_on, pin_range) -> Self { ... }
    pub fn with_dirty_lock(self, crate_name) -> Self { ... }
    pub fn with_registry_version(self, crate_name, version) -> Self { ... }
    pub fn publish(self, opts) -> PublishResult { ... }
}
```

Fake registry backing: in-memory map of `(crate_name, version) -> bool` that `cargo search` / `cargo publish` equivalents mock against.

### Fake `cargo publish`

Tests shouldn't hit real crates.io. Mock the publish step:

- `hyperforge` grows a hidden `--registry-mock` flag (test-only, behind a `cfg(test)` or feature flag).
- When set, cargo invocations use a local registry path instead of crates.io.
- Alternatively: intercept the `cargo publish` spawn and return success/failure per test config.

### Assertions

Each test asserts:
- Expected events emitted in order.
- Expected file state after run (Cargo.locks, Cargo.tomls, new audit tickets).
- Expected commit log entries (git log at test workspace's repo).
- Zero side effects leaking outside the tempdir.

## Risks

| Risk | Mitigation |
|---|---|
| Setting up realistic fake workspaces is verbose. | Invest in `TestWorkspace` builder helpers. Reuse across scenarios. |
| Fake registry is complex. | Start with the simplest mock: version-map + spawn-interceptor. Avoid building a full crates.io clone. |
| Tests are slow (each spawns cargo). | Parallelize test cases (tempdirs are isolated). Use `--test-threads=auto`. |
| Maintaining test fixtures as hyperforge evolves. | Keep scenarios small (3-5 crates each). Fixtures are inline-built, not checked-in. |

## What must NOT change

- Nothing in the publish pipeline proper. This ticket is purely test harness.
- No new hyperforge runtime features.
- No external test dependencies beyond what's already in hyperforge's `dev-dependencies`.

## Acceptance criteria

1. `cargo test -p hyperforge --test publish_pipeline_tests` runs all 10 scenarios, all pass.
2. Scenarios are independent (no test pollution).
3. Adding a new scenario is a ~50-line addition to the test file.
4. Running the integration tests against a regression (e.g., break HF-PUBLISH-2's pre-flight on purpose) surfaces the break with a clear failure message.
5. Hyperforge version bump final `4.2.0` → tag `hyperforge-v4.2.0`.

## Completion

PR against hyperforge. Status flipped Complete when all 10 scenarios pass in CI. Closes HF-PUBLISH epic.
