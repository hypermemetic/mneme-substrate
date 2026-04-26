---
id: HF-CTX-1
title: "HF-CTX sub-epic — context store, fact log, knowledge-graph queries, recursive zoom"
status: Epic
type: epic
blocked_by: [HF-IR-1]
unlocks: []
target_repo: hyperforge
---

## Goal

End state: hyperforge is the workspace's context store and knowledge-graph substrate. Every ticket that lands anywhere in the workspace emits a typed, append-only bundle of **facts** — `ArtifactIntroduced`, `CrateBumped`, `TypeRemoved`, `Tagged`, `TouchedPath`, `DocAuthored`, `DecisionRecorded`, `ConfigChanged`, etc. — into hyperforge's fact log. Queries over that log answer:

1. **Temporal compatibility:** "What was the last ticket that completed before `plexus-core` 0.5.0 and `plexus-macros` 0.4.0 became incompatible?"
2. **Scoped audit:** "Show me all work completed on `cone/storage.rs` since the IR epic started."
3. **Recursive zoom:** "Zoom out on the IR epic — show aggregate facts across every ticket, then roll up into the cross-epic program."
4. **Forward impact:** "This ticket deprecates `ChildCapabilities`. Which other tickets currently reference it?"

HF-CTX absorbs what was sketched as the substrate-local TM epic (`plans/TM/`). The existing TM drafts are marked `Superseded` with `superseded_by` pointers into HF-CTX tickets once concrete implementation tickets are pinned.

## Context

HF-CTX sits on top of:

- **HF-TT newtypes:** `PackageName`, `Ecosystem`, `ArtifactId`, `Version`, `CommitRef`, `RepoName`, `WorkspaceRoot`, `RepoPath`.
- **HF-IR primitives:** `#[child(list = "...")]` gates, `MethodRole::DynamicChild`, `DeprecationInfo`.
- **hyperforge's existing event taxonomy:** the 50+ `HyperforgeEvent` variants are a starting point for the fact schema. HF-CTX-S01 maps them onto the new `Fact` enum and fills gaps.

HF-CTX does NOT rebuild:

- Package/Version/Commit/Repo abstractions — HF-TT owns those.
- Activation registration, hub discovery — HF-DC and HF-IR own those.
- Git state tracking (`RepoStatus`, `SyncDiff`) — hyperforge already has it.

## Proposed surface

### Fact taxonomy (ratified in HF-CTX-S01)

Append-only events, each carrying typed payloads. Non-exhaustive starter set:

| Fact | Payload | Emitted when |
|---|---|---|
| `TicketCreated` | `{ticket_id, title, epic, scope}` | `tm.create_ticket` or FS import. |
| `TicketStatusChanged` | `{ticket_id, from, to, at_commit}` | Status flip. |
| `TicketLanded` | `{ticket_id, commits: {repo: CommitRef}}` | Status → Complete. |
| `ArtifactIntroduced` | `{artifact_id, kind, at_commit, at_ticket}` | New pub type / module / file / schema. |
| `ArtifactRemoved` | `{artifact_id, at_commit, at_ticket}` | Deletion. |
| `ArtifactRenamed` | `{from_id, to_id, at_commit, at_ticket}` | Identity-preserving rename. |
| `ArtifactDeprecated` | `{artifact_id, since_version, removed_in, at_ticket}` | `#[deprecated]` added. |
| `VersionPublished` | `{package: PackageName, ecosystem, version, at_commit, at_tag}` | Crate/cabal/npm/etc. publish. |
| `VersionBumped` | `{package, from, to, at_commit, at_ticket}` | Cargo.toml / cabal / package.json bump. |
| `VersionPinChanged` | `{consumer, dep, from_range, to_range, at_commit}` | Dep declaration change. |
| `CompatibilityBroken` | `{consumer, dep, between_versions, observed_at_ticket}` | Detected/reported incompat. |
| `CompatibilityRestored` | `{consumer, dep, at_version, observed_at_ticket}` | Re-stitched. |
| `SchemaChanged` | `{schema_id, change_kind, at_commit}` | SQL / proto / JSON-schema shift. |
| `MigrationApplied` | `{migration_id, scope, at_commit}` | DB or config migration shipped. |
| `ConfigChanged` | `{key, from, to, env, at_commit}` | Env var / feature flag / config knob change. |
| `DocAuthored` | `{doc_id, kind: ADR/Runbook/Architecture, at_commit}` | New design doc. |
| `DecisionRecorded` | `{decision_id, summary, at_ticket}` | ADR-style choice logged. |
| `ResearchConcluded` | `{spike_id, outcome: Pass/Fail/Inconclusive, at_ticket}` | Spike reached a verdict. |
| `TouchedPath` | `{ticket_id, path: RepoPath, change_kind}` | Any file modification. |
| `Tagged` | `{tag: TagRef, commit: CommitRef, package: PackageName}` | Git tag created. |
| `DependsOn` | `{ticket_a, ticket_b}` | `blocked_by` established. |

