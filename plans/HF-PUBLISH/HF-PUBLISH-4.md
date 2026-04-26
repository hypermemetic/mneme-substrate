---
id: HF-PUBLISH-4
title: "hyperforge.build.publish: failure recovery — revert consumer lock, file ticket, flag held-back"
status: Pending
type: implementation
blocked_by: [HF-PUBLISH-3]
unlocks: [HF-PUBLISH-5]
severity: High
target_repo: hyperforge
---

## Problem

HF-PUBLISH-3 propagates published crates to workspace consumers and emits `PropagateStep { status: "held_back" }` when a consumer fails to rebuild. This ticket handles the held-back consumers:

1. **Revert** the consumer's Cargo.lock update (undo is possible; the publish itself is not reverted).
2. **File a ticket** at `plans/HF-AUDIT/HF-AUDIT-N.md` capturing the consumer's failure mode with enough detail for a human to investigate.
3. **Annotate** the publish summary so the operator sees the held-back list clearly.
4. **Continue** propagation to other consumers — one held-back consumer doesn't abort the whole workflow.

## Context

- Repo: `/Users/shmendez/dev/controlflow/hypermemetic/hyperforge/`.
- Version: 4.2.0.
- Touches: `src/hubs/build/propagate.rs` (extends HF-PUBLISH-3's new module).

## Required behavior

### On propagation failure

When HF-PUBLISH-3's propagate step catches a red build:

1. **Revert Cargo.lock**:
   ```rust
   std::process::Command::new("git")
       .args(["restore", "Cargo.lock"])
       .current_dir(&consumer_path)
       .output()?;
   ```
   Confirm the consumer's working tree is clean after revert.

2. **File the audit ticket**:
   - Compute next available HF-AUDIT-N in `<substrate_root>/plans/HF-AUDIT/` (scan existing IDs, pick next).
   - Write a Pending ticket with:
     - Failed consumer name + path.
     - The crate that was being propagated + version.
     - The compile error (tail of `cargo build` output, 30 lines max).
     - Reproduction steps (`cargo update -p <crate>` + `cargo build -p <consumer>`).
     - Proposed fix skeleton.
   
   Use a templated body:
   ```markdown
   ---
   id: HF-AUDIT-N
   title: "<consumer>: rebuild fails after HF-PUBLISH propagation of <crate> to <version>"
   status: Pending
   type: implementation
   blocked_by: []
   unlocks: []
   severity: Medium
   target_repo: <consumer>
   ---
   
   ## Problem
   ...
   ```

3. **Emit a `HyperforgeEvent::HeldBack { consumer, crate, version, ticket_id }`** so downstream tooling (CI, operator dashboards) picks up the state.

4. **Aggregate**: at the end of the publish run, emit a summary:
   ```
   HF-PUBLISH Summary:
     Published: N
     Propagated: M
     Held back: K (see tickets HF-AUDIT-X, HF-AUDIT-Y, ...)
   ```

### On pin-bump-needed (semver break)

When a consumer's pin range doesn't accept the new version (detected in pre-flight, or caught at propagation time):

1. If `--auto-bump-consumer-pins` flag passed:
   - Edit consumer's `Cargo.toml` to bump the pin (patch → minor → major as needed).
   - Patch-bump the consumer's own version.
   - Commit: `chore(<consumer>): bump <crate> pin to <range> [HF-PUBLISH auto-bump]`.
   - Re-run propagation for this consumer.

2. Without the flag (default):
   - Emit `HyperforgeEvent::PinBumpSuggested { consumer, crate, from_range, to_range }`.
   - File an audit ticket the same way (HF-AUDIT-N).

### Failure-cascade considerations

If consumer C1 fails propagation AND C1 is itself in the publish plan for a later crate: skip C1's later publish (don't push a broken consumer). Emit `HyperforgeEvent::PublishSkipped { package, reason: "held_back from prior propagation" }`.

### Ticket template

The auto-filed audit tickets should follow the existing HF-AUDIT pattern and use the workspace's ticketing skill conventions. Include enough structured frontmatter that bulk-querying them via the skill's `grep -rl '^status: Pending' plans/` patterns works.

## Risks

| Risk | Mitigation |
|---|---|
| Auto-filed tickets pile up without triage. | Each held-back consumer is rare (depends on real breakage). Summary event surfaces them to the operator. CI can query `grep -rl '^status: Pending' plans/HF-AUDIT/` to report. |
| Revert of Cargo.lock loses legitimate changes. | The revert is scoped to the single `cargo update -p` invocation. Pre-propagation state is captured via git before the update; `git restore Cargo.lock` returns to it. |
| Auto-filed ticket file path collision if multiple held-backs happen in the same run. | Use an atomic ID allocation: lock a counter file, or scan-and-pick-next-N in a single pass before writing. |
| Pin-bump auto-flow is controversial (makes changes to Cargo.toml without explicit human approval). | Default-off. `--auto-bump-consumer-pins` is opt-in. |

## What must NOT change

- The publish chain itself (already-published crates stay published).
- HF-PUBLISH-2's pre-flight behavior.
- HF-PUBLISH-3's successful-propagation path.
- Existing `cargo publish` / `cabal upload` invocation patterns.

## Acceptance criteria

1. `cargo build -p hyperforge` + `cargo test -p hyperforge` green.
2. Scenario: inject a broken consumer (a workspace crate that won't compile against the latest `plexus-core`). Run `hyperforge.build.publish --execute`. Expected:
   - Publish succeeds for crates in the plan.
   - Propagation to the broken consumer fails.
   - Cargo.lock reverted cleanly.
   - A new `plans/HF-AUDIT/HF-AUDIT-N.md` exists with the correct fields.
   - `HyperforgeEvent::HeldBack` emitted.
   - Other consumers continue to propagate normally.
   - End-of-run summary lists the held-back.
3. With `--auto-bump-consumer-pins` and a consumer whose pin range rejects the new version, the consumer's Cargo.toml is edited, version bumped, committed, re-propagated.
4. Scenario: consumer that fails propagation AND is in the later publish plan → its later publish is skipped with `PublishSkipped` event.

## Completion

PR against hyperforge. Status flipped Complete when the broken-consumer scenario produces a clean diagnostic + ticket AND the workspace doesn't enter a half-updated state.
