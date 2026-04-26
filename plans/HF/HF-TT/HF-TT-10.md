---
id: HF-TT-10
title: "Cross-repo consumer audit: bump hyperforge pin and adjust call sites everywhere"
status: Pending
type: implementation
blocked_by: [HF-TT-9]
unlocks: []
target_repo: hyperforge
severity: High
---

## Problem

HF-TT-2 through HF-TT-9 tightened hyperforge's public surface. Every sibling workspace repo that depends on hyperforge (at minimum: plexus-substrate; potentially plexus-core, plexus-macros, synapse) pins a pre-migration version and will fail to build once the new minor versions are published or consumed. This ticket is the scheduled sweep: each consumer bumps its hyperforge dependency pin and adjusts call sites to satisfy the new newtype-taking signatures. Without this ticket, the HF-TT sub-epic only ships half of its promise — hyperforge is tight internally but every downstream crate still passes raw strings at the boundary and the migration work leaks cross-repo.

## Context

Consumer repos to audit (exhaustive list ratified at the start of this ticket by the implementor, not guessed):

- `/Users/shmendez/dev/controlflow/hypermemetic/plexus-substrate/`
- `/Users/shmendez/dev/controlflow/hypermemetic/plexus-core/`
- `/Users/shmendez/dev/controlflow/hypermemetic/plexus-macros/`
- `/Users/shmendez/dev/controlflow/hypermemetic/synapse/` (if exists and depends on hyperforge)
- Any other workspace repo found via `grep -l 'hyperforge' */Cargo.toml`

For each consumer:

1. Bump `Cargo.toml` pin to the new `hyperforge-types` / `hyperforge-core` / `hyperforge-hubs` versions.
2. Fix every compile error — they will all be shape-of: "expected `RepoName`, found `String`" or equivalent. Wrap with `RepoName::new(s)` at the call boundary, or (preferably) propagate the newtype upstream in the consumer's own API.
3. Run the consumer's own `cargo build --workspace && cargo test --workspace`.
4. Commit per consumer (one commit per repo, clean-per-crate).

File-boundary discipline: this ticket edits many repos, but each repo is one file-boundary in the cross-repo sense. Implementation proceeds repo-by-repo. Within a repo, the implementor decides whether to propagate newtypes deeper or stop at the seam — the mandate is "workspace builds green with the new pin", not "every consumer is fully typed."

## Required behavior

For each consumer repo:

| Item | Expected state after ticket |
|---|---|
| `Cargo.toml` pin on hyperforge crates | Bumped to HF-TT-sub-epic-final versions. |
| Call sites that pass a string to a hyperforge hub method | Compile — either the consumer propagates the newtype or wraps at the seam. |
| `cargo build --workspace` | Green. |
| `cargo test --workspace` | Green. |
| Wire format of the consumer's own RPC / DB schemas | Byte-identical pre/post pin bump (verified by existing tests; no new fixtures required from this ticket unless a consumer explicitly serializes a hyperforge type). |

Commit message per consumer documents: (a) the old and new hyperforge version pins, (b) the rough count of call-site fixes, (c) whether the consumer chose seam-wrapping or type-propagation.

## Risks

| Risk | Mitigation |
|---|---|
| A consumer repo's own ticketing discipline requires its own sub-epic, not a sweep. | If a consumer repo has >50 call-site fixes or introduces a new architectural boundary, the implementor stops, files a follow-up ticket for that consumer's migration, and seam-wraps in this ticket to keep the pin bump minimal. |
| A consumer uses hyperforge via a private API not covered by newtypes (e.g., a `core::internal::*` path). | Out of scope for this ticket. That's a hyperforge hygiene issue. File a follow-up. |
| Workspace repos are out of sync with hyperforge's new versions because they pin by git SHA, not semver. | Each consumer bumps by whatever pinning mechanism it uses (SHA or version). Criterion is "the consumer builds against the post-HF-TT hyperforge tree", not "semver is respected." |
| A consumer repo is archived / unused and can be skipped. | Implementor confirms in the commit message that the repo is archived and skips it. Not every grep-hit is a live consumer. |

## What must NOT change

- The consumer repos' own public APIs (they are not being newtype-migrated themselves in this ticket — that's their own future epic if warranted).
- The consumer repos' wire formats.
- The consumer repos' test coverage (every existing test must still pass; no tests are deleted).
- Hyperforge itself — no new changes to hyperforge in this ticket. If a hyperforge gap is found mid-sweep, file a follow-up.

## Acceptance criteria

1. Every consumer repo identified in the audit list builds green against the post-HF-TT hyperforge tree.
2. Every consumer repo's `cargo test --workspace` passes.
3. Each consumer repo has a commit containing the pin bump and call-site fixes, with a message documenting old-pin / new-pin / fix-count / seam-vs-propagate choice.
4. Audit list is exhaustive: grep across the `hypermemetic` workspace parent directory for `hyperforge` in every `Cargo.toml` produces no consumer not covered by this ticket's commit list.
5. For any consumer skipped (archived / unused), commit message or ticket closure note explicitly documents the skip.
6. No hyperforge repo changes in this ticket (`git diff` on hyperforge shows no new commits attributable to this ticket).

## Completion

Implementor commits per-consumer, confirms each consumer's build + test green against the post-HF-TT hyperforge tree, flips this ticket's status to Complete in the last per-consumer commit (or a dedicated closure commit in hyperforge's plans directory). HF-TT sub-epic completion gate closes once this ticket is Complete.
