---
id: HF-TT-3
title: "Migrate Repo cluster: RepoName, OrgName, WorkspaceName"
status: Pending
type: implementation
blocked_by: [HF-TT-2]
unlocks: [HF-TT-8]
target_repo: hyperforge
severity: Medium
---

## Problem

Every field in hyperforge that represents a repository name, org name, or workspace name is a raw `String`. Two `String` parameters can be silently swapped — passing an org name where a repo name is expected is a compile-pass today. The Repo cluster migration replaces every such field and function parameter inside `hyperforge-types` (struct definitions) and `hyperforge-core` (business logic that owns the Repo / Workspace types) with the corresponding newtype. Hubs and bins remain pinned to the old types until HF-TT-8 — this ticket's scope stops at `hyperforge-core`'s public API surface.

## Context

HF-TT-2 introduced `RepoName`, `OrgName`, `WorkspaceName` in `hyperforge-types`. Typical call sites to migrate, per the HF-0 survey:

- `Repo.name: String` → `RepoName`
- `RepoRecord.name: String` → `RepoName`
- `RepoRecord.org: String` → `OrgName`
- Function parameters in `hyperforge-core` that take a repo name or org name as `&str` or `String`
- Any `HashMap<String, Repo>` keyed by repo name → `HashMap<RepoName, Repo>`
- Workspace identifiers inside `WorkspaceHub` wire → `WorkspaceName`

File-boundary discipline: this ticket edits `crates/hyperforge-types/src/repo.rs` (or wherever `Repo` / `RepoRecord` live post-HF-DC) and `crates/hyperforge-core/src/` files that touch Repo / Workspace types. It does NOT touch `crates/hyperforge-core/src/package/`, `crates/hyperforge-core/src/build_system/`, path/credential modules, or any file that HF-TT-4/5/6/7 own. If a single file contains both repo-cluster fields and (for example) package-cluster fields, the package-cluster edits wait for HF-TT-4.

## Required behavior

| Before | After |
|---|---|
| `pub struct Repo { pub name: String, ... }` | `pub struct Repo { pub name: RepoName, ... }` |
| `pub struct RepoRecord { pub name: String, pub org: String, ... }` | `pub struct RepoRecord { pub name: RepoName, pub org: OrgName, ... }` |
| `fn get_repo(name: &str) -> ...` | `fn get_repo(name: &RepoName) -> ...` |
| `fn list_repos_for_org(org: &str) -> ...` | `fn list_repos_for_org(org: &OrgName) -> ...` |
| `HashMap<String, Repo>` (keyed by name) | `HashMap<RepoName, Repo>` |
| Workspace-name parameters | `WorkspaceName` |

Wire format preservation: because `RepoName`, `OrgName`, `WorkspaceName` are `#[serde(transparent)]` over `String`, the JSON shape of every serialized struct is byte-identical pre and post migration. A new test `crates/hyperforge-core/tests/repo_wire_compat.rs` (or augmentation of an existing test) loads a realistic fixture JSON — a `RepoRecord` dump pulled from a real cache — and confirms `serde_json::from_str::<RepoRecord>(fixture) -> serialize -> byte-identical to fixture`. Fixture is checked in at `crates/hyperforge-core/tests/fixtures/repo_record.json`.

Hubs and bins are NOT edited in this ticket. They continue to compile against the new `hyperforge-core` public API only because their Repo / Workspace interactions go through methods whose signatures now take newtypes — the hubs pass `RepoName::new(s)` at their call boundary, a transitional cast that HF-TT-8 removes. If any hub or bin site fails to compile, the fix is a minimal `.into()` or `RepoName::new(...)` at the seam, not a deep rewrite.

## Risks

| Risk | Mitigation |
|---|---|
| A `String` field is semantically overloaded (repo-name sometimes, org-name other times). | HF-TT-S01's overloaded-fields list preempts this. Each overloaded field is split in this ticket with rationale in the commit message. |
| A `HashMap<String, Repo>` has a different key meaning (e.g., keyed by org, not by repo name). | Each map migration site has a comment pinning what the key represents. |
| Transitional `RepoName::new(s)` at hub seams leaks into permanent code. | HF-TT-8 owns removal; this ticket's commit message lists every transitional seam so HF-TT-8 can sweep them. |

## What must NOT change

- Wire format of `RepoRecord`, `Repo`, any serialized type — byte-identical round-trip test is the gate.
- Public method names on any activation.
- CLI behavior or output.
- Database / on-disk cache schemas.
- Files outside `crates/hyperforge-types/src/repo.rs` (or equivalent) and `crates/hyperforge-core/src/` Repo-cluster modules, except the minimal hub/bin seams needed to keep the workspace building.

## Acceptance criteria

1. Every `Repo.name`, `RepoRecord.name` uses `RepoName`. Every `RepoRecord.org` uses `OrgName`. Every workspace-name field uses `WorkspaceName`.
2. Every function in `hyperforge-core`'s public API that accepts a repo / org / workspace name takes the newtype (`&RepoName`, `&OrgName`, `&WorkspaceName`), not `&str` or `String`.
3. `grep -rn 'name: String' crates/hyperforge-types/src/repo.rs` returns zero results (no raw-String repo-name field survives).
4. Round-trip wire-compat test passes: realistic `RepoRecord` fixture loads and re-serializes byte-identically.
5. `cargo build --workspace` green in hyperforge.
6. `cargo test --workspace` green in hyperforge.
7. File-boundary check: `git diff --stat` touches only `crates/hyperforge-types/src/repo.rs` (or the S01-pinned equivalent), `crates/hyperforge-core/src/` Repo-cluster files, minimal hub/bin seam fixes, and the added fixture + test file. No files owned by HF-TT-4/5/6/7.
8. Sibling-repo audit: every workspace repo that depends on hyperforge-types or hyperforge-core (substrate, plexus-core, etc.) either still builds at its pinned version or is flagged in the commit message for follow-up; the gate is `cargo build --workspace` in each consumer repo.
9. `hyperforge-types` and `hyperforge-core` minor-version bumped; tags `hyperforge-types-v<version>` and `hyperforge-core-v<version>` created locally, not pushed.

## Completion

Implementor commits the migration, round-trip fixture, version bumps, and transitional-seam inventory (in commit message), confirms `cargo build --workspace && cargo test --workspace` green in hyperforge plus every consumer repo listed in the sibling-repo audit, tags local versions, flips status to Complete in the same commit.
