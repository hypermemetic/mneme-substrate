---
id: HF-DC-1
title: "HF-DC sub-epic — hyperforge librification + decoupling"
status: Epic
type: epic
blocked_by: [HF-0]
unlocks: [HF-TT-1]
target_repo: hyperforge
---

## Goal

End state: hyperforge's single monolithic binary crate becomes a small workspace of curated-public-API library crates plus thin bin adapters. Downstream consumers (future TM-as-HF-CTX, other workspace tools, external callers) can depend on the specific library crate they need without pulling CLI/IO surface, binary dependencies, or unrelated hub modules.

The CLI boundary is already clean (per HF-0's survey — `src/lib.rs` is pure re-exports, I/O lives in `src/bin/*`), so HF-DC is not about untangling a monolith. It's about factoring the current single-crate layout into multiple published/depable units with explicit public-API boundaries.

## Context

Current state (HF-0 survey output):

- Single binary crate at `/Users/shmendez/dev/controlflow/hypermemetic/hyperforge/`, version `4.1.x` post-HF-0.
- `src/lib.rs` exports 21 modules.
- 7 activations with ~74 methods distributed across `src/hub.rs` (HyperforgeHub) and `src/hubs/*` (Workspace, Repo, Build, Images, Releases, Auth).
- Rich domain types live in `src/types/`, `src/build_system/`, `src/adapters/registry/`.
- No existing crate split — every module lives in the same crate's namespace.

Motivation for the split:

1. **HF-CTX (context store) needs to depend on hyperforge's domain types without pulling CLI/auth/SSH bits.** If HF-CTX lives inside hyperforge's single crate, it can be a sibling module, but any consumer outside hyperforge-the-project would pull the world. A types crate scoped to domain concepts is the minimum enabler.
2. **Type tightening (HF-TT) wants a tightly-scoped crate to introduce newtypes in.** Putting newtypes in a `hyperforge-types` crate constrains blast radius: bumping the types crate ripples through core but not bins.
3. **Downstream consumption.** Substrate activations, synapse extensions, or other workspace tools that just need to reference a `PackageName` or `RepoRecord` should not need to compile the auth sidecar or the SSH binary.

## Proposed crate split (ratified in HF-DC-S01 spike)

| Crate | Owns | Depended on by |
|---|---|---|
| `hyperforge-types` | Domain types (`Repo`, `RepoRecord`, `Forge`, `Visibility`, `PackageRegistry`, `BuildSystemKind`, `VersionBump`, `VersionMismatch`, `HyperforgeEvent` taxonomy, and all newtypes introduced in HF-TT) | core, hubs, bins, HF-CTX, external |
| `hyperforge-core` | Business logic: adapters, build_system, package, auth, git, services (minus I/O orchestration that only makes sense inside bins) | hubs, bins, HF-CTX |
| `hyperforge-hubs` | The 7 activation implementations (hub.rs + hubs/*) | bins, HF-CTX, test harnesses |
| `hyperforge` (bin) | CLI adapter, server startup, tracing, arg parsing. Depends on hubs + core + types. | (end user) |
| `hyperforge-auth` (bin) | Secrets sidecar | (end user) |
| `hyperforge-ssh` (bin) | SSH handler | (end user) |

This is the spike's **proposed** split. HF-DC-S01 confirms or adjusts before any HF-DC-N ticket is promoted.

## Dependency DAG

```
         HF-DC-S01 (crate split spike)
                 │
                 ▼
         HF-DC-2 (extract hyperforge-types)
                 │
                 ▼
         HF-DC-3 (extract hyperforge-core)
                 │
        ┌────────┼────────┐
        ▼        ▼        ▼
      HF-DC-4  HF-DC-5  HF-DC-6
    (hubs)  (hf bin)  (auth bin)
        │        │        │
        └────────┼────────┘
                 ▼
            HF-DC-7 (ssh bin)
                 │
                 ▼
            HF-DC-8 (workspace polish: features, docs, ci)
```

## Phase breakdown

| Phase | Tickets | Notes |
|---|---|---|
| 0. Spike | HF-DC-S01 | Ratify the crate split. Binary-pass. Must complete before phase 1. |
| 1. Types extraction | HF-DC-2 | `hyperforge-types`. Foundation for everything else. |
| 2. Core extraction | HF-DC-3 | `hyperforge-core`. Depends on types. |
| 3. Parallel bin extractions | HF-DC-4, HF-DC-5, HF-DC-6, HF-DC-7 | Hubs crate + each bin. File-boundary disjoint. |
| 4. Polish | HF-DC-8 | Features gating, crate-level docs, CI matrix for the workspace. |

## Cross-epic contracts pinned

- **Crate names:** `hyperforge-types`, `hyperforge-core`, `hyperforge-hubs`, plus bins `hyperforge`, `hyperforge-auth`, `hyperforge-ssh`. Confirmed in HF-DC-S01 or revised; final names recorded here post-spike.
- **Workspace root:** hyperforge becomes a Cargo workspace. Root `Cargo.toml` is `[workspace]`-only; member crates live in `crates/<name>/`.
- **Re-exports:** `hyperforge-core::prelude` re-exports the most common types from `hyperforge-types` so consumers don't always need both deps. Opt-in via feature flag.
- **Version independence:** each member crate versions independently. Version-tagging convention unchanged: `<crate>-v<version>`.

## What must NOT change

- Public CLI behavior (arg grammar, exit codes, output format).
- Activation namespaces (`hyperforge`, `auth`, etc.).
- The 74 existing methods' signatures (stays in whichever crate owns them; call-site paths change, not shapes). HF-TT is where shapes change.
- External consumers importing `hyperforge::foo` via the old path should break cleanly (compile error pointing at new path), not silently resolve to a stale symbol.

## Out of scope

- Introducing newtypes (HF-TT).
- Adopting `#[child]` gates on hyperforge's hubs (HF-IR).
- Building the context store (HF-CTX).
- Publishing any of the new crates to crates.io (workspace-internal only for now).
- Rewriting any activation's method list.

## Completion

Sub-epic is Complete when:

- HF-DC-S01 through HF-DC-8 are all Complete.
- `cargo build --workspace` and `cargo test --workspace` in hyperforge succeed.
- Every sibling workspace repo that depends on hyperforge still builds (audit sweep per the version-bump memory).
- A brief `hyperforge/README.md` update documents the new workspace layout and which crate to depend on for which use case.
