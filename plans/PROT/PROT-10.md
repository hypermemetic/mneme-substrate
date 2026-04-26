---
id: PROT-10
title: "PROT release: publish crates.io + push tags + end-to-end synapse drill-down verification"
status: Pending
type: implementation
blocked_by: [PROT-3, PROT-4, PROT-5, PROT-6, PROT-7, PROT-8, PROT-9]
unlocks: []
severity: Critical
target_repo: multiple
---

## Problem

PROT-2 through PROT-9 land the unified schema protocol across six Rust crates and two Haskell crates. This ticket ships them: publish the cargo crates to crates.io in dependency order, push all git tags + main branches, reinstall the synapse CLI, restart hyperforge against the new binary, and verify the HF-AUDIT-3 reproducer succeeds.

## Context

Publish order is strict — each crate's dependencies must be on crates.io before `cargo publish` for that crate will succeed.

Synapse + plexus-protocol are Haskell; not published to crates.io. Hyperforge is a binary; git tags + push only, no `cargo publish`.

## Required behavior

### Publishing (cargo)

Publish in dependency order:

1. `plexus-core 0.6.0` — `cd plexus-core && cargo publish`. Waits for indexing.
2. `plexus-macros 0.6.0` — same.
3. `plexus-transport 0.3.0` — same.
4. `plexus-substrate 0.6.0` — same.

Skip `cargo publish` for hyperforge (binary, not a library consumer pattern) and for any sibling that doesn't publish.

### Git pushes

For each affected repo (`plexus-core`, `plexus-macros`, `plexus-transport`, `plexus-protocol`, `synapse`, `plexus-substrate`, `hyperforge`):
- `git push origin main` (or equivalent branch).
- `git push origin <tag>` for each local tag created in PROT-2 through PROT-8.

For repos with multiple remotes (plexus-substrate has `codeberg` as `origin` and `github` as a secondary remote): push to both where tags don't conflict.

### Synapse CLI reinstall

```
cd /Users/shmendez/dev/controlflow/hypermemetic/synapse
cabal install exe:synapse --overwrite-policy=always
```

Verify `synapse --version` reports `4.0.0`.

### Hyperforge restart

```
kill <old-pid>  # if PID 18804 from autonomous run still running
cd /Users/shmendez/dev/controlflow/hypermemetic/hyperforge
cargo build --release --bin hyperforge
./target/release/hyperforge --port 44104 --no-register --no-secrets &
```

### End-to-end verification (THE RUBRIC)

The following commands must produce correct output — this is the pass/fail bar for the whole epic:

1. **Tree render at root:**
   `synapse lforge hyperforge`
   Expected: tree listing workspace/repo/build as child activations, plus top-level methods (status, reload, begin, orgs_list, auth_*).

2. **Drill into BuildHub:**
   `synapse lforge hyperforge build`
   Expected: BuildHub's schema tree, listing methods (unify, analyze, dirty, run, release, etc.). **This is the HF-AUDIT-3 reproducer — previously failed.** Must succeed.

3. **User method on child activation:**
   `synapse lforge hyperforge build dirty path=/Users/shmendez/dev/controlflow/hypermemetic/ all_git=true`
   Expected: stream of dirty-repo events; summary line (e.g., "N dirty, M clean (K repos checked)"); Done event.

4. **Drill into WorkspaceHub:**
   `synapse lforge hyperforge workspace`
   Expected: WorkspaceHub's schema tree.

5. **Drill into RepoHub:**
   `synapse lforge hyperforge repo`
   Expected: RepoHub's schema tree.

6. **Raw JSON-RPC sanity** (baseline — should still work):
   `echo '{"jsonrpc":"2.0","id":1,"method":"lforge.call","params":{"method":"hyperforge.status","params":{}}}' | websocat -t ws://127.0.0.1:44104/`
   Expected: subscription ID + Status event + Done.

### Ticket closures

- **Flip PROT-2 through PROT-9 to Complete** — each landed their scope and integration gate passed.
- **Flip HF-AUDIT-3 to Complete** — bug fixed. Cite PROT-3 as the fix.
- **Update HF-AUDIT-1 and HF-AUDIT-2** — note that the plexus-core version in their fix plans is now 0.6, not 0.5. They remain Pending for user prioritization.
- **Update plans/README.md** (if it tracks cross-epic contracts) — record the unified `.schema` protocol under "Pinned cross-epic contracts".

## Risks

| Risk | Mitigation |
|---|---|
| A crates.io publish fails mid-chain (e.g., plexus-transport) due to resolution timing. | `cargo publish` blocks on indexing. Retry once if a single publish fails. If it fails twice, investigate manifest. |
| An sibling workspace crate wasn't bumped per PROT-9 and breaks during the final verify sweep. | PROT-9 should have caught it. Any surviving drift gets an HF-AUDIT-N ticket; doesn't block PROT-10 itself. |
| Synapse reinstall fails due to cabal environment. | Check `cabal --version` pre-run. If GHC toolchain issue, file separately; this ticket can still report everything ELSE green. |
| Hyperforge restart hangs on port 44104 (e.g., address still in TIME_WAIT). | `--port 44105` workaround; update synapse commands accordingly. |
| End-to-end rubric item 2 (drill into BuildHub) still fails. | Means PROT-3's macro fix is incomplete. File HF-AUDIT-4, DO NOT flip PROT-3 to Complete. This is the rubric, not a checkbox. |
| Tag collision on push (plexus-substrate has pre-existing older tags on github remote that don't match local). | Push each new tag individually: `git push origin plexus-substrate-v0.6.0`. Skip conflicting older tags. |

## What must NOT change

- No existing tickets are rolled back or modified beyond the closures listed above.
- No patches added beyond the commits landed in PROT-2-9.
- No behavior changes beyond the unified schema protocol.

## Acceptance criteria

1. `cargo publish` succeeded for plexus-core 0.6.0, plexus-macros 0.6.0, plexus-transport 0.3.0, plexus-substrate 0.6.0. Confirmed via crates.io search or `cargo search`.
2. All git tags pushed to origin (codeberg) and github where applicable.
3. `synapse --version` reports `4.0.0`.
4. Hyperforge binary at PID <new-pid> (recorded in commit) is the 5.0.0 build.
5. All 6 verification commands (tree renders at root, build/workspace/repo drill-downs, user method invocation, raw JSON-RPC baseline) succeed.
6. HF-AUDIT-3 is Complete. PROT-2 through PROT-9 are Complete.
7. Commit body in plexus-substrate's `plans/PROT/` commit includes:
   - List of published crate versions.
   - List of tag pushes.
   - Synapse reinstall output.
   - Hyperforge restart log excerpt.
   - Output (stdout head) of each verification command.

## Completion

The ticket is Complete when the workspace is fully shipped and verified. PROT meta-epic is then Complete.