All facts derive `Debug, Clone, Serialize, Deserialize, JsonSchema`, carry a `valid_at: i64` (unix seconds), a `source_commit: Option<CommitRef>`, and a `source_ticket: Option<TicketId>`.

### Ticket scope frontmatter (extends the existing YAML header)

| Field | Type | Meaning |
|---|---|---|
| `scope.repos` | `Vec<RepoName>` | Repos touched. |
| `scope.packages` | `Vec<{ecosystem, package}>` | Packages touched. |
| `scope.ecosystems` | `Vec<Ecosystem>` | Union over packages. |
| `scope.starts_from` | `HashMap<RepoName, CommitRef>` | Baseline per repo. |
| `scope.ends_at` | `HashMap<RepoName, CommitRef>` | Post-ticket. |
| `scope.versions_before` | `HashMap<PackageName, Version>` | Before state. |
| `scope.versions_after` | `HashMap<PackageName, Version>` | After state. |
| `scope.introduces` | `Vec<ArtifactId>` | Qualified ids added. |
| `scope.deprecates` | `Vec<ArtifactId>` | Marked deprecated. |
| `scope.removes` | `Vec<ArtifactId>` | Deleted. |
| `scope.touches` | `Vec<ArtifactId>` | Modified but not created/removed. |
| `scope.tags_created` | `Vec<TagRef>` | Tags. |

### Knowledge-graph query surface

| Query | Signature | Answers |
|---|---|---|
| `compat_broken` | `(Consumer, Dep) -> Option<Ticket>` | Last ticket completed before consumer/dep incompatibility. |
| `work_on` | `(ArtifactId) -> Vec<Ticket>` | All tickets that touched/introduced/deprecated/removed the artifact. |
| `blast_radius` | `(TicketId, depth: u32) -> Graph` | Transitive consumers/producers, bounded depth. |
| `zoom` | `(EpicPrefix, depth: u32) -> ZoomedView` | Recursive aggregate: fact counts, distinct repos, distinct symbols, time span, child-epic summaries. |
| `currently_deprecated` | `() -> Vec<Artifact>` | All artifacts with open `Deprecated` fact and no `Removed` fact. |
| `incompat_observers` | `(Package, Version) -> Vec<Ticket>` | Who flagged this version as incompatible. |
| `who_touches` | `(RepoPath) -> Vec<Ticket>` | Tickets that touched the file. |

### Recursive zoom

Zoom is defined at arbitrary depth:

- **Depth 0:** single ticket's fact bundle.
- **Depth 1:** aggregate over all tickets in the ticket's immediate epic.
- **Depth 2:** aggregate over the epic's parent meta-epic (e.g., HF-1 is the parent of HF-DC-1, HF-TT-1, etc.).
- **Depth N:** recursive rollup all the way to workspace-wide.

Aggregation primitives: fact counts per kind, distinct packages touched, distinct ecosystems touched, time span start/end, list of child epics with their summaries.

## Dependency DAG

```
               HF-CTX-S01 (fact taxonomy + scope schema)
                      │
                      ▼
               HF-CTX-S02 (query surface + zoom algebra)
                      │
                      ▼
               HF-CTX-2 (fact types + storage + trait)
                      │
           ┌──────────┼──────────┬──────────┐
           ▼          ▼          ▼          ▼
        HF-CTX-3   HF-CTX-4   HF-CTX-5   HF-CTX-6
       (ticket    (fact-       (query-    (watch/
        CRUD +    emission     side       stream)
        scope    hooks in      methods:
        parse)   other hubs)   compat,
                                work_on,
                                etc.)
           │          │          │          │
           └──────────┴──────┬───┴──────────┘
                             ▼
                       HF-CTX-7 (recursive zoom)
                             │
                             ▼
                       HF-CTX-8 (human promotion gate)
                             │
                             ▼
                       HF-CTX-9 (filesystem exporter DB → plans/*.md)
                             │
                             ▼
                       HF-CTX-10 (one-shot importer plans/*.md → DB + facts)
                             │
                             ▼
                       HF-CTX-11 (supersede plans/TM/)
```

## Phase breakdown

