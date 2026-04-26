---
id: HF-AUDIT-1
title: "align plexus-core/macros pins across sibling workspace crates surfaced by HF-0"
status: Pending
type: implementation
blocked_by: [HF-0]
unlocks: []
severity: Medium
target_repo: multiple
---

## Problem

HF-0's workspace audit sweep (per `feedback_version_bumps_as_you_go.md`) surfaced three sibling crates outside hyperforge that still pin plexus-core 0.4.0 / plexus-macros 0.4.0, and in one case plexus-transport 0.1.0. Each will eventually hit the same dual-version dep-graph conflict hyperforge hit if anything transitively pulls in 0.5 — or just fail to compile if they haven't been built since the IR-epic bumps.

Scope is distinct from HF-0 (which was hyperforge-only by policy). Each sibling crate gets an audit: does it currently build as-is, does it need the bump, and is it active enough to be worth bumping right now versus deferring until it's next touched.

## Sibling crates with confirmed stale pins

Per the HF-0 agent's report (task `a8a26e2f5c7dbfff3`):

| Repo | Path | Stale pins |
|---|---|---|
| mono-provider | `/Users/shmendez/dev/controlflow/hypermemetic/mono-provider/Cargo.toml` | `plexus-core = "0.4.0"`, `plexus-macros = "0.4.0"` |
| plexus-music-royalty-free | `/Users/shmendez/dev/controlflow/hypermemetic/plexus-music-royalty-free/Cargo.toml` | `plexus-core = "0.4.0"`, `plexus-macros = "0.4.0"` |
| plexus-mono | `/Users/shmendez/dev/controlflow/hypermemetic/plexus-mono/Cargo.toml` | `plexus-core = "0.4.0"`, `plexus-macros = "0.4.0"`, `plexus-transport = "0.1.0"` |

## Context

HF-0's resolution for hyperforge used a local `.cargo/config.toml` with `[patch.crates-io]` pointing at sibling paths, because the 0.5.x workspace crates are unpublished. Each sibling here can follow the same pattern OR bump Cargo.toml pins directly if the sibling intends to consume crates.io 0.5.0 once it ships.

Open questions per sibling (resolve in the per-repo audit pass):

1. Does the crate currently build? If yes as-is (no 0.5 in its transitive graph), a pin bump might be premature — the current state is intentional.
2. Is the crate active (recent commits, active use)? A dormant crate shouldn't get a reactive bump; a defer-until-next-touch stance is fine.
3. Does the crate actually touch plexus-* symbols that changed between 0.4 and 0.5? If not, bump is trivial; if yes, same call-site work HF-0 did applies.

## Required behavior

For each sibling:

1. Cd into the repo, run `cargo build`. Capture output.
2. If green as-is: no action required beyond logging "deferred — builds cleanly at current pins" with a note about when to revisit (next time the sibling is touched substantively).
3. If broken with the same E0277 / E0599 / E0282 pattern hyperforge hit: apply the same fix (Cargo.toml bump to 0.5, add `.cargo/config.toml` pointing at local paths if needed, call-site fixes as surfaced).
4. If broken with a different failure mode: stop and file a fresh ticket for that sibling — do NOT force-fit HF-0's fix.
5. Version-bump the sibling crate per `feedback_version_bumps_as_you_go.md` when public surface changed (patch bump for pure dep-pin update, minor if call-sites changed meaningfully).
6. Audit consumers of each sibling if any exist in the workspace (transitive sweep). 

## What must NOT change

- hyperforge's build (already green via HF-0). No changes to hyperforge in this ticket.
- Any sibling's public surface beyond what's required to compile.
- Any sibling's `Cargo.lock` in isolation — regenerate via a fresh build, don't hand-edit.

## Acceptance criteria

1. Each sibling's current build state captured in the commit body (green-as-is / broken-with-E-codes / broken-differently).
2. For siblings that were broken: `cargo build` and `cargo test` green post-fix. Integration gate per rule 12.
3. For siblings that were green-as-is but should nonetheless bump: confirmation that the bump commit still builds. No speculative bumps for "cleanliness" without a concrete reason.
4. No duplicate plexus-core versions in any sibling's `cargo tree -d` post-fix.
5. Commit per repo with a per-repo message summarizing what was done and why (or why deferred).
6. Version bump + local tag per sibling where public surface changed. Not pushed.

## Completion

Per-repo commits in each sibling; HF-AUDIT-1 itself is a tracking ticket in substrate's plans/. Flip to Complete when all three siblings have been audited (bumped or explicitly deferred with note) and the integration gate is green where fixes were applied.
