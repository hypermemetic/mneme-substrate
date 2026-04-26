---
id: TM-1
title: "TM epic — ticketing management activation"
status: Epic
type: epic
blocked_by: []
unlocks: [TM-S01, TM-S02, TM-2, TM-3, TM-4, TM-5, TM-6, TM-7, TM-8, TM-9]
target_repo: plexus-substrate
---

## Goal

End state: a Linear-style ticket manager ships inside substrate as a Plexus RPC activation under the namespace `tm`. Humans, machine agents, and Orcha all interact with tickets via Plexus RPC calls against `tm`, not by reading or editing files under `plans/`. The filesystem becomes a one-way read-only export of the database for git visibility. The current `plans/<EPIC>/*.md` tree is imported once into TM storage and then treated as a derived artifact.

Concretely, after this epic lands:

- `synapse tm ready` returns all `Ready` tickets across epics.
- `synapse tm promote TID` performs the human-gated `Pending → Ready` transition. Machine agents cannot call it successfully.
- `synapse tm watch` streams `TicketEvent` for every status change.
- Orcha pulls ready tickets from TM via the library API (per DC conventions) instead of reading `plans/` files, and writes status transitions back as it starts and completes graphs.
- A one-shot import reads the existing `plans/<EPIC>/*.md` layout into `TicketStore`, idempotent, without deleting the source files.
- After import, whenever TM mutates a ticket, the corresponding `plans/<EPIC>/*.md` file is rewritten from the DB as an export mirror.

## Namespace name pinning

**Chosen namespace: `tm`.**

Candidates considered: `tm`, `thread`, `tix`, `weave`, `plan`.

| Candidate | Verdict |
|---|---|
| `tm` | **Chosen.** Matches the epic prefix, grep-friendly, unambiguous, two-character CLI prefix (`synapse tm ready`). Short and neutral — no metaphor to defend. |
| `thread` | Rejected. Overloaded with OS threads, message threads, and the project-wide "threading" metaphor around streams. Collides at the terminology level. |
| `tix` | Rejected. Jokey. Low ecosystem fit with the rest of the substrate surface (`arbor`, `cone`, `solar`, `orcha` — serious nouns). |
| `weave` | Rejected. Evocative but doesn't obviously mean "ticket"; would require documentation just to introduce. Too abstract for the CLI. |
| `plan` | Rejected. Collides with the `plans/` directory we're replacing. Using `plan` as the RPC namespace while the filesystem export still sits under `plans/` is an obvious footgun. |

`tm` is pinned here. All TM-N tickets use `tm` in their `namespace = "..."` macro arg and in CLI examples. If the user wants to bikeshed this later, changing the namespace is a cheap one-line edit — but the epic ships with `tm`.

## Context

TM is a standalone Plexus RPC activation inside substrate. It owns:

- A `TicketStore` trait (pinned in `plans/README.md` under "Trait surfaces") and a default SQLite implementation following the per-activation storage pattern used by `OrchaStore`, `LatticeStore`, etc.
- A set of Plexus RPC methods (CRUD, queries, streaming) exposed under `namespace = "tm"`.
- A filesystem exporter that mirrors DB state to `plans/<EPIC>/*.md`.
- A one-shot importer that reads existing `plans/<EPIC>/*.md` files into `TicketStore`.

TM does **not** own:

- Graph execution. Orcha still owns graph runtime and compilation. TM owns the authoritative ticket lifecycle; Orcha reads from it.
- The domain newtype `TicketId`. That is owned by the ST epic (`TicketId(String)` — pinned in `plans/README.md`). TM consumes it as-is.

Adjacent activation already in substrate: `orcha/pm` (`src/activations/orcha/pm/`). `pm` tracks `graph_id → ticket_id → node_id` mappings and node event logs for Orcha's runtime — a very different concern from ticket authorship and lifecycle. Whether `pm` is absorbed by TM or coexists is gated on TM-S01.

Key design decisions pinned here (before any implementation ticket is promoted):

| Decision | Call |
|---|---|
| DB is source of truth. | Yes. Filesystem is a derived export for git visibility. |
| `TicketStore` trait shape. | Per-activation trait, like `OrchaStore`. Named `TicketStore`. |
| Default backend. | SQLite. `tm.db` under the substrate DB root, matching sibling activations (`arbor.db`, `loopback.db`, etc.). |
| Human gate for `Pending → Ready`. | Enforced at the RPC surface via the auth context. Only authenticated humans can call `promote`. Machine agents (Claude, Orcha) cannot. |
| Library API for Orcha. | Per DC conventions, TM exposes an in-process Rust API that Orcha consumes directly — no wire-level call for the hot path. RPC methods remain the surface for humans, CLIs, and remote integrations. |
| Filesystem export direction. | One-way, DB → files. TM never reads `plans/` after the one-shot import. Edits to `plans/` files are silently overwritten on the next export. |
| Importer deletion policy. | Importer does **not** delete source files. They become the exported mirror — the next export pass rewrites them from the DB. |
| Query surface (typed methods vs filter DSL). | Gated on TM-S02. |

## Dependency DAG

