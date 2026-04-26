---
id: HF-CTX-S01
title: "Spike: fact taxonomy + scope frontmatter schema"
status: Pending
type: spike
blocked_by: [HF-IR-1]
unlocks: [HF-CTX-S02, HF-CTX-2]
severity: High
target_repo: hyperforge
---

## Question

What is the complete, pinned shape of the `Fact` enum, the `TicketScope` struct, and the mapping from the existing `HyperforgeEvent` variants onto `Fact`s — and which crate owns these types?

Binary pass: a decision document that enumerates (a) every `Fact` variant and its payload fields, (b) the `TicketScope` field set and types, (c) a 1:1 (or 1:many / many:1 / none) mapping from each `HyperforgeEvent` variant to zero-or-more `Fact` variants, (d) the "new" facts that have no `HyperforgeEvent` counterpart, (e) which crate (`hyperforge-types` vs a new `hyperforge-ctx-types`) owns them, (f) the activation namespace (`hyperforge.ctx` vs an alternative).

## Context

Upstream inputs:

- **HF-IR-1 complete.** Hyperforge's activation surface has adopted CHILD + IR primitives. `DeprecationInfo`, `MethodRole::DynamicChild`, `#[child(list = "...")]` are all in place on hyperforge's surface. Fact types can reference the IR-reformed shape.
- **HF-TT newtypes.** `PackageName`, `Ecosystem`, `ArtifactId`, `Version`, `CommitRef`, `RepoName`, `WorkspaceRoot`, `RepoPath`, `TagRef` all exist in hyperforge's types crate. Fact payloads reference these newtypes — never raw `String` where a newtype fits.
- **Existing `HyperforgeEvent` enum.** ~50+ variants already live at `src/hub.rs` in hyperforge. Starter set at `HF-CTX-1.md`'s "Fact taxonomy" table is the first cut; this spike fills in the rest and pins the mapping.

Seed Fact variants (from HF-CTX-1 — ratify, extend, or adjust here):

`TicketCreated`, `TicketStatusChanged`, `TicketLanded`, `ArtifactIntroduced`, `ArtifactRemoved`, `ArtifactRenamed`, `ArtifactDeprecated`, `VersionPublished`, `VersionBumped`, `VersionPinChanged`, `CompatibilityBroken`, `CompatibilityRestored`, `SchemaChanged`, `MigrationApplied`, `ConfigChanged`, `DocAuthored`, `DecisionRecorded`, `ResearchConcluded`, `TouchedPath`, `Tagged`, `DependsOn`.

Seed `TicketScope` fields (from HF-CTX-1):

`repos`, `packages`, `ecosystems`, `starts_from`, `ends_at`, `versions_before`, `versions_after`, `introduces`, `deprecates`, `removes`, `touches`, `tags_created`.

Two type-ownership candidates:

**Option A — `hyperforge-types`.** Facts live alongside `PackageName`, `ArtifactId`, etc. Advantage: single types crate for the whole workspace; downstream tools already depend on it. Disadvantage: `hyperforge-types` pulls in fact-log concerns that existed independently of the newtypes.

**Option B — `hyperforge-ctx-types`.** New crate, depends on `hyperforge-types`. Advantage: fact log is an optional concern, not every consumer needs it. Disadvantage: yet another crate; downstream fact consumers depend on both.

## Setup

1. Enumerate every `HyperforgeEvent` variant in `src/hub.rs`. For each variant, decide:
   - Maps 1:1 to an existing seed `Fact` variant.
   - Maps to multiple seed `Fact`s (compound event).
   - Needs a new `Fact` variant introduced by this spike.
   - Is purely runtime / in-memory (e.g., `Info`, `Error`, `Status`) and maps to **no** `Fact` (stays runtime-only).

   Produce the mapping table.

2. Enumerate the "new" facts — ones that have no `HyperforgeEvent` counterpart. For each, pin:
   - Payload fields with types (referencing HF-TT newtypes).
   - Emission trigger (which method call, which lifecycle point).
   - Which hub (`BuildHub`, `RepoHub`, `ReleasesHub`, `ImagesHub`, `AuthHub`, or a new `CtxHub`) is responsible for emitting it.

3. For the `TicketScope` struct, confirm the HF-CTX-1 field set. If any field is redundant or missing, note it with rationale. Pin exact types for each field (e.g., `starts_from: HashMap<RepoName, CommitRef>` vs `Vec<(RepoName, CommitRef)>`).

4. Decide type ownership (A vs B above). Justify in one paragraph against:
   - Does it avoid adding a new crate to the dependency graph of consumers that don't care about facts?
   - Does it keep `hyperforge-types`'s surface coherent around "domain primitives" vs branching into "fact log records"?
   - Does it let the fact log evolve in patch bumps without forcing `hyperforge-types` itself to churn?

5. Pin the activation namespace. Candidates: `hyperforge.ctx`, `hyperforge.context`, `hyperforge.tm`, `hyperforge.tickets`. The namespace appears in every synapse CLI invocation and is load-bearing.

6. Pin the required derives on every `Fact` variant: `Debug, Clone, Serialize, Deserialize, JsonSchema`. Confirm `#[non_exhaustive]` on the top-level `Fact` enum. Confirm every fact carries `valid_at: i64`, `source_commit: Option<CommitRef>`, `source_ticket: Option<TicketId>` at the outer record level (not inside each variant's payload).

## Pass condition

A decision document under `plans/HF/HF-CTX/S01-report.md` (or inlined in HF-CTX-1's Context section) contains:

1. Complete `Fact` variant list with exact payload types.
2. Complete `TicketScope` struct with exact field types.
3. Complete `HyperforgeEvent → Fact` mapping table, marking runtime-only variants explicitly.
4. Ownership decision (types crate) with justification.
5. Activation namespace decision with justification.
6. Derive set and `#[non_exhaustive]` placement confirmed.

Binary: all six pinned → PASS. Any left open → FAIL.

## Fail → next

If the taxonomy enumeration reveals > ~35 total `Fact` variants, or the `HyperforgeEvent` mapping has > ~8 ambiguous rows, stop and write HF-CTX-S01b to split the taxonomy into domains (e.g., `BuildFact`, `RepoFact`, `TicketFact`) before HF-CTX-2 is promoted. Taxonomy size alone is not a blocker; ambiguity about where a variant belongs is.

## Fail → fallback

If type ownership can't be pinned cleanly, default to **Option A** (`hyperforge-types` owns facts). Rationale: downstream consumers already depend on `hyperforge-types`; adding facts to a second crate doubles the dependency surface without a clear win. Revisit if the fact log grows too large in later epics.

## Time budget

Four focused hours for enumeration + mapping + ownership decision. If the spike exceeds this, stop and report regardless of pass/fail state.

## Out of scope

- Implementing any fact emission (HF-CTX-4's job).
- Writing SQL schema for fact storage (HF-CTX-2's job).
- Persisting `HyperforgeEvent` as facts retroactively — this spike pins forward-looking emission only.
- The query algebra — HF-CTX-S02 owns that.
- Deciding whether `HyperforgeEvent` itself is deprecated. (It isn't, per HF-CTX-1: events are runtime, facts are durable.)

## Completion

Spike delivers:

1. The decision document with all six items pinned.
2. The fact-variant list becomes the authoritative input to HF-CTX-2.
3. The `HyperforgeEvent → Fact` mapping becomes the authoritative input to HF-CTX-4.
4. Pass/fail result, time spent, one-paragraph summary.

Report lands in HF-CTX-1's Context section as a reference before HF-CTX-2 is promoted to Ready.
