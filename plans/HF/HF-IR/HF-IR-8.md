---
id: HF-IR-8
title: "#[child(list)] gates: release (ReleasesHub) + image (ImagesHub); extract release/image methods"
status: Pending
type: implementation
blocked_by: [HF-IR-4, HF-IR-5, HF-IR-6, HF-IR-7]
unlocks: [HF-IR-9]
severity: Medium
target_repo: hyperforge
---

## Problem

`ReleasesHub` and `ImagesHub` are the two remaining hubs whose flat per-id methods should become dynamic child gates. Both host activation surfaces that keyed entities live under:

- `ReleasesHub` — per-release-version operations (`info`, `artifacts`, `publish-notes`). `ReleaseActivation` shell awaits from HF-IR-2.
- `ImagesHub` — per-image operations (`inspect`, `tag`, `pull`, `push`). `ImageActivation` shell awaits from HF-IR-2.

Both gates are on distinct hub files and could have been split into two tickets. They land together here because:

1. They share the HF-IR-4..7 prerequisite set (release artifact listings may depend on repo-side artifact gating from HF-IR-6).
2. They're small — each extracts a few methods.
3. Landing them together keeps the HF-IR-9 deprecation ticket's pre-deprecation surface stable.

If implementation finds the two hubs need independent timelines, split into HF-IR-8a and HF-IR-8b without further ticketing ceremony.

## Context

Per HF-IR-S01:

- `ReleasesHub` gate: `list_method = "release_versions"`, `search_method = <per S01>`. Id type: `Version` (HF-TT newtype).
- `ImagesHub` gate: `list_method = "image_ids"`, `search_method = <per S01>`. Id type: `ArtifactId` (or an image-specific variant if HF-TT-S01 split it).

Method extraction (final list per HF-IR-S01; expected):

**ReleasesHub → ReleaseActivation:**

| Source | Target | Kept flat? |
|---|---|---|
| `get_release(version)` | `info()` | Yes, deprecated in HF-IR-9 |
| `release_artifacts(version)` | `artifacts()` | Yes, deprecated in HF-IR-9 |
| `publish_notes(version)` | `publish_notes()` | Yes, deprecated in HF-IR-9 |
| `list_releases()` | n/a — superseded by `release_versions` | Yes, deprecated in HF-IR-9 |

**ImagesHub → ImageActivation:**

| Source | Target | Kept flat? |
|---|---|---|
| `get_image(id)` | `inspect()` | Yes, deprecated in HF-IR-9 |
| `tag_image(id, tag)` | `tag(tag)` | Yes, deprecated in HF-IR-9 |
| `pull_image(id)` | `pull()` | Yes, deprecated in HF-IR-9 |
| `push_image(id)` | `push()` | Yes, deprecated in HF-IR-9 |
| `list_images()` | n/a — superseded by `image_ids` | Yes, deprecated in HF-IR-9 |

## Required behavior

| Invocation | Behavior |
|---|---|
| `synapse hyperforge releases release` | Tree-lists `release_versions`. |
| `synapse hyperforge releases release <v> info` | Byte-identical to pre-ticket `get_release(version=<v>)`. |
| `synapse hyperforge images image` | Tree-lists `image_ids`. |
| `synapse hyperforge images image <id> pull` | Byte-identical to pre-ticket `pull_image(id=<id>)`. |
| `ChildRouter::get_child(releases_hub, "<valid-version>")` | `Some(ReleaseActivation)`. |
| `ChildRouter::get_child(releases_hub, "<invalid-version>")` | `None`. |
| `ChildRouter::get_child(images_hub, "<valid-image>")` | `Some(ImageActivation)`. |
| `ChildRouter::get_child(images_hub, "<invalid-image>")` | `None`. |
| `plugin_schema()` on `ReleasesHub` | Contains method entry named `release` with `role: MethodRole::DynamicChild { list_method: Some("release_versions"), search_method: <per S01> }`. |
| `plugin_schema()` on `ImagesHub` | Contains method entry named `image` with `role: MethodRole::DynamicChild { list_method: Some("image_ids"), search_method: <per S01> }`. |
| Flat release / image methods | Unchanged wire behavior. Deprecation in HF-IR-9. |
| `ChildCapabilities::LIST` | Set on both `ChildRouter` impls. |

## Risks

| Risk | Mitigation |
|---|---|
| `Version` strings are ecosystem-qualified in some contexts (e.g., `crate-name@1.2.3`) — gate input format must be pinned. | Use HF-TT's `Version` newtype; its `from_str` defines the accepted format. Gate accepts whatever `Version::from_str` accepts. |
| `ImagesHub` image listings include ephemeral builds — noisy. | Keep existing listing semantics (the flat `list_images` contract) — do not filter in this ticket. |
| `ReleasesHub` and `ImagesHub` are extended by plugins in downstream consumers. | Plugin extensions continue to work unchanged — the `#[child]` gate does not remove plugin registration surface. |
| Two ticket's worth of work in one file-disjoint commit risks merge friction. | Land in two commits if needed (one per hub). Single PR is optional. |

## What must NOT change

- Flat release / image method wire format and semantics.
- `Version`, `ArtifactId` newtypes.
- Release / image storage formats.
- HF-IR-3..7 gates.
- Other hubs.

## Acceptance criteria

1. `cargo build --workspace` in hyperforge passes.
2. `cargo test --workspace` in hyperforge passes.
3. `cargo build` green in every sibling repo that depends on hyperforge.
4. `plugin_schema()` on `ReleasesHub` and `ImagesHub` each contain the correct `release` / `image` entries with `MethodRole::DynamicChild`.
5. `ChildRouter::get_child` tests for both hubs return `Some(...)` on valid ids and `None` on invalid.
6. `ChildCapabilities::LIST` set on both `ChildRouter` impls.
7. For every method extracted into `ReleaseActivation` and `ImageActivation`, a test asserts the nested path returns byte-identical response to the flat method.
8. Hyperforge version remains `4.2.0`.
9. File-boundary scope: this ticket modifies `hubs/releases.rs`, `hubs/images.rs`, and the library files holding `ReleaseActivation` + `ImageActivation`. No edits to `hubs/workspace.rs`, `hubs/repo.rs`, `hubs/build.rs`, `hubs/auth.rs`, or `hubs/hyperforge.rs`.

## Completion

Commit(s) land both gates + streams + extracted methods. `cargo build --workspace` + `cargo test --workspace` green. Status flipped to Complete in the same commit that completes the second gate.
