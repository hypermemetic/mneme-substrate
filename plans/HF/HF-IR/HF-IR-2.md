---
id: HF-IR-2
title: "Introduce empty child-activation structs for hyperforge's six dynamic children"
status: Pending
type: implementation
blocked_by: [HF-IR-S01]
unlocks: [HF-IR-3, HF-IR-4, HF-IR-5, HF-IR-6, HF-IR-7]
severity: Medium
target_repo: hyperforge
---

## Problem

HF-IR's child-gate adoption requires per-child activation types (`RepoActivation`, `PackageActivation`, `ArtifactActivation`, `ReleaseActivation`, `ImageActivation`, `CredentialActivation`). Today these don't exist — per-id operations are flat methods on the hub that holds the concern. Before HF-IR-3..8 can attach `#[child(list = ...)]` gates and extract methods into the children, the child types must exist as empty shells wired into the workspace, with a handle back to the parent's storage and the child id scoped as a filter.

This ticket introduces those six shells in a single commit. No methods are moved yet — HF-IR-4..8 own the extraction per-hub. By landing all six shells together, the per-hub extraction tickets can run in parallel without stepping on each other's type definitions.

## Context

Ratified in HF-IR-S01: the six child activations named above, their parents (per the HF-IR-1 target tree), and their `list_method` names. Each child holds:

- A `Handle<ParentStorage>` (or `Arc<ParentStorage>` — matches the parent hub's existing storage access pattern).
- A strongly-typed id field (`RepoName`, `PackageName`, `ArtifactId`, etc. — per HF-TT's newtype inventory).

Precedent: IR-18 (ClaudeCode/SessionActivation) and IR-19 (Cone/ConeActivation) both land their child activations as empty-ish structs with one flagship method, then extend. HF-IR-2 is broader (six children at once) but structurally identical.

The `#[plexus_macros::activation(namespace = "...")]` macro attaches; the struct compiles against today's `hub-core` and `hub-macro` surface (no new macro work).

If any child activation ends up generic over its parent's storage type (`P: HubContext`), the `HandleEnum` derive needs the `plugin_id_type = "..."` attr per IR-21. HF-IR-S01's mapping records which children are generic and which are concrete; this ticket honors those decisions.

## Required behavior

| Child activation | Parent hub | id field type | Storage handle type | Namespace |
|---|---|---|---|---|
| `RepoActivation` | `WorkspaceHub` | `RepoName` | handle to `WorkspaceHub`'s storage | `repo` |
| `PackageActivation` | `RepoActivation` (post-HF-IR-5) | `PackageName` | handle via `RepoActivation` → `WorkspaceHub` storage | `package` |
| `ArtifactActivation` | `RepoActivation` (post-HF-IR-6) | `ArtifactId` | same | `artifact` |
| `ReleaseActivation` | `ReleasesHub` | `Version` | handle to `ReleasesHub`'s storage | `release` |
| `ImageActivation` | `ImagesHub` | `ArtifactId` (image variant) | handle to `ImagesHub`'s storage | `image` |
| `CredentialActivation` | `AuthHub` | `CredentialKey` | handle to `AuthHub`'s storage | `credential` |

Each child type:

- Is declared with `#[plexus_macros::activation(namespace = "<name>")]`.
- Has a `pub fn new(id: <IdType>, storage: <HandleType>) -> Self` constructor.
- Has the id field accessible via a `pub fn id(&self) -> &<IdType>` method (non-macro, regular Rust).
- Carries zero `#[plexus_macros::method]`-annotated methods in this ticket — the impl block is empty apart from the constructor and id accessor.
- Compiles cleanly as part of the hyperforge workspace.

Wire format: each child activation type registers a `PluginSchema` via the activation macro; that schema appears in `WorkspaceHub`'s (or the relevant parent's) child list only after the parent's `#[child]` gate is added in HF-IR-3..8. In this ticket, the types exist but are not yet reachable over the wire.

## Risks

| Risk | Mitigation |
|---|---|
| Storage-handle design differs per parent hub (some hubs hold `Arc<State>`, some hold more granular handles). | Match each parent hub's existing access pattern; don't normalize across hubs in this ticket. HF-DC already split the library surface — each hub's storage access is settled. |
| A child activation needs a generic parameter (e.g., `PackageActivation<P: HubContext>`) because its parent is generic. | Use IR-21's `plugin_id_type = "..."` attr on any `HandleEnum` derive inside the generic child. HF-IR-S01 flagged which children are generic. |
| `ArtifactId` is used by both `ArtifactActivation` (repo-scoped publishing artifacts) and `ImageActivation` (image registry artifacts); semantics may diverge. | HF-TT's `ArtifactId` newtype covers both — qualified id format per HF-TT-S01. If divergence emerges, split into `PublishedArtifactId` and `ImageArtifactId` inside this ticket; flag in HF-IR-S01's mapping. |
| Empty activations might not register cleanly in the `PluginSchema` tree (older macro versions asserted ≥1 method). | Verify on the `plexus_macros` version this workspace pins; if the macro asserts a non-empty method list, add a placeholder `ping` method returning `()` that HF-IR-4..8 can remove when real methods land. |

## What must NOT change

- Every existing hyperforge method signature, wire format, and semantics. This ticket adds types; it does not touch the existing hubs' impl blocks.
- `HyperforgeState` and all hub storage types — unchanged.
- Existing activation namespaces (`hyperforge`, `auth`, `secrets`) — unchanged.
- Hyperforge's CLI argument grammar — unchanged.

## Acceptance criteria

1. `cargo build --workspace` in hyperforge passes.
2. `cargo test --workspace` in hyperforge passes — no existing tests regress. No new tests required (empty types).
3. `cargo build` in every sibling repo that depends on hyperforge passes (integration gate per rule 12).
4. Six new public types exist in hyperforge's library crate with the names and field shapes in the Required behavior table: `RepoActivation`, `PackageActivation`, `ArtifactActivation`, `ReleaseActivation`, `ImageActivation`, `CredentialActivation`.
5. Each type has a `new(...)` constructor and an `id()` accessor. Calling `PluginSchema::for_type::<RepoActivation>()` (or the equivalent macro-provided schema introspection) yields a `PluginSchema` with namespace `"repo"` and zero method entries; analogous for the other five.
6. Hyperforge's version is bumped from `4.1.x` → `4.2.0` in the same commit (this is the first HF-IR ticket landing; subsequent HF-IR tickets contribute to 4.2.0 per `feedback_version_bumps_as_you_go.md`).
7. Any child activation flagged generic in HF-IR-S01 uses `plugin_id_type = "..."` on its `HandleEnum` derive per IR-21. Non-generic children derive `HandleEnum` with the default pattern.

## Completion

PR (or direct commit, per hyperforge's current flow) lands the six types + version bump. `cargo build --workspace` + `cargo test --workspace` green in hyperforge and every sibling workspace consumer. Status flipped to Complete in the same commit.
