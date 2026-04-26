---
id: HF-PUBLISH-6
title: "hyperforge.build.patch: add / remove / list workspace crates.io patches (with safety check on remove)"
status: Pending
type: implementation
blocked_by: [HF-PUBLISH-S01]
unlocks: []
severity: High
target_repo: hyperforge
---

## Problem

Today, managing `[patch.crates-io]` entries in workspace-root `.cargo/config.toml` is manual:

- For local iteration on an in-flight crate version, you edit the config file by hand.
- For removing patches after publishing, you `rm` the file or edit out specific entries.
- There's no safety check that removal won't break workspace builds. I demonstrated this painfully on 2026-04-22 when the autonomous cleanup removed all patches and silently broke substrate's transitive dep graph.

Hyperforge already has `build.unify` which auto-generates a full patch block from detected workspace crates. It's an all-or-nothing reset. What's missing is **incremental patch management** + **pre-removal safety checks**.

This ticket adds three methods to `BuildHub`:

- `patch_add(crate_name, local_path)` — adds a single patch entry.
- `patch_remove(crate_name, --force?)` — removes a single entry, with pre-removal safety check.
- `patch_list()` — returns the current patch block as structured data.

## Context

- Repo: `/Users/shmendez/dev/controlflow/hypermemetic/hyperforge/`.
- Version: 4.2.0.
- Touches: `src/hubs/build/mod.rs` (new methods), `src/build_system/patch.rs` (new helper module).

The canonical file location for workspace patches is `<workspace-root>/.cargo/config.toml`. Per-repo patches at `<repo>/.cargo/config.toml` exist too (hyperforge and substrate both had them transiently) and are handled by the same methods — parameter `--scope workspace|repo:<name>` selects target.

## Required behavior

### `patch_add(crate_name, local_path, scope?)`

Params:
- `crate_name: String` — the crate being patched.
- `local_path: PathBuf` — the local path to redirect to.
- `scope: Option<String>` — default `"workspace"`; `"repo:<name>"` targets a specific repo's `.cargo/config.toml`.

Behavior:
1. Resolve the target `.cargo/config.toml` file path. Create it (and parent dirs) if absent.
2. Parse the existing TOML (if any) using `toml_edit` to preserve formatting.
3. Ensure the file has a clear header comment (`# Managed by hyperforge — use 'hyperforge build patch' to modify`) and is gitignored.
4. If `[patch.crates-io]` section exists:
   - Check if `crate_name` already has an entry. If yes, emit `HyperforgeEvent::PatchExists` with a warning, update with new path.
   - Otherwise, append the entry.
5. If no `[patch.crates-io]` section: create it + add the entry.
6. Write file atomically (tempfile + rename).
7. Verify via `cargo tree -d` that the patch resolves correctly (no duplicate crates introduced).
8. Emit `HyperforgeEvent::PatchAdded { crate_name, local_path, scope }`.

### `patch_remove(crate_name, scope?, force?)`

Params:
- `crate_name: String`
- `scope: Option<String>` — default `"workspace"`.
- `force: Option<bool>` — default `false`. Skips the safety check.

Behavior:
1. Resolve target `.cargo/config.toml`. If absent, emit a no-op success.
2. Parse TOML. If `crate_name` not in `[patch.crates-io]`, emit no-op success.
3. **Safety check** (unless `force` is true):
   - Compute the `crate_name`'s current local version (from the path-patch target).
   - Query crates.io for the latest published version of `crate_name`.
   - For each workspace crate that depends on `crate_name`:
     - Read its pinned version range.
     - Semver-check: does the latest crates.io version satisfy the range?
     - If NO, emit `HyperforgeEvent::PatchRemovalBlocked { consumer, crate_name, pin_range, latest_published }` for each failing consumer.
   - If any blocking consumers, do NOT remove; return a structured error with the full list.
4. If safe (or `force` passed): remove the entry. Write file atomically. If the `[patch.crates-io]` section becomes empty, remove the section header too.
5. Verify via `cargo tree -d` workspace-wide that no duplicates surface post-removal.
6. Emit `HyperforgeEvent::PatchRemoved { crate_name, scope }`.

### `patch_list(scope?)`

Params:
- `scope: Option<String>` — default `"workspace"`.

Returns:
```rust
struct PatchEntry {
    crate_name: String,
    local_path: PathBuf,
    source_of_truth_version: Option<String>,  // version in Cargo.toml at local_path
    latest_published: Option<String>,         // from crates.io
}
```

Emits `HyperforgeEvent::PatchListed { patches: Vec<PatchEntry> }`.

### Interaction with `build.unify`

`unify` is the "full regen from auto-detected workspace" operation — replaces the entire patch block. `patch_*` methods are incremental. After calling `unify`, `patch_list` shows the auto-generated set.

### CLI surface

These methods are accessible via synapse:
```
synapse lforge hyperforge build patch_add --crate-name plexus-core --local-path ../plexus-core
synapse lforge hyperforge build patch_remove --crate-name plexus-core
synapse lforge hyperforge build patch_remove --crate-name plexus-core --force true
synapse lforge hyperforge build patch_list
```

## Risks

| Risk | Mitigation |
|---|---|
| Users hand-edit `.cargo/config.toml` AND use these methods → conflicts. | The header comment warns against hand-editing. `patch_list` is non-destructive; users can diff before/after. |
| Safety check is slow (queries crates.io for each removable patch). | Cache crates.io queries in the run. `patch_remove` typical case is one crate; acceptable. |
| A consumer accepts the latest version's range but would actually FAIL to compile due to API changes. | The safety check covers version-range semver only, not API compat. That's what HF-PUBLISH-3's propagation rebuild catches. `patch_remove --force` + full workspace rebuild is the integration test. |
| Atomic file write failure. | Tempfile-and-rename is standard. Fallback: skip on write error, emit `PatchEdit::Failed`. |
| `--scope repo:<name>` path resolution — which .cargo/config.toml to target. | Use hyperforge's workspace metadata (it already knows which repos have `.cargo/` dirs). |

## What must NOT change

- `build.unify` behavior (stays as the all-or-nothing regen).
- Workspace-root or per-repo `.cargo/config.toml` formatting conventions.
- `.gitignore` rules for `.cargo/` files (these files remain gitignored).

## Acceptance criteria

1. `cargo build -p hyperforge` + `cargo test -p hyperforge` green.
2. `patch_add plexus-core ../plexus-core` creates/updates `<workspace>/.cargo/config.toml` with the entry, verifiable via `patch_list`.
3. `patch_remove plexus-core` WITHOUT `--force` in a scenario where a consumer pins `plexus-core = "^0.5"` and crates.io has `0.5.0` → succeeds.
4. `patch_remove plexus-core` WITHOUT `--force` in a scenario where a consumer pins `plexus-core = "^0.3"` and crates.io has only `0.5.0` → FAILS with a structured error listing the consumer + pin.
5. `patch_remove plexus-core --force true` in the same scenario → succeeds. Caller accepts the workspace may break.
6. `patch_list` returns structured data for each patch entry including local version + latest-published version.
7. Integration test (under HF-PUBLISH-5's harness) exercises each method.

## Completion

PR against hyperforge. Status flipped Complete when:
- All 7 acceptance criteria pass.
- The safety check prevents a re-occurrence of the 2026-04-22 autonomous-cleanup failure mode (patches removed, substrate builds break silently).
- `patch_list` output is clear enough that a human can audit patch state in one glance.
