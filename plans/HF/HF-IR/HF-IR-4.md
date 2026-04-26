---
id: HF-IR-4
title: "#[child(list = \"repo_names\")] gate: repo (dynamic) under WorkspaceHub; extract repo methods into RepoActivation"
status: Pending
type: implementation
blocked_by: [HF-IR-2]
unlocks: [HF-IR-5, HF-IR-6, HF-IR-8]
severity: Medium
target_repo: hyperforge
---

## Problem

`WorkspaceHub` exposes per-repo operations as flat methods keyed by `RepoName`: `get_repo(name)`, repo-scoped `status(name)`, `pull(name)`, etc. Synapse addressing for a specific repo is `workspace.status(name=plexus-substrate)` rather than the natural nested `workspace.repo plexus-substrate.status`. The `RepoActivation` shell introduced in HF-IR-2 is ready to receive these methods; the missing piece is the `#[child(list = "repo_names")]` gate on `WorkspaceHub` plus the extraction of repo-scoped method bodies into `RepoActivation`.

## Context

Per HF-IR-S01, the dynamic gate uses `list_method = "repo_names"` and `search_method = <value per S01>` (expected `None`; S01 may pin a search method like `find_repo` if warranted). The list method returns `impl Stream<Item = String> + Send + '_` yielding each `RepoName`'s inner string representation — convention established by IR-12 / IR-18 / IR-19.

Method extraction pattern (from IR-18's risk table):

- Factor repo-scoped body logic into helpers on `WorkspaceHub`'s storage (or a helper module) callable from both the flat method (until HF-IR-9 deprecates it) and the new `RepoActivation` method.
- `RepoActivation` constructs via `WorkspaceHub::repo(&self, name: &str) -> Option<RepoActivation>`: looks up the repo in storage, returns `Some(RepoActivation::new(repo_name, storage_handle))` if present, else `None`.

Methods extracted in this ticket (final list pinned in HF-IR-S01; expected):

| Source (flat on WorkspaceHub) | Target (method on RepoActivation) | Kept on WorkspaceHub? |
|---|---|---|
| `get_repo(name)` | `info()` (or equivalent) | Yes, deprecated in HF-IR-9 |
| `status(repo)` | `status()` | Yes, deprecated in HF-IR-9 |
| `pull(repo)` | `pull()` | Yes, deprecated in HF-IR-9 |
| `list_repos()` | n/a — superseded by `repo_names` stream | Yes, deprecated in HF-IR-9 |

Exact set per HF-IR-S01. Any repo-scoped method identified by S01 as semantically incompatible with a child gate stays flat and is not moved.

Package-scoped methods currently on `WorkspaceHub` or `RepoHub` are explicitly out of scope for this ticket — HF-IR-5 handles packages.

## Required behavior

| Invocation | Behavior |
|---|---|
| `synapse hyperforge workspace <ws> repo` | Tree-lists all known repo names via `repo_names` stream. |
| `synapse hyperforge workspace <ws> repo <name> status` | Returns same status as `synapse hyperforge workspace <ws> status repo=<name>` did pre-ticket. Byte-identical response payload. |
| `ChildRouter::get_child(workspace_hub, "<valid-repo>")` | `Some(RepoActivation)`. |
| `ChildRouter::get_child(workspace_hub, "<invalid-repo>")` | `None`. |
| `plugin_schema()` on `WorkspaceHub` | Contains a method entry named `repo` with `role: MethodRole::DynamicChild { list_method: Some("repo_names"), search_method: <value per S01> }`. |
| Flat methods (`get_repo`, `status`, `pull`, etc.) still work | Unchanged wire behavior. No deprecation notice yet (HF-IR-9). |
| `ChildCapabilities::LIST` | Set on the `ChildRouter` impl for `WorkspaceHub`. |

## Risks

| Risk | Mitigation |
|---|---|
| Extracting repo method bodies duplicates logic between the flat method and `RepoActivation`. | Factor shared logic into helpers called from both. Both entry points call the same helper; neither inlines business logic. |
| `WorkspaceHub`'s storage is shared with package/artifact activations (HF-IR-5/6). Extracting repo methods must not break storage access for those. | `RepoActivation` takes a handle to the same storage the flat method used. No storage restructuring in this ticket. |
| `repo_names` stream contract: paging, ordering, stability. | Return names in lexicographic order (matches existing `list_repos` convention — verify in HF-IR-S01). No paging requirement at this scale. |
| Generic over parent: `WorkspaceHub<P: HubContext>`. `RepoActivation` needs to compile against that. | Per IR-21, use `plugin_id_type = "..."` on `HandleEnum` derive if `RepoActivation` becomes generic. HF-IR-2 already made the generic decision. |

## What must NOT change

- The wire format and semantics of every flat repo-scoped method on `WorkspaceHub` — those stay exactly as-is until HF-IR-9.
- `RepoName` newtype semantics (HF-TT).
- `WorkspaceHub`'s storage type / access pattern.
- Other hubs (`HyperforgeHub`, `RepoHub`, `BuildHub`, `ImagesHub`, `ReleasesHub`, `AuthHub`).

## Acceptance criteria

1. `cargo build --workspace` in hyperforge passes.
2. `cargo test --workspace` in hyperforge passes.
3. `cargo build` green in every sibling repo that depends on hyperforge (integration gate).
4. A test asserts that `plugin_schema()` on `WorkspaceHub` contains a method entry named `repo` with `role: MethodRole::DynamicChild { list_method: Some("repo_names"), search_method: <per S01> }`.
5. A test asserts that `ChildRouter::get_child(workspace_hub, "<valid-repo>")` returns `Some(RepoActivation)` and `get_child("<invalid-repo>")` returns `None`.
6. A test asserts that `ChildCapabilities::LIST` is set on the `ChildRouter` impl for `WorkspaceHub`.
7. For every method extracted into `RepoActivation`, a test demonstrates that invoking it via the nested path (`workspace.repo.<name>.<method>`) returns the same result as invoking the flat method (`workspace.<flat_method>(name=<name>)`) — byte-identical response payload.
8. Hyperforge version remains `4.2.0`.
9. File-boundary scope: this ticket modifies `hubs/workspace.rs` and the shared library file(s) holding `RepoActivation`. No edits to `hubs/repo.rs`, `hubs/build.rs`, `hubs/images.rs`, `hubs/releases.rs`, `hubs/auth.rs`, or `hubs/hyperforge.rs` beyond what HF-IR-3 already landed.

## Completion

Commit lands `#[child(list = "repo_names")]` + `repo_names` stream + extracted methods on `RepoActivation`. `cargo build --workspace` + `cargo test --workspace` green. Status flipped to Complete in the same commit.
