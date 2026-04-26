---
id: HF-IR-5
title: "#[child(list = \"package_names\")] gate: package (dynamic) under RepoActivation; extract package methods"
status: Pending
type: implementation
blocked_by: [HF-IR-2]
unlocks: [HF-IR-8]
severity: Medium
target_repo: hyperforge
---

## Problem

`RepoHub` exposes per-package operations (`build`, `test`, `publish`, lookups) as flat methods keyed by `PackageName`. The natural addressing is `workspace.repo <r>.package <p>.build`, which requires a dynamic `#[child(list = "package_names")]` gate on the repo-scoped activation (now `RepoActivation`, per HF-IR-4's rewiring) plus extraction of package-scoped method bodies into `PackageActivation`.

Packages are partitioned by ecosystem via `BuildSystemKind::ecosystem()` (HF-TT-7); this ticket treats packages as a flat enumeration addressable by `PackageName` across all ecosystems — the ecosystem partitioning does not add a separate child-gate level in this ticket's scope.

## Context

Per HF-IR-S01, the dynamic gate uses `list_method = "package_names"` and `search_method = <per S01>` (expected `None`; possible `find_package` if warranted).

This ticket attaches the gate on `RepoActivation` (the new repo-scoped activation introduced in HF-IR-4). `RepoHub` as a hub type may persist (HF-IR-S01 pins whether `RepoHub` is fully replaced by `RepoActivation` or coexists during migration); this ticket attaches the gate wherever S01 pins the repo surface.

Method extraction pattern mirrors HF-IR-4: helper functions factored out of flat methods; `PackageActivation` constructed via lookup, with storage handle scoped to the parent repo.

Methods extracted in this ticket (final list per HF-IR-S01; expected):

| Source (flat, repo-scoped) | Target (on PackageActivation) | Kept flat? |
|---|---|---|
| `get_package(repo, name)` | `info()` | Yes, deprecated in HF-IR-9 |
| `build(repo, pkg)` | `build()` | Yes, deprecated in HF-IR-9 |
| `test(repo, pkg)` | `test()` | Yes, deprecated in HF-IR-9 |
| `publish(repo, pkg)` | `publish()` | Yes, deprecated in HF-IR-9 |
| `list_packages(repo)` | n/a — superseded by `package_names` stream | Yes, deprecated in HF-IR-9 |

Multi-level nesting note: `workspace.repo.package` is 3 levels of dynamic child gating (workspace may be static per HF-IR-3; repo dynamic; package dynamic). HF-IR-S01 verified synapse 3.12.0 renders this correctly, or filed a synapse follow-up. Either way, Rust-side routing works per CHILD-3/4 semantics.

## Required behavior

| Invocation | Behavior |
|---|---|
| `synapse hyperforge workspace <ws> repo <r> package` | Tree-lists all known package names in that repo via `package_names` stream. |
| `synapse hyperforge workspace <ws> repo <r> package <p> build` | Returns same result as `synapse hyperforge workspace <ws> build repo=<r> pkg=<p>` did pre-ticket. Byte-identical response payload. |
| `ChildRouter::get_child(repo_activation, "<valid-pkg>")` | `Some(PackageActivation)`. |
| `ChildRouter::get_child(repo_activation, "<invalid-pkg>")` | `None`. |
| `plugin_schema()` on `RepoActivation` | Contains a method entry named `package` with `role: MethodRole::DynamicChild { list_method: Some("package_names"), search_method: <per S01> }`. |
| Flat package methods | Unchanged wire behavior. Deprecation in HF-IR-9. |
| `ChildCapabilities::LIST` | Set on `ChildRouter` impl for `RepoActivation`. |

## Risks

| Risk | Mitigation |
|---|---|
| `package_names` stream scope: all packages in the repo, or partitioned by ecosystem? | All packages across ecosystems, sorted lexicographically by `PackageName`. Ecosystem partitioning remains accessible via `BuildSystemKind::ecosystem()` on the retrieved package data, not via a separate child gate in this ticket. |
| Some packages live under sub-paths (monorepo with nested workspaces). `PackageName` may not disambiguate. | HF-TT's `PackageName` newtype semantics apply. If ambiguity exists, HF-IR-S01 pins disambiguation (qualified `PackageName` or a separate sub-repo gate). |
| A repo with zero packages still needs the gate. | `package_names` yields an empty stream; `package(name)` returns `None` for any input. Both compile and behave correctly. |
| Generic over parent: `RepoActivation` is generic if `WorkspaceHub` is. Cascade to `PackageActivation`. | IR-21's `plugin_id_type = "..."` attr applies. Set in HF-IR-2. |

## What must NOT change

- Flat per-package method wire format and semantics.
- `PackageName` newtype.
- `BuildSystemKind` and `Ecosystem` enums.
- `RepoActivation`'s existing methods (from HF-IR-4).
- Other hubs' methods (`HyperforgeHub`, `BuildHub`, `ImagesHub`, `ReleasesHub`, `AuthHub`).

## Acceptance criteria

1. `cargo build --workspace` in hyperforge passes.
2. `cargo test --workspace` in hyperforge passes.
3. `cargo build` green in every sibling repo that depends on hyperforge.
4. `plugin_schema()` on `RepoActivation` contains a method entry named `package` with correct `MethodRole::DynamicChild`.
5. `ChildRouter::get_child(repo_activation, "<valid-pkg>")` returns `Some(PackageActivation)`; `get_child("<invalid-pkg>")` returns `None`.
6. `ChildCapabilities::LIST` set on `RepoActivation`'s `ChildRouter` impl.
7. For every method extracted into `PackageActivation`, a test asserts the nested path returns byte-identical response to the flat method.
8. Hyperforge version remains `4.2.0`.
9. File-boundary scope: this ticket modifies `hubs/repo.rs` and the shared library file holding `PackageActivation`. No edits to `hubs/workspace.rs` (beyond HF-IR-4), `hubs/build.rs`, `hubs/images.rs`, `hubs/releases.rs`, `hubs/auth.rs`, or `hubs/hyperforge.rs`.

## Completion

Commit lands `#[child(list = "package_names")]` + `package_names` stream + extracted methods on `PackageActivation`. `cargo build --workspace` + `cargo test --workspace` green. Status flipped to Complete in the same commit.
