---
id: HF-1
title: "HF meta-epic — hyperforge: unbreak, decouple, tighten, adopt interface reform, absorb context store"
status: Epic
type: epic
blocked_by: []
unlocks: [HF-0, HF-DC-1, HF-TT-1, HF-IR-1, HF-CTX-1]
target_repo: hyperforge
---

## Goal

End state: hyperforge is the workspace's context-store and knowledge-graph substrate. Its library core exports typed, curated public APIs for the domain concepts it already knows about (packages, artifacts, versions, commits, workspaces, repos, ecosystems). Downstream consumers — TM-as-was, future activations, CLI tools — depend on `hyperforge-core` (or equivalent) without pulling CLI/IO surface. Ticketing, fact logging, knowledge-graph queries, and recursive zoom over the history of work all live inside hyperforge because the primitives already live there.

Concretely, at the end of the meta-epic:

- `cargo build` and `cargo test` are green workspace-wide (HF-0 gate).
- Hyperforge's core is factored into library crates with curated public APIs; the CLI is a thin adapter (HF-DC).
- Domain newtypes (`PackageName`, `Ecosystem`, `ArtifactId`, `Version`, `CommitRef`, `RepoRef`, `WorkspaceRoot`, `RepoPath`, etc.) replace raw strings at API boundaries (HF-TT).
- Hyperforge's activation surface adopts CHILD + IR primitives: artifacts/packages as `#[child(list = ...)]` dynamic child gates, `DeprecationInfo` on phased-out methods, `MethodRole::DynamicChild` where appropriate (HF-IR).
- A context-store / tickets layer built on hyperforge's artifact/version/commit primitives emits a typed, append-only fact log; supports knowledge-graph queries; supports recursive zoom across tickets → epics → meta-epics → cross-epic programs (HF-CTX). This absorbs what was sketched as the TM epic.

## Why HF instead of a fresh TM activation

The substrate-local TM drafts (`plans/TM/`) treated "ticket management" as a new concern in a new module. But hyperforge already encodes Package, Version, Workspace, Repo, Commit — the exact primitives a ticket's fact bundle needs to reference. Building TM in substrate would duplicate those concepts. Folding TM into hyperforge after decouple + type-tighten means the fact log consumes hyperforge's typed primitives directly, and hyperforge gains the ticket/context surface natively.

The existing `plans/TM/` drafts are retained as frozen inspiration for HF-CTX's ticket bodies but marked `Superseded` with `superseded_by: HF-CTX-*` pointers once HF-CTX's concrete implementation tickets are pinned.

## Phase structure

Each phase is its own sub-epic under `plans/HF/<phase>/`. Tickets are namespaced by phase: `HF-DC-N`, `HF-TT-N`, `HF-IR-N`, `HF-CTX-N`. Sub-epic overviews are always `<phase>-1.md`.

| Phase | Subdir | Overview | Purpose |
|---|---|---|---|
| HF-0 | (top-level single ticket) | `plans/HF/HF-0.md` | Unbreak the build. Single ticket; spike if diagnosis is nontrivial. |
| HF-DC | `plans/HF/HF-DC/` | `HF-DC-1.md` | Librification + decoupling: core into library crates with curated public APIs, CLI as thin adapter. |
| HF-TT | `plans/HF/HF-TT/` | `HF-TT-1.md` | Type tightening: newtypes for all domain concepts. Hyperforge owns these types outright. |
| HF-IR | `plans/HF/HF-IR/` | `HF-IR-1.md` | Adopt CHILD + IR primitives on hyperforge's activation surface. Distinct from substrate's IR epic — this is hyperforge's own IR adoption. |
| HF-CTX | `plans/HF/HF-CTX/` | `HF-CTX-1.md` | Context store / ticket fact log / knowledge graph / recursive zoom. Absorbs TM. |

## Dependency DAG

```
                   HF-0 (unbreak)
                        │
                        ▼
                   HF-DC (decouple)
                        │
                        ▼
                   HF-TT (type tighten)
                        │
                        ▼
                   HF-IR (interface reform)
                        │
                        ▼
                   HF-CTX (context store)
```