```
          TM-S01 (absorb vs coexist)
                 │
                 ▼
          TM-S02 (query surface shape)
                 │
                 ▼
               TM-2 (TicketStore trait + types)
                 │
      ┌──────────┼──────────┬──────────┐
      ▼          ▼          ▼          ▼
    TM-3       TM-4       TM-5       TM-6
   (CRUD)    (queries)  (watch)   (promote gate)
      │          │          │          │
      └──────────┴──────┬───┴──────────┘
                        ▼
              ┌─────────┼─────────┐
              ▼         ▼         ▼
            TM-7      TM-8      TM-9
           (orcha)  (export)  (importer)
```

Notes:

- TM-S01 and TM-S02 gate everything. Both are binary-pass spikes; both must complete before TM-2 is promoted.
- TM-3, TM-4, TM-5, TM-6 all consume TM-2's trait shape and can run in parallel. They touch disjoint method clusters on the activation (CRUD mutations, read-side queries, streaming, and auth-gated promotion respectively), so file-boundary concurrency holds if they are split into separate files per cluster.
- TM-7, TM-8, TM-9 all integrate TM into adjacent systems (Orcha, filesystem output, filesystem input). They can run in parallel with each other; each touches its own seam.

## Phase Breakdown

| Phase | Tickets | Notes |
|---|---|---|
| 0. Spikes | TM-S01, TM-S02 | Binary-pass. Both must complete before phase 1. |
| 1. Foundation | TM-2 | Single ticket; pins trait and types. Blocks all implementation tickets. |
| 2. RPC surface | TM-3, TM-4, TM-5, TM-6 | Parallel. CRUD, queries, streams, promotion gate. |
| 3. Integration | TM-7, TM-8, TM-9 | Parallel. Orcha integration, filesystem export, one-shot importer. |

## Tickets

| ID | Summary | Target repo | Status |
|---|---|---|---|
| TM-1 | This epic overview | — | Epic |
| TM-S01 | Spike: absorb vs coexist with `orcha/pm` | plexus-substrate | Pending |
| TM-S02 | Spike: typed query methods vs filter DSL | plexus-substrate | Pending |
| TM-2 | `TicketStore` trait + ticket types (`Ticket`, `Epic`, `Status`, `DeprecationInfo`) | plexus-substrate | Pending |
| TM-3 | CRUD RPC methods (create, get, update body, update status, delete) | plexus-substrate | Pending |
| TM-4 | Query methods (list, ready, blocked_on, unlocks_chain, epic_dag, epic_progress) | plexus-substrate | Pending |
| TM-5 | Watch / stream methods (watch_ticket, watch_epic, watch_all) | plexus-substrate | Pending |
| TM-6 | Human promotion gate — auth-gated `Pending → Ready` | plexus-substrate | Pending |
| TM-7 | Orcha integration via library API | plexus-substrate | Pending |
| TM-8 | Filesystem export (DB → `plans/<EPIC>/*.md`) | plexus-substrate | Pending |
| TM-9 | One-shot import of existing `plans/<EPIC>/*.md` | plexus-substrate | Pending |

## Out of scope

- **Multi-tenant ticket storage.** Single workspace, single DB. Per-user visibility is deferred.
- **Ticket attachments / file uploads.** Body text only.
- **Rich editing UI.** TM is an RPC surface + `synapse` CLI. No web UI in this epic.
- **Migration of non-substrate plans directories.** TM-9's importer targets this repo's `plans/` tree. Cross-repo import is a follow-up.
- **Deletion of `orcha/pm`.** That is an output of TM-S01, not an assumption. If S01 decides TM absorbs pm, a follow-up ticket (outside this epic) handles the deletion.
- **`TicketId` newtype introduction.** Owned by ST epic. TM uses whatever shape is current at the time TM-2 lands; if ST lands first, TM-2 uses `TicketId`; if ST is still in flight, TM-2 uses `String` with a pinned plan to migrate.
- **Realtime collaborative editing.** `update_body` is last-write-wins. No CRDTs.

## What must NOT change

- Existing `plans/<EPIC>/*.md` files remain readable and editable by humans and agents during and after TM ships. The importer is idempotent; the exporter is one-way and rewrites deterministically, so a human edit that agrees with the DB is a no-op. An edit that disagrees is silently overwritten — this is a feature of the "DB is source of truth" policy, not a regression.
- Every other substrate activation continues to compile and test green. TM is additive.
- `orcha/pm`'s wire surface is unchanged until TM-S01 decides otherwise.
- Synapse's method discovery and invocation behavior for all other activations is unchanged.

## Completion

Epic is Complete when TM-S01 through TM-9 are all Complete, and the following end-to-end demo is captured in TM-7's PR:

1. `synapse tm ready` returns the current set of Ready tickets.
2. A human runs `synapse tm promote TM-DEMO-1` and the ticket flips `Pending → Ready`.
3. Orcha picks up the newly-ready ticket from its library-API poll and starts a graph.
4. On graph completion, the ticket's status is `Complete` in TM and in its exported `plans/<EPIC>/<EPIC>-N.md` file.
5. `git diff plans/` shows a deterministic, human-readable change reflecting the status transition.
