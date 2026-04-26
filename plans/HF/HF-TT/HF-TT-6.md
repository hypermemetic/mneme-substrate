---
id: HF-TT-6
title: "Migrate Path/Credential cluster: RepoPath, WorkspaceRoot, CredentialKey"
status: Pending
type: implementation
blocked_by: [HF-TT-2]
unlocks: [HF-TT-8]
target_repo: hyperforge
severity: Medium
---

## Problem

Hyperforge passes `PathBuf` and `String` values for three distinct path/credential concepts with no type-level discrimination: paths rooted inside a repo (relative), absolute workspace roots, and credential-key strings used for secrets / auth. A function taking `(workspace: &Path, repo_path: &Path)` silently compiles with arguments swapped. Credential keys printed in a log line leak secrets. This ticket introduces newtype discrimination at every such field and parameter inside `hyperforge-types` and `hyperforge-core`.

## Context

HF-TT-2 introduced `RepoPath` (over `PathBuf`), `WorkspaceRoot` (over `PathBuf`), `CredentialKey` (over `String`, with a redacting `Display`).

Typical call sites:

- `workspace_root: PathBuf` → `WorkspaceRoot`
- `path: PathBuf` inside a repo context → `RepoPath`
- Relative-to-repo path parameters → `RepoPath`
- Secret / credential-key strings in auth modules → `CredentialKey`
- `HashMap<String, Secret>` keyed by credential key → `HashMap<CredentialKey, Secret>`

File-boundary discipline: this ticket edits `crates/hyperforge-types/src/path.rs` + `crates/hyperforge-types/src/credential.rs` plus `crates/hyperforge-core/src/workspace/` and `crates/hyperforge-core/src/auth/` (or equivalent post-HF-DC locations). Does NOT touch Repo / Package / Version / Ecosystem modules.

`CredentialKey`'s `Display` emits `"<redacted>"` (per HF-TT-2). Every log line, tracing field, and error message that previously interpolated the credential key as a raw string now emits `"<redacted>"`. This is a behavior change at the log surface; acceptance criteria verify no log-line regression breaks expected developer ergonomics — the key's discriminating info (e.g., its leading chars) may be exposed via a separate `CredentialKey::prefix(&self, n: usize) -> String` helper if audit trails need it.

## Required behavior

| Before | After |
|---|---|
| `workspace_root: PathBuf` | `workspace_root: WorkspaceRoot` |
| `pub struct WorkspaceConfig { pub root: PathBuf, ... }` | `pub struct WorkspaceConfig { pub root: WorkspaceRoot, ... }` |
| Repo-relative path fields | `RepoPath` |
| `fn resolve_in_repo(p: &Path) -> ...` | `fn resolve_in_repo(p: &RepoPath) -> ...` |
| Credential-key strings in auth types | `CredentialKey` |
| `tracing::info!("using key {}", key)` emitting raw string | Emits `"<redacted>"` via `CredentialKey::Display`. |
| `HashMap<String, Secret>` | `HashMap<CredentialKey, Secret>` |

Wire format preservation: `RepoPath` and `WorkspaceRoot` are `#[serde(transparent)]` over `PathBuf` — identical to how `PathBuf` already serializes (as a string). `CredentialKey` is `#[serde(transparent)]` over `String`. Round-trip test `crates/hyperforge-core/tests/path_credential_wire_compat.rs` loads fixtures (a `WorkspaceConfig`, a struct with a `RepoPath`, a struct with a `CredentialKey`) and confirms byte-identical re-serialization.

Additional test in the same file: `CredentialKey::Display` emits the redaction token even when the key is `format!`-interpolated. Inversely, `CredentialKey::Serialize` emits the inner string (wire format must stay compatible).

## Risks

| Risk | Mitigation |
|---|---|
| A log line that was useful for debugging now emits `"<redacted>"` and loses operator utility. | `CredentialKey::prefix(n)` helper is introduced in HF-TT-2 or this ticket (whichever lands first); operator logs switch to prefix. |
| `RepoPath` absolute-vs-relative semantics are ambiguous at construction. | `RepoPath::new` accepts any `PathBuf` but documents that callers are responsible for ensuring it is repo-relative. No construction-time validation in this ticket. |
| On-disk paths get serialized differently on Windows vs Unix. | Out of scope — `PathBuf` serialization behavior is unchanged from pre-migration. |

## What must NOT change

- Wire format of any struct containing `RepoPath`, `WorkspaceRoot`, or `CredentialKey` — byte-identical round-trip.
- Public method names on any activation.
- CLI behavior or output (the redacted Display does change log output — pin this explicitly as a planned, audited change, not a regression).
- Files outside the Path/Credential cluster boundary.

## Acceptance criteria

1. Every workspace-root field uses `WorkspaceRoot`.
2. Every repo-relative path field uses `RepoPath`.
3. Every credential-key field uses `CredentialKey`.
4. `CredentialKey::Display` emits the redaction token (test).
5. `CredentialKey::Serialize` emits the inner string (test — wire format preserved).
6. `CredentialKey::prefix(n)` helper exists and returns the first `n` chars for audit-trail use cases.
7. Round-trip wire-compat test passes for all three fixture types.
8. `cargo build --workspace` green in hyperforge.
9. `cargo test --workspace` green in hyperforge.
10. File-boundary check: edits confined to Path/Credential cluster files plus minimal seams.
11. Sibling-repo audit: consumer repos still build.
12. `hyperforge-types` and `hyperforge-core` version bumps; tags local, not pushed.

## Completion

Implementor commits migration, fixtures, version bumps, seam inventory, confirms full workspace + consumers green, tags local, flips status to Complete in the same commit. Commit message lists any log-line output changes so operators can anticipate the new redaction.
