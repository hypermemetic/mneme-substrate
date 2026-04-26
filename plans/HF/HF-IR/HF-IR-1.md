---
id: HF-IR-1
title: "HF-IR sub-epic — hyperforge adopts CHILD + IR primitives"
status: Epic
type: epic
blocked_by: [HF-TT-1]
unlocks: [HF-CTX-1]
target_repo: hyperforge
---

## Goal

End state: hyperforge's 7-activation surface uses the child-gate, MethodRole::DynamicChild, and DeprecationInfo primitives shipped by plexus-substrate's CHILD and IR epics. The currently-static child registration (hardcoded in `HyperforgeState`) is replaced by `#[plexus_macros::child(list = "...")]` dynamic children where it makes sense — repos under a workspace, packages under a repo, artifacts under a release — giving synapse users natural nested addressing:

```
synapse hyperforge workspace {ws}.repo {name}.package {pkg}.build
```

Deprecated or about-to-be-obsoleted methods carry `DeprecationInfo` so consumers see the migration guidance in synapse's tree rendering and invocation warnings (IR-6, IR-14, IR-15 primitives).

Hyperforge becomes one of the first post-IR activation citizens — per the user's directive, hyperforge is an early beneficiary of the CHILD + IR investment.

## Context

Current state (per HF-0 survey):

- 7 activations: `HyperforgeHub` (namespace `hyperforge`, hub=true, 40+ methods), `WorkspaceHub`, `RepoHub`, `BuildHub`, `ImagesHub`, `ReleasesHub` (child hubs), `AuthHub` (namespace `auth`/`secrets`, 7 methods).
- **No `#[child]` gates.** All child relationships are hardcoded in `HyperforgeState` (the state struct the activation holds). Synapse sees flat namespaces, not nested child trees.
- **No `MethodRole::DynamicChild`.** Every listable-by-id concern (repos, packages, artifacts) is exposed as a flat method pair: `list_repos()` + `get_repo(name)`, rather than the dynamic child pattern shipped in IR-12.
- **No `DeprecationInfo`.** Existing method pairs that would be obsoleted by child-gate adoption cannot yet surface their deprecation to consumers.

After HF-IR, hyperforge's activation tree in synapse looks like:

```
hyperforge
├── method: status
├── method: refresh
├── child: workspace (static, one per known workspace)
│   └── child: repo (dynamic, list = "repo_names")
│       ├── method: status
│       ├── method: pull
│       ├── child: package (dynamic, list = "package_names")
│       │   ├── method: build
│       │   ├── method: test
│       │   └── method: publish
│       └── child: artifact (dynamic, list = "artifact_ids")
│           ├── method: download
│           └── method: inspect
└── child: auth (static, AuthHub)
```

## Proposed child-gate mapping (ratified in HF-IR-S01)

| Parent | Child gate | Kind | List method | Notes |
|---|---|---|---|---|
| `HyperforgeHub` | `workspace` | Static (one instance per known workspace) | — | `#[child]` with no args. |
| `WorkspaceHub` | `repo` | Dynamic | `repo_names` | `#[child(list = "repo_names")] fn repo(&self, name: &RepoName) -> RepoActivation` |
| `RepoHub` (now per-repo) | `package` | Dynamic | `package_names` | Per ecosystem; `BuildSystemKind::ecosystem()` partitions. |
| `RepoHub` | `artifact` | Dynamic | `artifact_ids` | For repos that publish (has `PackageRegistry` entry). |
| `ReleasesHub` | `release` | Dynamic | `release_versions` | Per-release addressing. |
| `ImagesHub` | `image` | Dynamic | `image_ids` | Per-image addressing. |
| `AuthHub` | `credential` | Dynamic | `credential_keys` | Name-by-key; `search = "find_credential"` if the spike decides. |

Each dynamic child gate obsoletes the corresponding `list_X` / `get_X` method pair. Those pairs get `#[deprecated(since = "4.2.0", note = "...")]` with `DeprecationInfo` pointing at the child gate.

## Dependency DAG

```
           HF-IR-S01 (child gate mapping spike)
                  │
                  ▼
           HF-IR-2 (introduce child-activation structs: RepoActivation,
                    PackageActivation, etc. — empty method lists)
                  │
        ┌─────────┼─────────┬─────────┬─────────┐
        ▼         ▼         ▼         ▼         ▼
     HF-IR-3   HF-IR-4   HF-IR-5   HF-IR-6   HF-IR-7
    (workspace (repo gate (package  (artifact (auth/
     gate on    on Work-   gate on   gate on   credential)
     Hyperforge) space)    Repo)     Repo)     AuthHub)
        │         │         │         │         │
        └─────────┴─────┬───┴─────────┴─────────┘
                        ▼
              HF-IR-8 (images + releases gates)
                        │
                        ▼
              HF-IR-9 (deprecate flat list/get pairs,
                       wire DeprecationInfo)
                        │
                        ▼
              HF-IR-10 (synapse integration verification:
                        tree rendering, invocation warnings)
```

