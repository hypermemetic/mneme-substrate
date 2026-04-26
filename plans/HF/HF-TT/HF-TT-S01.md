---
id: HF-TT-S01
title: "Spike: ratify newtype inventory, ArtifactId shape, Ecosystem variants"
status: Pending
type: spike
blocked_by: [HF-DC-1]
unlocks: [HF-TT-2]
target_repo: hyperforge
severity: High
---

## Problem

Before newtypes are introduced at API boundaries across hyperforge, three open questions must be resolved with evidence from the actual codebase rather than guesses. First, whether `ArtifactId` is a plain `String` newtype or a parsed struct `{ ecosystem, namespace, name }` — the downstream HF-CTX fact taxonomy wants qualified ids, but a parsed struct imposes validation cost at every construction site. Second, the exhaustive list of `Ecosystem` variants that hyperforge touches today (Rust / Haskell / JavaScript / Python / Go / Ruby / Elixir — is any one missing? is any one unused?). Third, whether the proposed 12-item newtype inventory is complete — a grep of `name: String` and equivalent patterns may surface additional domain identifiers not currently pinned in `HF-TT-1.md`.

Until these three questions are answered with binary pass / fail, HF-TT-2's foundation work cannot be promoted to Ready without risking mid-epic rework.

## Context

The sub-epic overview `plans/HF/HF-TT/HF-TT-1.md` pins a provisional inventory of 12 newtypes plus the `Ecosystem` enum. The provisional list is:

| Newtype | Wraps | Kind |
|---|---|---|
| `RepoName` | `String` | Plain |
| `OrgName` | `String` | Plain |
| `WorkspaceName` | `String` | Plain |
| `PackageName` | `String` | Plain |
| `ArtifactId` | `String` or parsed struct | TBD — this spike decides |
| `Version` | `String` | Plain |
| `CommitRef` | `String` | Plain |
| `BranchRef` | `String` | Plain |
| `TagRef` | `String` | Plain |
| `RepoPath` | `PathBuf` | Plain |
| `WorkspaceRoot` | `PathBuf` | Plain |
| `CredentialKey` | `String` | Plain |
| `Ecosystem` | — (enum) | `#[non_exhaustive]` |

Hyperforge at HF-DC completion is a Cargo workspace with `hyperforge-types`, `hyperforge-core`, `hyperforge-hubs`, and bin crates. This spike reads the post-HF-DC tree.

## Required behavior

The spike produces a written deliverable (a short markdown report checked into the hyperforge repo at `docs/hf-tt-s01-report.md`) that answers, for each question, yes-or-no with supporting evidence (grep counts, file paths, concrete usage citations):

| Question | Expected deliverable |
|---|---|
| Is `ArtifactId` plain or parsed? | Decision (`plain` or `parsed`) with rationale citing concrete call sites that either benefit from parsing or would be burdened by it. If `parsed`, pin the struct shape: `{ ecosystem: Ecosystem, namespace: String, name: PackageName }` (or revision). |
| What are the `Ecosystem` variants? | Final list with `#[non_exhaustive]` confirmed. Must include every build tool family present in the current `BuildSystemKind` enum. |
| Is the 12-item inventory complete? | Confirmed complete, or augmented with additional newtypes found during the grep sweep. Each addition has a wrap type and a one-line rationale. |
| Any overloaded fields? | List of `String` fields whose semantic is split (repo-name-sometimes, package-name-other-times). Each one is flagged for a split decision that HF-TT-3..7 implement. |

The report also pins: exact location of the newtype module inside `hyperforge-types` (e.g., `crates/hyperforge-types/src/newtypes/`), one file per cluster (repo, package, version, path, credential, ecosystem).

## Risks

| Risk | Mitigation |
|---|---|
| `ArtifactId` parsing imposes runtime validation cost at every hot path. | Spike measures the blast radius: if `ArtifactId` is constructed in tight loops, plain wins. |
| Additional newtypes surface mid-epic. | The spike's grep sweep is exhaustive before HF-TT-2 starts. Late discoveries become their own follow-up ticket, not scope creep inside HF-TT-2. |
| `Ecosystem` variants miss a real-world ecosystem hyperforge silently supports. | `#[non_exhaustive]` lets a variant be added post-spike without a breaking change. |

## What must NOT change

- No source-code changes. The spike is read-only against the post-HF-DC tree.
- No newtype definitions land in this ticket — the next ticket (HF-TT-2) owns that.
- `plans/HF/HF-TT/HF-TT-1.md` is not rewritten; the report is a sibling deliverable that HF-TT-2 references.

## Acceptance criteria

1. A report file exists at `docs/hf-tt-s01-report.md` inside the hyperforge repo with all four questions answered.
2. `ArtifactId` shape is pinned as either `plain String newtype` or a concrete parsed struct whose fields are spelled out.
3. `Ecosystem` variant list is pinned and marked `#[non_exhaustive]`.
4. The newtype inventory is either confirmed identical to the HF-TT-1 provisional list or augmented with additions that each have a wrap-type and a one-line rationale.
5. Overloaded-field list is produced (even if empty). Each entry names the struct, the field, the two semantics observed, and which newtype cluster will split it.
6. File-module layout inside `hyperforge-types` is pinned (one source file per newtype cluster: `repo.rs`, `package.rs`, `version.rs`, `path.rs`, `credential.rs`, `ecosystem.rs`).
7. `cargo build --workspace` and `cargo test --workspace` in hyperforge are green (the spike itself makes no code changes; this verifies the pre-HF-TT baseline is clean).

## Completion

Implementor commits the report markdown to hyperforge, runs `cargo build --workspace && cargo test --workspace` to confirm the baseline is green, flips status to Complete in the same commit. No version bump (no public surface changed).