Phases are serial. Parallelism exists inside each sub-epic (see each sub-epic's DAG). Rationale for strict serial phase ordering:

- HF-DC must finish before HF-TT because newtypes get introduced at library-API boundaries that don't exist until the library surface is pinned.
- HF-TT must finish before HF-IR because `#[child(list = "...")]` + MethodRole primitives want strongly-typed identifiers as their arguments; retrofitting both at once risks whack-a-mole.
- HF-IR must finish before HF-CTX because the context store is an activation built on top of hyperforge's IR-reformed surface; it consumes the reformed types and child gates.

Within each phase, tickets fan out — see per-sub-epic overviews.

## Ownership & type ownership

Hyperforge owns all domain types it introduces: `PackageName`, `Ecosystem`, `ArtifactId`, `Version`, `CommitRef`, `RepoRef`, `WorkspaceRoot`, `RepoPath`, `Fact`, `Scope`, `TicketEvent`, `TicketId`, and any others that fall out of HF-TT / HF-CTX drafting. Downstream consumers (substrate activations, synapse, other tools) consume these as-is — they do not re-newtype or shadow them.

This resolves the earlier open correction from the substrate-local TM drafts, which incorrectly sourced `TicketId` from a sibling epic. Under HF ownership, hyperforge is the single source.

## Hyperforge as one of the first beneficiaries of CHILD + IR

The plexus-substrate CHILD and IR epics shipped primitives — `#[plexus_macros::child]`, `MethodRole::DynamicChild { list_method, search_method }`, `DeprecationInfo`, param-level deprecation, tagged versions — that hyperforge benefits from natively given its existing abstraction set. HF-IR makes those primitives load-bearing on hyperforge's surface. Example mappings:

- Artifacts under a workspace become `#[child(list = "artifact_ids")]` dynamic children.
- Packages under a repo become `#[child(list = "package_names")]` dynamic children.
- Deprecated hyperforge methods carry `DeprecationInfo` pointing at their replacements.
- Per-package subsurfaces (build, test, publish) become child activations in the reformed API.

## Cross-epic contracts pinned here

- Hyperforge location: confirmed at `/Users/shmendez/dev/controlflow/hypermemetic/hyperforge/` once HF-0's diagnosis report lands; adjust here if wrong.
- Hyperforge's library crate name TBD in HF-DC-1; candidates: `hyperforge-core`, `hyperforge-lib`, `hyperforge`. Decision lands in HF-DC-S01 (spike).
- Hyperforge's activation namespace today: `hf` (confirm in HF-0's survey — if different, pin here).
- Newtype location: all domain newtypes live in a single types crate (`hyperforge-types` or equivalent) consumed by both the library core and the context-store layer. Decision in HF-TT-1.

## Out of scope

- Moving hyperforge to a different repo or publishing to crates.io.
- Rewriting the CLI's argument grammar beyond what's necessary for the library split.
- Cross-workspace knowledge graph (connecting to workspaces beyond `~/dev/controlflow/hypermemetic/`) — single-workspace scope only.
- Multi-user / multi-tenant ticket visibility.
- Replacing git as the underlying VCS abstraction.
- Rich editing UI — hyperforge ships as RPC + CLI.
- Historical backfill of facts for work completed before HF-CTX ships — the ticket-to-facts emission is live-only; pre-HF-CTX work is imported from existing `plans/<EPIC>/*.md` frontmatter (HF-CTX importer ticket).

## What must NOT change

- Existing plexus-substrate, plexus-core, plexus-macros, synapse behaviors. HF-0 and later phases are additive (from the downstream perspective); upstream version pins inside hyperforge bump to align with the workspace.
- `plans/<EPIC>/*.md` files remain readable and editable by humans and agents. HF-CTX imports them; post-import, DB is source of truth, filesystem is export mirror.
- Existing ticket statuses across every other epic. HF-0 touches hyperforge only; HF-DC / HF-TT / HF-IR touch hyperforge internals only; HF-CTX adds a layer on top.

## Completion

Meta-epic is Complete when:

- HF-0 is Complete: workspace-wide `cargo build` + `cargo test` green in hyperforge and every repo that depends on it.
- HF-DC is Complete: library crate split shipped, CLI is a thin adapter, downstream consumers can import `hyperforge-core` (or equivalent name) without CLI/IO.
- HF-TT is Complete: all domain newtypes introduced at API boundaries, no raw `String` ids at public surfaces.
- HF-IR is Complete: hyperforge's activation surface uses `#[child(list = ...)]`, `MethodRole::DynamicChild`, `DeprecationInfo` where appropriate.
- HF-CTX is Complete: fact log is live; knowledge-graph queries answer the zoom + compatibility query shapes called out in the epic body; `plans/TM/` is marked Superseded.

Final demo in HF-CTX-N (the integration ticket): `synapse hf tickets ready` returns the current Ready set across all epics, `synapse hf facts compat-broken plexus-core plexus-macros` returns the ticket at which the two became incompatible, `synapse hf zoom HF --depth 3` returns a recursive aggregation across HF's tree.
