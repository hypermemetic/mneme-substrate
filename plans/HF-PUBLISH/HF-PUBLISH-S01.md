---
id: HF-PUBLISH-S01
title: "Spike: pin HF-PUBLISH pre-flight check set, propagation algorithm, patch management semantics"
status: Pending
type: spike
blocked_by: []
unlocks: [HF-PUBLISH-2, HF-PUBLISH-3, HF-PUBLISH-4, HF-PUBLISH-5, HF-PUBLISH-6]
severity: High
target_repo: hyperforge
---

## Problem

HF-PUBLISH-1 sketches the pre-flight check set, propagation algorithm, and patch-management surface in general terms. Each has decisions that cascade into every downstream ticket:

- Which checks run at what phase of the pipeline? Some are cheap (read Cargo.toml); some are expensive (full `cargo build`). What's the fail-fast ordering?
- The propagation algorithm touches consumers after each publish. How is "consumer" defined — direct deps only, or transitive? What about dev-deps, build-deps, optional features?
- Patch management needs to atomically update a central config file (which one? workspace-root `.cargo/config.toml`? per-repo?). How does it interact with hyperforge's existing `build.unify`?

This spike pins the exact semantics before implementation tickets promote.

## Decisions to pin

### Pre-flight check set (HF-PUBLISH-2)

| Check | When | Cheap/Expensive | Failure mode |
|---|---|---|---|
| Metadata completeness | Per crate, fast | Cheap (grep Cargo.toml) | Abort pipeline |
| Version not regressed | Per crate, pre-chain | Cheap (cargo search + compare) | Abort pipeline |
| Working tree clean | Per crate, pre-chain | Cheap (git status) | Abort or --allow-dirty |
| Dep pin freshness | Per crate, pre-chain | Medium (grep + semver check) | Warn or abort (configurable) |
| Dep graph uniqueness | Per crate, pre-chain | Medium (cargo tree -d) | Abort pipeline |
| Consumer acceptance | Per newly-publishable version, pre-chain | Expensive (walk workspace + semver check) | Abort or auto-pin-bump |
| Tag collision | Per tag, pre-chain | Cheap (git ls-remote) | No-op if same commit / abort if different |

Decision: **all checks run in one pre-flight phase before ANY `cargo publish`/`cabal upload`**. Total abort if any fails. Single diagnostic report enumerating every issue across every package — no piecemeal "fix one, re-run, fix the next".

Open question: should `--strict` gate the new checks for backwards compat, or do they become default behavior at version 4.2.0? Decision: default-on at 4.2.0; `--no-preflight-X` flags for each individual check to skip selectively (for CI scenarios that need escape hatches).

### Propagation scope (HF-PUBLISH-3)

"Consumer of crate X" means any workspace crate with X in its `[dependencies]`, `[dev-dependencies]`, or `[build-dependencies]`. Transitive consumers are out of scope per-publish (they naturally pick up updates via their direct deps' propagation).

Lock file commit strategy: per-consumer commit with message `chore(C): bump X to <version> [HF-PUBLISH propagation]`. Consumer's own version bumps (if required) land in a separate commit: `chore(C): bump version to <ver> for X upgrade [HF-PUBLISH]`.

Open question: what if a consumer is ALSO in the publish chain (e.g., substrate depends on plexus-core, and both publish)? Decision: substrate publishes FIRST per dep order (it's a leaf), then substrate's propagation is a no-op (it already has the new plexus-core from when plexus-core published). Avoid double-update thrashing.

### Failure recovery (HF-PUBLISH-4)

When consumer C fails to rebuild post-update of crate X:
1. Revert C's Cargo.lock to pre-update state (git restore).
2. File a ticket at `plans/HF-AUDIT/HF-AUDIT-N.md` documenting the failure.
3. Annotate C as "held back" in hyperforge's publish output.
4. Continue propagation to other consumers (don't abort the whole propagation chain).

Decision: revert is NOT undo of the publish. The publish already happened; only the consumer's lock update gets reverted. Consumer stays on old X version until the ticket resolves.

### Patch management semantics (HF-PUBLISH-6)

Surface:
- `hyperforge.build.patch_add(crate_name, local_path)` → adds `crate_name = { path = "local_path" }` to workspace-root `.cargo/config.toml` `[patch.crates-io]`. Creates the file if absent. Warns if `crate_name` already patched.
- `hyperforge.build.patch_remove(crate_name)` → removes the entry. Pre-check: for every workspace consumer pinning `crate_name`, verify their pin range accepts the latest crates.io version. If any consumer would fail to resolve, emit a warning listing them; pass `--force true` to remove anyway.
- `hyperforge.build.patch_list()` → returns the current patch config as a structured response.

Storage: workspace-root `.cargo/config.toml` is the canonical location. The file has a clear header (`# Managed by hyperforge — use 'hyperforge build patch' to modify`) to reduce human hand-edits.

Interaction with `build.unify`: `unify` is the "reset to auto-detected" operation. It replaces the entire patch block. `patch_add` / `patch_remove` are incremental. A `patch_list` call after `unify` shows the auto-generated set.

## Required behavior

1. **Read** current `hyperforge.build.publish` source in `hyperforge/src/hubs/build/` (mod.rs + helpers). Understand the existing auto-bump logic, the cargo publish chain, the tag-creation steps.
2. **Read** `hyperforge.build.unify` source. Understand the current patch-file writer.
3. **Decide** each open question above. Write a decision document at `/Users/shmendez/dev/controlflow/hypermemetic/hyperforge/docs/architecture/HF-PUBLISH-spike.md` (or similar).
4. **Prototype minimally** if useful: one pre-flight check (e.g., metadata) wired into a branch of `publish`. Verify the integration shape before HF-PUBLISH-2 widens it.
5. **Update HF-PUBLISH-2 through HF-PUBLISH-6** with the ratified decisions.

## Acceptance criteria

1. A decision document covering each open question.
2. Updated ticket text in HF-PUBLISH-2, 3, 4, 5, 6 matching the spike's ratifications.
3. Optional: one pre-flight check prototyped in a feature branch to validate integration shape.
4. No source changes merged in this spike.

## Completion

Spike concludes with HF-PUBLISH-2 through HF-PUBLISH-6 scoped tightly. Status flipped Complete; those tickets are now unblocked for implementation.
