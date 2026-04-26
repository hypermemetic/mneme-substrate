---
id: HF-TT-1
title: "HF-TT sub-epic — hyperforge type tightening (newtypes for domain IDs)"
status: Epic
type: epic
blocked_by: [HF-DC-1]
unlocks: [HF-IR-1]
target_repo: hyperforge
---

## Goal

End state: every public API boundary in hyperforge that currently accepts or returns a raw `String` for a domain identifier uses a typed newtype instead. Mis-use between id kinds (e.g., passing a `PackageName` where a `RepoName` is expected) becomes a compile error instead of a silent bug. Downstream consumers — HF-CTX, substrate activations, synapse extensions — inherit the typed identifiers.

HF-TT applies `~/dev/controlflow/hypermemetic/skills/skills/strong-typing/SKILL.md` systematically to the newly-librified hyperforge workspace.

## Context

Current state (per HF-0 survey):

- **Rich enums already exist:** `Forge` (GitHub/Codeberg/GitLab), `Visibility` (Public/Private), `VersionBump` (Patch/Minor/Major), `PackageRegistry` (CratesIo/Hackage/Npm), `BuildSystemKind` (Cargo/Cabal/Node/Npm/Pnpm/Poetry/...), `PackageStatus`, `PublishActionKind`, `RunnerType` (Local/Docker), 50+ `HyperforgeEvent` variants.
- **Zero string newtypes:** every id-like field is raw `String`. `Repo.name: String`, `RepoRecord.org: String`, `PackageInfo.name: String`, `CrateInfo.name: String`, etc. Nothing prevents a caller from passing a `package_name` to a function expecting a `repo_name`.
- **No `Ecosystem` type:** `BuildSystemKind` lives at the build-tool level (Cargo, Cabal, Npm). There's no overarching `Ecosystem` (Rust, Haskell, JavaScript, Python, ...) that groups build tools. HF-CTX's fact taxonomy wants `Ecosystem` because artifact qualified ids look like `<ecosystem>:<namespace>:<name>`.

## Target newtype inventory (ratified in HF-TT-S01)

| Newtype | Wraps | Replaces usages of | Kind |
|---|---|---|---|
| `RepoName` | `String` | `Repo.name`, `RepoRecord.name`, function params everywhere a repo name travels. | Plain newtype, `#[serde(transparent)]`. |
| `OrgName` | `String` | `RepoRecord.org`, function params around org ownership. | Plain newtype. |
| `WorkspaceName` | `String` | workspace identifiers across `WorkspaceHub`. | Plain newtype. |
| `PackageName` | `String` | `Package.name`, `CrateInfo.name`, `PackageInfo.name`. | Plain newtype. |
| `ArtifactId` | `String` | Publishing + image + release artifact identifiers. | Qualified id; may be a parsed struct `{ ecosystem, namespace, name }` rather than plain string. HF-TT-S01 decides. |
| `Version` | `String` | Crate versions, cabal versions, npm versions — ecosystem-agnostic. | Plain newtype; validation per-ecosystem deferred to a parsing trait. |
| `CommitRef` | `String` | Git SHAs, tags, branch refs when they stand in for a commit. | Plain newtype with `from_sha` / `from_tag` / `from_branch` constructors. |
| `BranchRef` | `String` | Branch names. | Plain newtype. |
| `TagRef` | `String` | Git tags. | Plain newtype. |
| `RepoPath` | `PathBuf` | Paths rooted inside a repo. | Plain newtype over `PathBuf`. |
| `WorkspaceRoot` | `PathBuf` | Absolute workspace root. | Plain newtype. |
| `CredentialKey` | `String` | Secrets / auth keys. | Plain newtype, probably `#[serde(skip)]` on Display. |
| `Ecosystem` | — (new enum) | N/A (new). `{ Rust, Haskell, JavaScript, Python, Go, Ruby, Elixir, … }`. `BuildSystemKind` gets a `fn ecosystem() -> Ecosystem` accessor. | Enum. |