## Phase breakdown

| Phase | Tickets | Notes |
|---|---|---|
| 0. Spike | HF-IR-S01 | Ratify the child-gate mapping + `list_method` / `search_method` names. Binary-pass. |
| 1. Foundation | HF-IR-2 | Introduce empty child-activation structs. No methods yet — they come from the existing methods being moved into the children in phase 2/3. |
| 2. Parallel gate introductions | HF-IR-3..7 | Each hub gains its child gates. File-boundary disjoint. |
| 3. Artifact gates | HF-IR-8 | Images + releases. |
| 4. Deprecation wiring | HF-IR-9 | Mark flat list/get pairs deprecated with migration pointers. |
| 5. Integration verification | HF-IR-10 | Verify synapse renders the new tree correctly and emits invocation warnings for deprecated methods. |

## Cross-epic contracts pinned

- **HandleEnum plugin_id_type:** if any child activation uses `HandleEnum` derive and is generic, use the `plugin_id_type = "..."` attr introduced in IR-21 of the plexus-substrate IR epic.
- **`MethodRole::DynamicChild { list_method, search_method }`:** always provide `list_method`; `search_method` is optional. HF-IR-S01 pins which child gates also provide search.
- **`DeprecationInfo`:** every flat `list_X`/`get_X` pair deprecated in HF-IR-9 gets `since = "4.2.0"` (the version HF-IR lands) and `removed_in = "5.0.0"` (the next major). Removal is not in HF-IR — it's a later epic.
- **Version bump:** hyperforge 4.1.x → 4.2.0 on the first HF-IR ticket that lands; subsequent tickets contribute to 4.2.0 per `feedback_version_bumps_as_you_go.md`.

## What must NOT change

- Hyperforge's wire format (JSON over the plexus transport) for existing methods. Flat methods still work at the wire layer during the deprecation window — they just emit warnings.
- The 74 existing methods' semantics. HF-IR wraps them into child activations but does not change what they do.
- Activation namespaces at the root level (`hyperforge`, `auth`/`secrets` — preserve).

## Risks

| Risk | Mitigation |
|---|---|
| A method that looks like a flat `list_X` turns out to have semantics incompatible with a dynamic child gate (e.g., returns aggregated state, not a list of child-addressable entities). | Spike HF-IR-S01 surfaces this. Such methods stay flat, don't get deprecated. |
| `list_method` returning a type other than `impl Stream<Item = String>` (e.g., structured enumerations). | Per IR-12 / IR-18 / IR-19 convention, the list method returns a stream of the newtype's inner string representation. Migrate return types as part of the child-gate ticket. |
| Cross-child addressing confuses synapse's tree renderer (e.g., `workspace.repo {name}.package {pkg}` is 3 levels deep with dynamic nesting). | Substrate's IR-18/19 already ship `#[child(list = ...)]` at one level; nested dynamic gates are the next frontier. HF-IR-S01 must verify synapse 3.12.0's tree renderer handles multi-level nesting — if not, file a synapse follow-up ticket. |
| Every child activation needs its own storage / state split. | Scope: each child activation owns a handle back to the parent's storage, with the child's id as a filter. Don't split storage — just scope access. |

## Out of scope

- Removing any flat method — deprecation only. Removal is a future epic.
- Changing existing method bodies beyond the extraction into child activations.
- Building the fact log / context store (HF-CTX).
- Cross-workspace addressing (single-workspace scope per HF-1).
- Authentication / authorization reshaping (AuthHub is in scope for its own child gates, but the auth model isn't changing here).

## Completion

Sub-epic is Complete when:

- HF-IR-S01 through HF-IR-10 are all Complete.
- `synapse hyperforge` tree rendering shows the nested structure per the "target tree" sketch above.
- `synapse hyperforge list_repos` (deprecated) emits the stderr warning per IR-15; `synapse hyperforge workspace {ws}.repo_names` (the child-gate list) produces the same content without warning.
- `cargo build --workspace` + `cargo test --workspace` green.
- `hyperforge-hubs` (or the relevant member crate) version reflects the surface change (minor bump); tag local.
