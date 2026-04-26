---
id: HF-TT-5
title: "Migrate Version/Commit/Branch/Tag cluster: Version, CommitRef, BranchRef, TagRef"
status: Pending
type: implementation
blocked_by: [HF-TT-2]
unlocks: [HF-TT-8]
target_repo: hyperforge
severity: Medium
---

## Problem

Every field in hyperforge that represents a version string, git commit SHA, branch name, or tag is a raw `String`. A function that takes `(commit: &str, branch: &str)` will silently compile with the arguments swapped. Version strings from different ecosystems (cargo semver, cabal version, npm version) are compared and passed around as raw strings with no type-level distinction between "a version" and "a commit reference". This ticket replaces every such field and parameter inside `hyperforge-types` and `hyperforge-core` with the corresponding newtype.

## Context

HF-TT-2 introduced `Version`, `CommitRef`, `BranchRef`, `TagRef`. `CommitRef` also ships `from_sha`, `from_tag`, `from_branch` constructors to capture the "this commit reference happens to be a tag" case without requiring the caller to upcast a `TagRef`.

Typical call sites to migrate:

- Crate/cabal/npm version fields → `Version`
- `VersionMismatch { expected: String, actual: String }` → `VersionMismatch { expected: Version, actual: Version }`
- Git commit SHAs in `GitCommit`, revision structs → `CommitRef`
- Branch names in tooling (current branch, target branch) → `BranchRef`
- Tag names in release flows → `TagRef`
- Function parameters in git adapters, package adapters, publish flows

File-boundary discipline: this ticket edits `crates/hyperforge-types/src/version.rs` (or equivalent) and `crates/hyperforge-core/src/git/` + version-touching files. It does NOT touch Repo / Package / Path / Credential / Ecosystem modules. Coordinate with HF-TT-4 for structs that have both `name` and `version` fields (see HF-TT-4's Risks).

## Required behavior

| Before | After |
|---|---|
| `pub struct CrateInfo { pub version: String, ... }` | `pub struct CrateInfo { pub version: Version, ... }` |
| `pub struct VersionMismatch { pub expected: String, pub actual: String }` | `pub struct VersionMismatch { pub expected: Version, pub actual: Version }` |
| `pub struct GitCommit { pub sha: String, ... }` | `pub struct GitCommit { pub sha: CommitRef, ... }` |
| `fn checkout(branch: &str)` | `fn checkout(branch: &BranchRef)` |
| `fn tag_release(tag: &str)` | `fn tag_release(tag: &TagRef)` |
| `fn resolve_ref(s: &str) -> CommitRef` | New helper via `CommitRef::from_sha` / `from_tag` / `from_branch`. |

Wire format preservation: all four newtypes are `#[serde(transparent)]` over `String`, byte-identical. Round-trip test `crates/hyperforge-core/tests/version_wire_compat.rs` loads fixtures for each — a `VersionMismatch`, a `GitCommit`, a branch ref, a tag ref — and confirms byte-identical re-serialization.

Hubs and bins not edited beyond minimal seam casts. Transitional seams logged.

## Risks

| Risk | Mitigation |
|---|---|
| Ecosystem-specific version validation (semver vs cabal vs npm) is tempting to bolt into `Version::new`. | Out of scope — HF-TT-2 pinned `Version` as unvalidated. Validation is a follow-up ticket if needed. |
| `CommitRef` vs `BranchRef` vs `TagRef` overlap at call sites that accept any of the three (generic "rev"). | `CommitRef::from_sha/from_tag/from_branch` constructors cover the conversion — caller converts at the seam rather than the function accepting `&str`. |
| Package-cluster files (HF-TT-4) share structs. | Coordinate ticket ordering; each line set is disjoint. |

## What must NOT change

- Wire format of `CrateInfo`, `VersionMismatch`, `GitCommit`, branch/tag-bearing structs — byte-identical round-trip test is the gate.
- Public method names on any activation.
- CLI behavior or output.
- Database / on-disk schemas.
- Files outside the Version/Commit/Branch/Tag cluster boundary.

## Acceptance criteria

1. Every version-string field uses `Version`.
2. Every git SHA field uses `CommitRef`. Every branch-name field uses `BranchRef`. Every tag-name field uses `TagRef`.
3. Every function in `hyperforge-core`'s public API that accepts any of these takes the newtype.
4. `grep -rn 'version: String' crates/hyperforge-types/` returns zero results.
5. `grep -rn 'sha: String' crates/hyperforge-types/` returns zero results.
6. Round-trip wire-compat test passes for all four fixture types.
7. `cargo build --workspace` green in hyperforge.
8. `cargo test --workspace` green in hyperforge.
9. File-boundary check: edits confined to Version/Commit/Branch/Tag cluster files plus minimal seams.
10. Sibling-repo audit: consumer repos still build.
11. `hyperforge-types` and `hyperforge-core` version bumps; tags local, not pushed.

## Completion

Implementor commits migration, fixtures, version bumps, seam inventory, confirms full workspace + consumers green, tags local, flips status to Complete in the same commit.
