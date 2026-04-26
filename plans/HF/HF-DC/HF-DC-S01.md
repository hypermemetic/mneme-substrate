---
id: HF-DC-S01
title: "Spike: ratify the hyperforge crate split"
status: Pending
type: spike
blocked_by: [HF-0]
unlocks: [HF-DC-2, HF-DC-3, HF-DC-4, HF-DC-5, HF-DC-6, HF-DC-7, HF-DC-8]
severity: High
target_repo: hyperforge
---

## Problem

HF-DC-1 proposes a four-crate split (`hyperforge-types`, `hyperforge-core`, `hyperforge-hubs`, plus the three existing bins). The proposal is informed by HF-0's survey but has not been ratified against the actual module graph. Before any extraction ticket lands, the split must be binary: every module, every `pub use`, every `[dependencies]` line has a single unambiguous destination crate. Ambiguity at implementation time causes thrash — modules bouncing between types and core on per-import reconsideration.

## Context

HF-0 survey results pinned:

- Hyperforge at `/Users/shmendez/dev/controlflow/hypermemetic/hyperforge/`, currently a single binary crate at version `4.1.x` (post-HF-0 gate).
- `src/lib.rs` exports 21 modules.
- 3 bins in `src/bin/`: `hyperforge`, `hyperforge-auth`, `hyperforge-ssh`.
- 7 activations totaling ~74 methods across `src/hub.rs` and `src/hubs/*`.
- Rich domain types in `src/types/`, `src/build_system/`, `src/adapters/registry/`.
- Clean lib/bin boundary (I/O lives in bins, library code is pure).
- Zero string newtypes today (HF-TT's concern, not this spike's).

Candidate split from HF-DC-1:

| Crate | Owns |
|---|---|
| `hyperforge-types` | Domain types: `Repo`, `RepoRecord`, `Forge`, `Visibility`, `PackageRegistry`, `BuildSystemKind`, `VersionBump`, `VersionMismatch`, `HyperforgeEvent`, `PackageInfo`, `CrateInfo`, and related enums/structs. |
| `hyperforge-core` | Business logic: adapters, build_system, package, auth, git, services. |
| `hyperforge-hubs` | 7 activation impls (HyperforgeHub, WorkspaceHub, RepoHub, BuildHub, ImagesHub, ReleasesHub, AuthHub). |
| `hyperforge` (bin) | CLI adapter. |
| `hyperforge-auth` (bin) | Secrets sidecar. |
| `hyperforge-ssh` (bin) | SSH handler. |

This spike confirms the names, pins the module-to-crate mapping, and records open questions and their resolutions.

## Required behavior

Output of the spike is a decision document committed at `/Users/shmendez/dev/controlflow/hypermemetic/hyperforge/docs/architecture/<timestamp>_hf-dc-crate-split.md` (using the reverse-chronological naming convention from `CLAUDE.md`). The document answers:

1. **Final crate names.** Ratified or adjusted from the HF-DC-1 proposal. If adjusted, rationale recorded.
2. **Module-to-crate mapping.** A table mapping every module listed in today's `src/lib.rs` to its destination crate. Every module has exactly one destination. No module is "shared" — if shared semantics are needed, the module is split in a follow-up ticket and both halves are named here.
3. **Public API surface per crate.** For each member crate, a bullet list of the types/functions/macros that become `pub` at the crate root (or a named submodule). Anything not listed stays `pub(crate)`.
4. **Feature flags.** Whether `hyperforge-core` (or others) should expose optional features — e.g., `prelude` re-exporting common types from `hyperforge-types`, `git` gating libgit2-heavy paths, etc. Default features listed.
5. **Publish decisions.** For each crate, whether it is intended for eventual `crates.io` publication or workspace-internal only. HF-DC scope is workspace-internal only; this field records intent for later epics.
6. **Re-export strategy.** How `hyperforge-core::prelude` (or equivalent) surfaces common `hyperforge-types` symbols. What the bin crates import (typically just core + hubs, not types directly).
7. **Dependency matrix.** For each crate, which workspace crates and which external crates it depends on. Used by HF-DC-2..7 to populate `Cargo.toml` files.
8. **Workspace root layout.** Confirmation that `hyperforge/Cargo.toml` becomes `[workspace]`-only and member crates live at `hyperforge/crates/<name>/`. Alternative layouts (e.g., member crates at the repo root) rejected with rationale.
9. **Open questions resolved.** Any ambiguity the spike discovers must be closed before implementation tickets start. Deferrable questions (things that can wait until HF-TT or HF-IR) are marked deferred with an explicit owning epic.

## Risks

| Risk | Mitigation |
|---|---|
| A module contains both domain types and business logic (e.g., a type struct with a `fn new_from_env()` that reads the filesystem). | Spike identifies such modules and records the split plan: type stays in `hyperforge-types`, the constructor moves to `hyperforge-core`. The spike does not perform the split — that's an implementation ticket's work. |
| A type re-exported under multiple paths creates ambiguity about ownership. | Spike enumerates every public re-export site and assigns a single canonical path per type. |
| The `adapters/registry` hierarchy may not cleanly fit either `types` or `core`. | Spike's module-to-crate table includes adapter subtrees at leaf granularity where needed. |
| Feature-gated code (if any exists post-HF-0) crosses crate boundaries. | Spike records feature flag plan per crate; no feature spans crates. |

## What must NOT change

- Hyperforge's runtime behavior. This spike writes a document; it does not touch `src/`.
- Version numbers of the existing `hyperforge` crate. Version bumps happen in the implementation tickets.
- Any other sub-epic's planning directory.

## Acceptance criteria

1. A decision document exists at `hyperforge/docs/architecture/<timestamp>_hf-dc-crate-split.md` with all 9 sections from "Required behavior" populated.
2. The module-to-crate table covers every module currently listed in `src/lib.rs` (21 modules per HF-0 survey). No module is missing; no module has two destinations.
3. Each member crate listed in the document has: a name, an intended semver version (starting at `0.1.0` for new crates), a dependency list, and a public-API surface bullet list.
4. Open questions that would block any HF-DC-N ticket are resolved in the document. Open questions that defer to HF-TT / HF-IR / HF-CTX are explicitly labeled deferred and assigned to their owning epic.
5. The decision document is committed to the hyperforge repo on a spike branch (not merged until HF-DC-1 promotion gate).
6. `cargo build --workspace` and `cargo test --workspace` still pass in hyperforge (spike touched docs only, so gate is trivially green).

## Completion

Deliverable: the architecture document referenced above, plus a short comment on HF-DC-1 summarizing the ratified split (in the "Cross-epic contracts pinned" section of HF-DC-1) so HF-DC-1 reflects the final decisions. Flip this ticket's status to Complete once the document is committed and HF-DC-1 is updated.