All newtypes derive `Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema` (matching `plans/README.md`'s "all newtypes derive" contract). `#[serde(transparent)]` where the underlying string representation must be preserved on the wire.

## Dependency DAG

```
           HF-TT-S01 (newtype inventory + shape ratification)
                  │
                  ▼
           HF-TT-2 (introduce newtypes in hyperforge-types crate)
                  │
       ┌──────────┼──────────┬──────────┬──────────┐
       ▼          ▼          ▼          ▼          ▼
     HF-TT-3    HF-TT-4    HF-TT-5    HF-TT-6    HF-TT-7
    (Repo*    (Package/  (Version/  (Path/      (Ecosystem +
    migrate)  Artifact)  Commit/    Credential) BuildSystemKind
                         Branch/Tag)             accessor)
                  │          │          │          │          │
                  └──────────┴──────┬───┴──────────┴──────────┘
                                    ▼
                            HF-TT-8 (call-site sweep: hubs)
                                    │
                                    ▼
                            HF-TT-9 (call-site sweep: bins)
                                    │
                                    ▼
                            HF-TT-10 (cross-repo consumer audit)
```

## Phase breakdown

| Phase | Tickets | Notes |
|---|---|---|
| 0. Spike | HF-TT-S01 | Pin the exact newtype set + whether ArtifactId is plain string or parsed struct. Binary-pass. |
| 1. Foundation | HF-TT-2 | Introduce all newtypes in `hyperforge-types`. No consumers yet — types only. |
| 2. Parallel migration | HF-TT-3..7 | Migrate each cluster (Repo, Package/Artifact, Version/Commit/Branch/Tag, Path/Credential, Ecosystem). File-boundary disjoint. |
| 3. Call-site sweep | HF-TT-8, HF-TT-9 | Hubs then bins. Could go parallel but serial is cleaner (hubs first, then bins pick up the new signatures). |
| 4. Consumer audit | HF-TT-10 | Check every sibling repo in the workspace that depends on hyperforge; bump pin and adjust call-sites. |

## Cross-epic contracts pinned

- **All newtypes live in `hyperforge-types`** (the crate extracted in HF-DC-2).
- **Wire format preservation:** every newtype over String uses `#[serde(transparent)]` so existing JSON/wire formats are byte-identical pre and post migration. This is the same discipline used by plexus-core's `TicketId`, `MethodId`, etc. If a consumer already has a database column or config file with `"repo": "plexus-substrate"`, post-HF-TT it still deserializes to `Repo { name: RepoName("plexus-substrate") }`.
- **HF-CTX inherits all newtypes as-is.** The context store's fact taxonomy references `RepoName`, `PackageName`, `CommitRef`, etc. — no re-newtyping, no shadowing.
- **`Ecosystem` sits alongside `BuildSystemKind`, not replaces it.** `BuildSystemKind::Cargo.ecosystem() == Ecosystem::Rust`. `BuildSystemKind::Npm.ecosystem() == Ecosystem::JavaScript`. Keeping both layers honors the existing domain distinction (multiple build tools per ecosystem).

## What must NOT change

- Hyperforge's public CLI behavior — newtypes render as their inner string via `Display`, so logs and stdout look identical.
- Wire format (JSON, TOML) — `#[serde(transparent)]` guarantees byte-identical serialization.
- Activation method signatures **from the caller's perspective at the wire layer** — the wire still accepts strings. Only the in-process Rust API tightens.
- Database schemas — newtypes are storage-transparent.

## Risks

| Risk | Mitigation |
|---|---|
| An existing `String` field has semantic overloading (sometimes a repo name, sometimes a package name in the same struct). | Found during the spike's inventory. If overloaded, split into two fields during this epic (justified in the ticket body). |
| Serde migration breaks an existing on-disk format. | Pre-/post-migration round-trip tests for every type, on a realistic fixture (pulled from a real RepoRecord JSON). |
| Call-site churn is huge. | Split per-cluster. Each HF-TT-3..7 is one file boundary. HF-TT-8/9 are the sweep. |
| `Ecosystem` enum misses a variant. | Make it non-exhaustive (`#[non_exhaustive]`) so adding variants later is backwards-compatible. |

## Out of scope

- Changing method bodies beyond replacing `String` with the newtype.
- Adding new methods or removing existing ones (HF-IR territory).
- Building the fact taxonomy or context store (HF-CTX).
- Introducing a `Fact` or `Scope` type (HF-CTX).
- Publishing newtypes as a public external crate.

## Completion

Sub-epic is Complete when:

- HF-TT-S01 through HF-TT-10 are all Complete.
- `grep` for `name: String` (case-sensitive) in hyperforge's public API surface returns zero results where a newtype should apply.
- `cargo build --workspace` + `cargo test --workspace` green.
- Every sibling workspace consumer still builds.
- `hyperforge-types` version reflects the newtype addition (minor bump); tag `hyperforge-types-v0.2.0` (or whatever lands) local, not pushed.
