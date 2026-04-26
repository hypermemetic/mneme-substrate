---
id: HF-0
title: "hyperforge: unbreak the build (plexus-core 0.4 → 0.5 dual-version conflict)"
status: Complete
type: implementation
blocked_by: []
unlocks: [HF-DC-1]
severity: Critical
target_repo: hyperforge
---

## Problem

Hyperforge's two binary targets (`src/bin/hyperforge.rs`, `src/bin/hyperforge-auth.rs`) fail to compile with 8× `E0277` errors of the form `trait bound DynamicHub: plexus_core::plexus::plexus::Activation is not satisfied`, plus secondary `E0599` (method-not-found on `TransportServerBuilder`) and `E0282` (type annotations needed).

**Root cause:** dual-version conflict in the dependency graph.

Hyperforge's `Cargo.toml` pins:
```toml
plexus-core      = "0.4"   # stale — workspace has shipped 0.5.0
plexus-macros    = "0.4"   # stale — workspace has shipped 0.5.1
plexus-transport = "0.2"   # transitively depends on plexus-core 0.5
```

plexus-transport 0.2 brings plexus-core 0.5 into the graph, but hyperforge's own code imports `plexus_core::Activation` against the 0.4 version. When hyperforge passes a `DynamicHub` (instantiated from 0.5) into a `TransportServerBuilder` (also from 0.5 via plexus-transport), the compiler sees `DynamicHub: plexus_core_0_5::Activation` but the caller is asking for `plexus_core_0_4::Activation` — distinct traits. Cascade.

This is the exact failure mode `feedback_version_bumps_as_you_go.md` predicted: IR epic bumped plexus-core 0.4 → 0.5 in plexus-substrate, plexus-macros, plexus-transport, synapse. Hyperforge sat behind without an audit sweep. Now it's one of several downstream casualties.

## Context

Survey subagent `ad95774c9b91c89c0` produced the following findings:

- **Root path:** `/Users/shmendez/dev/controlflow/hypermemetic/hyperforge/`
- **Layout:** single binary crate (not a workspace), version `4.1.0`. 21 module directories under `src/`. Three binaries under `src/bin/`: `hyperforge.rs`, `hyperforge-auth.rs`, `hyperforge-ssh.rs`.
- **CLI boundary:** already clean. `src/lib.rs` is pure module re-exports (32 lines, no I/O). CLI I/O lives in `src/bin/*`.
- **Activation surface:** already multi-activation. 7 activations (`HyperforgeHub`, `WorkspaceHub`, `RepoHub`, `BuildHub`, `ImagesHub`, `ReleasesHub`, `AuthHub`). ~74 `#[plexus_macros::method]` attrs. **No `#[child]` gates** — all children hardcoded via `HyperforgeState`. No `MethodRole::DynamicChild` usage.
- **Recent history:** commit `89ff906` (switch plexus deps path → crates.io), commit `72c593e` (partial 0.5 migration for AuthContext + RawRequestContext — incomplete). HF-0 completes that migration.
- **Existing abstractions:** `Repo`, `RepoRecord`, `CrateInfo`, `PackageInfo`, `BuildSystemKind` (Cargo/Cabal/Node/Npm/Pnpm/Poetry/...), `PackageRegistry` (CratesIo/Hackage/Npm), `PackageStatus`, `Forge` (GitHub/Codeberg/GitLab), `Visibility`, `VersionBump`, `VersionMismatch`. No string newtypes.

Failing files & specific errors:

- `src/bin/hyperforge-auth.rs:71` — `DynamicHub: Activation` not satisfied
- `src/bin/hyperforge.rs:140, 174` — same
- Secondary errors: method signatures on `TransportServerBuilder` (method not found) and builder chain type inference (E0282).

## Required behavior

1. Bump hyperforge's plexus pins to align with the workspace:
   ```toml
   plexus-core      = "0.5"
   plexus-macros    = "0.5"
   plexus-transport = "0.2"   # verify 0.2 is current — may need bump too
   ```