| Phase | Tickets | Notes |
|---|---|---|
| 0. Spikes | HF-CTX-S01, HF-CTX-S02 | Pin taxonomy, scope schema, query algebra. Both binary-pass. |
| 1. Foundation | HF-CTX-2 | `Fact`, `TicketScope`, `TicketStore` trait, SQLite default backend. |
| 2. Parallel surface | HF-CTX-3..6 | CRUD, fact-emission hooks, query methods, watch. |
| 3. Zoom | HF-CTX-7 | Recursive aggregate. |
| 4. Promote gate | HF-CTX-8 | Auth-gated Pending → Ready (matches original TM-6). |
| 5. Filesystem mirror | HF-CTX-9, HF-CTX-10 | Export and one-shot import of existing `plans/`. |
| 6. TM supersession | HF-CTX-11 | Mark every `plans/TM/*.md` file `Superseded` with pointers into HF-CTX. |

## Cross-epic contracts pinned

- **Write-once fact table with frontmatter reconciler.** Facts are stored in the DB as the source of truth. Ticket frontmatter is a human-readable export mirror (like the body). When both exist, the DB wins; a reconciler warns on disagreement. This resolves the earlier "write-once vs derive" question in favor of write-once, given that non-code facts (decisions, research outcomes, approvals) have no git-derivable source.
- **Facts belong to HF-CTX.** All `Fact`, `TicketScope`, `TicketId` types live in the HF-CTX layer (or alongside the newtypes in `hyperforge-types`, pinned by HF-CTX-S01). Downstream tools consume as-is.
- **Fact emission is pluggable.** Other hubs (`BuildHub`, `RepoHub`, `ImagesHub`, `ReleasesHub`) emit facts via a `FactSink` trait passed during construction. Keeps the context layer inverted — HF-CTX pulls, not pushes.
- **Activation namespace:** HF-CTX methods live on `hyperforge.ctx` (or similar subpath — pinned by HF-CTX-S01). `synapse hyperforge ctx tickets ready`.

## What must NOT change

- Existing `plans/<EPIC>/*.md` files remain readable and editable throughout the epic. The importer (HF-CTX-10) is idempotent; the exporter (HF-CTX-9) is one-way and rewrites deterministically. Human edits that agree with DB state are no-ops. Edits that disagree are overwritten — this is the "DB is source of truth" policy, not a regression.
- Every hyperforge method existing post-HF-IR continues to work. Fact emission is additive plumbing.
- Activation trees under `hyperforge.workspace.*`, `hyperforge.repo.*`, etc. are not restructured by HF-CTX — it adds a sibling `ctx` subtree.
- Synapse behavior for existing methods is unchanged.

## Risks

| Risk | Mitigation |
|---|---|
| Fact schema misses a concept that turns out to be essential. | `Fact` enum is non-exhaustive (`#[non_exhaustive]`). Add variants in patch bumps. |
| Performance: writing a fact per touched path per ticket could balloon the DB. | Index on `(valid_at, ticket_id)` + `(artifact_id)` + `(package, ecosystem)`. Pagination on every query. `TouchedPath` optional per-ticket (ticket author decides). |
| Reconciler warnings become noise. | Warning is only emitted on hard disagreement (ticket says "introduces X" but DB has no `ArtifactIntroduced(X)` for this ticket). Soft differences (ticket lacks `scope.touches` that DB has) are silent. |
| `plans/` import misses facts that were never in frontmatter. | Import seeds `TicketCreated`, `TicketStatusChanged`, `TicketLanded`, `TouchedPath` from git log per repo. Richer facts (`ArtifactIntroduced` etc.) come from new tickets going forward. |
| Zoom query is O(n × depth) across the fact log. | Precompute per-epic rollups on ticket landing (incremental aggregation). Zoom reads rollups, not raw facts. |

## Out of scope

- Cross-workspace fact log (multi-workspace deferred).
- Rich editing UI.
- LLM-based summarization of facts (future epic).
- Replacing `HyperforgeEvent` — `HyperforgeEvent` is the in-memory runtime event stream; `Fact` is the durable historical record. Both coexist; events can be persisted as facts (HF-CTX-S01 pins the mapping).

## Completion

Sub-epic is Complete when:

- HF-CTX-S01 through HF-CTX-11 are all Complete.
- The HF-1 meta-epic's closing demo runs: `synapse hyperforge ctx tickets ready`, `synapse hyperforge ctx facts compat-broken plexus-core plexus-macros`, `synapse hyperforge ctx zoom HF --depth 3` all return non-trivial, correct output.
- Every `plans/TM/*.md` file has `status: Superseded` and `superseded_by: HF-CTX-*` in its frontmatter.
- `plans/HF/` and every existing epic's tickets are importable and round-trippable through the DB.
- `cargo build --workspace` and `cargo test --workspace` green.
- Hyperforge version reflects the surface addition (minor bump); tag local.
