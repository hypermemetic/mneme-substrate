---
id: HF-TT-4
title: "Migrate Package/Artifact cluster: PackageName, ArtifactId"
status: Pending
type: implementation
blocked_by: [HF-TT-2, HF-TT-S01]
unlocks: [HF-TT-8]
target_repo: hyperforge
severity: Medium
---

## Problem

Every field in hyperforge that represents a package name or artifact identifier is a raw `String`. `PackageName` and `RepoName` are indistinguishable to the compiler today — they both wrap a plain string and silently swap at any call site that takes two string parameters. Artifact identifiers (release IDs, image tags, registry references) share the same risk. This ticket replaces every such field and parameter inside `hyperforge-types` and `hyperforge-core` with the corresponding newtype.

## Context

HF-TT-2 introduced `PackageName` and `ArtifactId`. HF-TT-S01 pinned `ArtifactId`'s shape — either plain `String` newtype or parsed struct `{ ecosystem: Ecosystem, namespace: String, name: PackageName }`. This ticket adopts whichever shape S01 ratified.

Typical call sites to migrate, per HF-0:

- `Package.name: String` → `PackageName`
- `CrateInfo.name: String` → `PackageName`
- `PackageInfo.name: String` → `PackageName`
- Artifact identifiers across image and release types → `ArtifactId`
- Function parameters in package / artifact business logic
- `HashMap<String, Package>` keyed by package name → `HashMap<PackageName, Package>`

File-boundary discipline: this ticket edits `crates/hyperforge-types/src/package.rs` (or equivalent) and `crates/hyperforge-core/src/package/` + `crates/hyperforge-core/src/adapters/registry/` files. It does NOT touch Repo / Version / Path / Credential / Ecosystem modules. Files jointly owned with HF-TT-5 (e.g., a `PackageInfo` struct with both `name: PackageName` and `version: Version` fields) are coordinated: this ticket migrates the `name` field, HF-TT-5 migrates `version`. Both tickets edit the same file but disjoint lines — split the file edits into sequential commits if needed, or coordinate via ticket ordering.

## Required behavior

| Before | After |
|---|---|
| `pub struct Package { pub name: String, ... }` | `pub struct Package { pub name: PackageName, ... }` |
| `pub struct CrateInfo { pub name: String, ... }` | `pub struct CrateInfo { pub name: PackageName, ... }` |
| `pub struct PackageInfo { pub name: String, ... }` | `pub struct PackageInfo { pub name: PackageName, ... }` |
| Artifact identifier fields (`image_id: String`, `release_id: String`, etc.) | `ArtifactId` |
| `fn publish_package(name: &str) -> ...` | `fn publish_package(name: &PackageName) -> ...` |
| `HashMap<String, Package>` (keyed by name) | `HashMap<PackageName, Package>` |

Wire format preservation: `PackageName` is `#[serde(transparent)]` over `String`, byte-identical. `ArtifactId` preserves wire format per S01's decision: if plain, byte-identical; if parsed, `Display` and `FromStr` round-trip `<ecosystem>:<namespace>:<name>` and `#[serde(into = "String", try_from = "String")]` is used so the wire stays a plain string. A round-trip test in `crates/hyperforge-core/tests/package_wire_compat.rs` loads realistic `PackageInfo` and artifact-identifier fixtures and confirms byte-identical re-serialization.

Hubs and bins are NOT edited in this ticket beyond minimal seam casts to keep the workspace building. Transitional seams logged in commit message for HF-TT-8 to sweep.

## Risks

| Risk | Mitigation |
|---|---|
| `ArtifactId` parsed form fails on a legacy fixture that doesn't fit `<ecosystem>:<namespace>:<name>`. | Round-trip test includes at least one legacy-shape fixture. If it fails, S01's shape decision is wrong and this ticket blocks on a revised spike rather than forcing. |
| A single struct contains both `PackageName` and `Version` fields; edits in this ticket overlap with HF-TT-5. | Coordinate via ordering — complete HF-TT-4 first, HF-TT-5 picks up the version field after. Or split edits across commits within one working session, as long as each file lands disjoint line sets. |
| `PackageName` gets confused with `RepoName` at a seam where both appear. | This is the exact bug the newtype prevents. The compile error at HF-TT-8's sweep is the success signal. |

## What must NOT change

- Wire format of `Package`, `CrateInfo`, `PackageInfo`, artifact types — byte-identical round-trip test is the gate.
- Public method names on any activation.
- CLI behavior or output.
- Database / on-disk cache schemas (including `ArtifactId` serialized form — if parsed, `try_from = "String"` keeps the wire stable).
- Files outside the Package / Artifact cluster boundary.

## Acceptance criteria

1. Every `Package.name`, `CrateInfo.name`, `PackageInfo.name` uses `PackageName`.
2. Every artifact identifier field (image IDs, release IDs, registry refs) uses `ArtifactId` in the shape S01 ratified.
3. Every function in `hyperforge-core`'s public API that accepts a package name or artifact identifier takes the newtype.
4. `grep -rn 'name: String' crates/hyperforge-types/src/package.rs` returns zero results.
5. Round-trip wire-compat test passes for both `PackageInfo` and an artifact-identifier fixture.
6. `cargo build --workspace` green in hyperforge.
7. `cargo test --workspace` green in hyperforge.
8. File-boundary check: `git diff --stat` touches only Package/Artifact cluster files plus minimal seams. No Repo / Version / Path / Credential / Ecosystem cluster edits.
9. Sibling-repo audit: consumer repos still build.
10. `hyperforge-types` and `hyperforge-core` version bumps; tags `hyperforge-types-v<version>` / `hyperforge-core-v<version>` local, not pushed.

## Completion

Implementor commits migration, round-trip fixtures, version bumps, transitional-seam inventory, confirms the full workspace + consumer audit green, tags local versions, flips status to Complete in the same commit.