2. Apply any call-site adjustments commit `72c593e` (partial 0.5 migration) left incomplete. The `TransportServerBuilder` signature and `DynamicHub.arc_into_rpc_module()` (or equivalent) shape changed between 0.4 and 0.5. Read plexus-core 0.5's `src/transport.rs` (or wherever `TransportServerBuilder` lives) and align hyperforge's call sites.
3. If hyperforge consumes any plexus-core symbols deprecated in IR-4 (`ChildCapabilities`, `PluginSchema::is_hub()`, hand-written `plugin_children()`), wrap in `#[allow(deprecated)]` with a `// TODO(HF-IR): migrate to <replacement>` marker. Do NOT migrate to the replacement in this ticket — HF-IR owns that cleanup.
4. Audit sibling workspace crates after the bump. Grep `**/Cargo.toml` for any remaining pins at plexus-core 0.4.x or plexus-macros 0.4.x; bump each in the same PR or an immediately-following commit. Use `cargo tree -d` in hyperforge to verify single-version-in-graph post-fix.
5. Bump hyperforge's own version: `4.1.0` → `4.1.1` (patch bump — bug fix, no API surface change). Tag `hyperforge-v4.1.1` locally per `feedback_version_bumps_as_you_go.md`. Do not push.

## Risks

| Risk | Mitigation |
|---|---|
| plexus-transport 0.2 is itself stale. | `cargo search plexus-transport` or check the plexus-transport crate's `Cargo.toml` in the workspace. Bump if a newer version exists. |
| `arc_into_rpc_module` / `TransportServerBuilder` changed in ways that need more than a one-line call-site update. | Read plexus-core 0.5's transport module end-to-end. Adjust the entire build chain in hyperforge's bins at once. If the shape fundamentally changed (not just renamed), document the delta in the commit body. |
| Sibling crates (hub-* family, Ledger, other workspace participants) also drifted. | Audit sweep per `feedback_version_bumps_as_you_go.md`. Any that drift get their own ticket — do not attempt to fix every downstream casualty in HF-0. HF-0 is hyperforge-only. |
| Adding `#[allow(deprecated)]` accidentally silences legitimate warnings. | Narrowly scope: wrap the specific expression, not the enclosing fn/mod. Every `#[allow(deprecated)]` site gets an enumerated TODO in the commit body. |
| Fix reveals a deeper design issue (e.g., hyperforge's hub registration pattern doesn't map to 0.5 cleanly). | HF-0's scope is minimal — add `#[allow(deprecated)]` and/or the minimum call-site adjustment to get compile green. Larger design issues become HF-IR tickets. |

## What must NOT change

- Hyperforge's public CLI behavior (arg grammar, exit codes, stdout format).
- Hyperforge's activation namespaces (`hyperforge`, plus child hubs — preserve).
- Any substrate / synapse / plexus-core behavior. HF-0 touches hyperforge only.
- The existing module layout (no extraction to a separate crate — that's HF-DC).
- Any type signatures at public API boundaries (no newtype introduction — that's HF-TT).
- Static-vs-dynamic child routing shape (HF-IR's territory).

## Acceptance criteria

1. `cargo build` at `/Users/shmendez/dev/controlflow/hypermemetic/hyperforge/` exits 0. Full command output captured in the commit body.
2. `cargo test` at the same root exits 0 OR pre-existing failures are enumerated with a `git stash` demonstration that they reproduce byte-identically pre-fix.
3. `cargo tree -d` in hyperforge shows a single version of each plexus-* crate. No duplicates.
4. Sibling workspace audit: `cargo build` green in `plexus-substrate`, `plexus-core`, `plexus-macros`, `plexus-transport`, `synapse` still succeeds after HF-0 lands.
5. Commit body includes: (a) the one-paragraph root-cause summary (pulled from the HF-0 survey report in the task output), (b) the exact version bumps in hyperforge's `Cargo.toml`, (c) each `#[allow(deprecated)]` site added with its `// TODO(HF-IR):` marker.
6. Hyperforge's own version bumped `4.1.0` → `4.1.1` in `Cargo.toml`; annotated tag `hyperforge-v4.1.1` created locally (not pushed).

## Completion

PR against hyperforge. Status flipped from `Ready` → `Complete` in the same commit as the fix. Unblocks HF-DC-1 and the rest of the HF phases.
