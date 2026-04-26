---
id: HF-IR-6
title: "#[child(list = \"artifact_ids\")] gate: artifact (dynamic) under RepoActivation; extract publishing methods"
status: Pending
type: implementation
blocked_by: [HF-IR-2]
unlocks: [HF-IR-8]
severity: Medium
target_repo: hyperforge
---

## Problem

Publishing artifacts (built + published binaries, tarballs, crate uploads) are addressed today via flat methods on `RepoHub` / `ReleasesHub`, keyed by `ArtifactId`. Natural addressing is `workspace.repo <r>.artifact <id>.download` or `.inspect`. The `ArtifactActivation` shell introduced in HF-IR-2 receives these operations; `RepoActivation` gets a `#[child(list = "artifact_ids")]` gate on the publishing side.

This ticket covers publishing-side artifacts under a repo — distinct from image artifacts under `ImagesHub` (HF-IR-8) and release-version artifacts under `ReleasesHub` (HF-IR-8). Gate attachment is on `RepoActivation`; the `ImageActivation` + `ReleaseActivation` gates are HF-IR-8's scope.

## Context

Per HF-IR-S01, the dynamic gate uses `list_method = "artifact_ids"` and `search_method = <per S01>`. Applies only to repos that publish — has a `PackageRegistry` entry. For repos that don't publish, `artifact_ids` yields an empty stream and `artifact(id)` returns `None`.

`ArtifactId` newtype per HF-TT may be a qualified id (`<ecosystem>:<namespace>:<name>`) or a plain string — HF-TT-S01 pinned. Either shape serializes transparently via `#[serde(transparent)]` so wire format is stable.

Methods extracted (final list per HF-IR-S01; expected):

| Source (flat on RepoHub or ReleasesHub) | Target (on ArtifactActivation) | Kept flat? |
|---|---|---|
| `get_artifact(repo, id)` | `info()` | Yes, deprecated in HF-IR-9 |
| `download_artifact(repo, id)` | `download()` | Yes, deprecated in HF-IR-9 |
| `inspect_artifact(repo, id)` | `inspect()` | Yes, deprecated in HF-IR-9 |
| `list_artifacts(repo)` | n/a — superseded by `artifact_ids` stream | Yes, deprecated in HF-IR-9 |

If HF-IR-S01 assigns any of these methods to `ReleasesHub` rather than `RepoHub` in hyperforge's current shape, extract from whichever hub owns them; gate attaches on `RepoActivation` either way.

## Required behavior

| Invocation | Behavior |
|---|---|
| `synapse hyperforge workspace <ws> repo <r> artifact` | Tree-lists all `artifact_ids` for the repo. Empty for non-publishing repos. |
| `synapse hyperforge workspace <ws> repo <r> artifact <id> download` | Returns the same bytes as the flat `download_artifact` call pre-ticket. |
| `ChildRouter::get_child(repo_activation, "<valid-artifact-id>")` | `Some(ArtifactActivation)`. |
| `ChildRouter::get_child(repo_activation, "<invalid-id>")` | `None`. |
| `plugin_schema()` on `RepoActivation` | Contains a method entry named `artifact` with `role: MethodRole::DynamicChild { list_method: Some("artifact_ids"), search_method: <per S01> }`. |
| Flat artifact methods | Unchanged wire behavior. Deprecation in HF-IR-9. |
| `ChildCapabilities::LIST` | Set on `ChildRouter` impl for `RepoActivation` (additive on top of HF-IR-5's package gate). |

## Risks

| Risk | Mitigation |
|---|---|
| Two `MethodRole::DynamicChild` entries on the same activation (`package` + `artifact`). | CHILD-3/4 supports multiple `#[child]` gates per activation; each has its own `list_method`. `ChildRouter::get_child` disambiguates by the child namespace prefix. |
| A repo with packages but zero published artifacts still needs the gate to register. | `artifact_ids` yields empty stream; `artifact(id)` returns `None`. The gate exists in `plugin_schema()` regardless. |
| Cross-repo artifact reuse (same artifact published from multiple repos). | Out of scope — each repo's artifact listing shows only that repo's artifacts. |
| `ArtifactId` qualified-id format parsing differs from plain-string format. | Use the `ArtifactId` newtype's `from_str` / `to_string` as defined in HF-TT; do not re-parse here. |

## What must NOT change

- Flat per-artifact method wire format and semantics.
- `ArtifactId` newtype.
- `PackageRegistry` enum.
- HF-IR-4's `repo` gate and HF-IR-5's `package` gate on their respective parents — this ticket adds one more gate alongside them, it does not touch their wiring.
- `ImagesHub` and `ReleasesHub` activations — HF-IR-8 owns their gates.

## Acceptance criteria

1. `cargo build --workspace` in hyperforge passes.
2. `cargo test --workspace` in hyperforge passes.
3. `cargo build` green in every sibling repo that depends on hyperforge.
4. `plugin_schema()` on `RepoActivation` contains a method entry named `artifact` with correct `MethodRole::DynamicChild`.
5. `ChildRouter::get_child(repo_activation, "<valid-artifact-id>")` returns `Some(ArtifactActivation)`; `get_child("<invalid-id>")` returns `None`.
6. `ChildCapabilities::LIST` set on `RepoActivation`'s `ChildRouter` impl.
7. For a publishing repo fixture, a test asserts the nested `artifact` tree lists the same set of ids as the flat `list_artifacts(repo)` pre-ticket.
8. For every method extracted into `ArtifactActivation`, a test asserts the nested path returns byte-identical response to the flat method.
9. Hyperforge version remains `4.2.0`.
10. File-boundary scope: this ticket modifies `hubs/repo.rs` (or `hubs/releases.rs` if HF-IR-S01 sources artifacts there) and the library file holding `ArtifactActivation`. No edits to `hubs/workspace.rs`, `hubs/build.rs`, `hubs/images.rs`, `hubs/auth.rs`, or `hubs/hyperforge.rs`.

## Completion

Commit lands `#[child(list = "artifact_ids")]` + `artifact_ids` stream + extracted methods on `ArtifactActivation`. `cargo build --workspace` + `cargo test --workspace` green. Status flipped to Complete in the same commit.
