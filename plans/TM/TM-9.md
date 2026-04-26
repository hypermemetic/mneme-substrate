---
id: TM-9
title: "TM one-shot import from existing plans/<EPIC>/*.md"
status: Pending
type: implementation
blocked_by: [TM-2, TM-3]
unlocks: []
severity: High
target_repo: plexus-substrate
---

## Problem

Substrate currently has dozens of tickets across ~20 epic directories under `plans/`. Before TM can be the source of truth, every one of those files must be ingested into `TicketStore` without data loss. This ticket ships a one-shot, idempotent importer that walks `plans/<EPIC>/*.md`, parses each frontmatter + body, and writes a `Ticket` (or `Epic`) record into TM. Source files are **not** deleted — they become the exported mirror owned by TM-8.

## Context

Target repo: `plexus-substrate`. Target file: `src/activations/tm/import.rs`.

Existing ticket files live in `plans/<EPIC>/<EPIC>-<N>.md` (plus `<EPIC>-S<NN>.md` for spikes). Current epics: `ARBOR`, `BIDIR`, `CHILD`, `CLAUDECODE`, `CLEANUP`, `DEVEX`, `DISCORD`, `DISPATCH`, `ERRORS`, `HANDLE`, `HASH`, `IR`, `LIVE-GRAPH`, `MCP`, `ORCHA`, `RUNPLAN`, `STREAM-UX`, `SYN`, `TDD`. (Plus this new `TM/` directory.)

Frontmatter format (per `skills/ticketing/SKILL.md`):

```yaml
id: EPIC-N
title: "..."
status: Pending | Ready | Blocked | Complete | Idea | Epic | Superseded
type: implementation | analysis | spike | epic
blocked_by: [A, B]
unlocks: [C, D]
severity: Critical | High | Medium | Low         # optional
target_repo: plexus-core                         # optional
superseded_by: TICKET-ID                         # optional
```

The body is everything after the closing `---`. The importer preserves it verbatim.

Idempotency: re-running the importer produces the same `TicketStore` state. Existing tickets in the store are updated if their file representation differs; absent tickets are inserted.

## Required behavior

### RPC method

| Method | Args | Return | Behavior |
|---|---|---|---|
| `import_from_disk` | `root: Option<String>` | `TmImportResult` | Walks `<root>/<EPIC>/*.md` for every subdirectory under `<root>`. Parses each file. Inserts or updates `Ticket` / `Epic` records. Returns `ok { epics_found, tickets_imported, tickets_updated, tickets_skipped, errors }`. |

`root` defaults to the workspace `plans/` directory.

### Parsing

1. For each `<root>/<EPIC>/` directory:
   - Treat `<EPIC>` as the epic prefix.
   - Enumerate files matching `<EPIC>-*.md`.
   - Parse each file's frontmatter block (between the first two `---` lines). Use a YAML parser.
   - The closing `---` delimiter and everything after becomes the `body`.
   - Construct a `Ticket` with `id`, `title`, `status`, `ticket_type`, `blocked_by`, `unlocks`, `severity`, `target_repo`, `superseded_by`, and `body`.
   - `created_at` is set to the file's mtime (or `now()` if mtime unavailable). `updated_at` is set the same.

2. Epic overview records (`<EPIC>-1.md` with `type: epic`):
   - Additionally construct an `Epic` record with `prefix = <EPIC>`, `title = ticket.title`, `goal` extracted from the body's `## Goal` section (greedy match until next `##` heading or end of file), `ticket_ids` populated by scanning the directory.

### Idempotency

For each parsed `Ticket`:

1. Look up `get_ticket(id)`.
2. If absent: `create_ticket(ticket)`.
3. If present and the stored ticket differs only in `updated_at`: skip.
4. If present and the stored ticket differs in any other field: `update_ticket_body` and `update_ticket_status` as needed.

The importer does not invoke TM-6's promote gate — it writes directly through `TicketStore` as a privileged bootstrap operation. This is the only code path in TM that bypasses the state-machine and auth restrictions, and it is only exposed via this RPC (which is itself gated to `Caller::Human` — see Risks).

### Auth gating

`import_from_disk` is auth-gated the same way `promote` is (TM-6): only `Caller::Human` may invoke. Reason: running the importer writes wholesale ticket state, and a misconfigured client running it after drift would clobber live work.

### Files not touched

- Non-`.md` files in `<root>/<EPIC>/` (e.g., `README.md`, diagrams, `.png` fixtures) are ignored.
- The `README.md` at `<root>/README.md` is ignored (it's the roadmap document, not a ticket).
- Subdirectories beyond one level are ignored — only `<root>/<EPIC>/<EPIC>-<N>.md`.
- Files that don't parse (missing frontmatter, malformed YAML, unknown `status` value) are recorded in the `errors` list in the result; they are skipped without halting the import.

### Error reporting

`TmImportResult::ok { ..., errors: Vec<ImportError> }` where `ImportError` is `{ path: String, reason: String }`. Empty `errors` → clean import. Any non-empty → partial success, inspect `errors` list.

## Risks

| Risk | Mitigation |
|---|---|
| A human edits a file between the importer's read and write, and their edit is lost. | Mitigated by running the importer as a one-shot operation, not a loop. Guidance: run the importer once at TM rollout, then never again except for explicit recovery. |
| Idempotency fails on ordering — a ticket references a `blocked_by` that isn't imported yet. | The store accepts the ticket; references are validated at query time, not write time. All tickets end up imported; cross-references resolve on any subsequent `blocked_on` call. |
| Malformed YAML in a subset of files halts the whole import. | Per-file error handling: log, add to `errors` list, continue. Acceptance criterion 2 pins this. |
| A ticket's frontmatter declares an `id` that doesn't match its filename. | Importer warns (adds to `errors`) and uses the frontmatter's `id` as authoritative. Filename mismatch is a pre-existing data-quality issue; the importer surfaces it but doesn't "fix" names. |
| Importer overwrites TM's own plans (recursion). | The TM epic directory is imported too — expected. The importer treats `TM-*.md` the same as any other epic. Idempotency guarantees the end state matches the file state. |

## What must NOT change

- Every existing `plans/<EPIC>/*.md` file remains on disk after import. No deletions, no moves.
- Non-ticket files (`plans/README.md`, any `docs/`, `*.png`, etc.) are not touched.
- TM-2 trait surface. This ticket consumes `create_ticket`, `update_ticket_body`, `update_ticket_status`, `create_epic`, `list_tickets` — all existing.
- TM-6's promote gate for every path except this importer's explicit bypass. The bypass is documented and auth-gated.
- Every other substrate activation's behavior.

## Acceptance criteria

1. `cargo build -p plexus-substrate` succeeds.
2. `cargo test -p plexus-substrate` succeeds. Tests cover:

   | Scenario | Expected |
   |---|---|
   | Import a fixture `plans/` with 3 epics × 5 tickets (15 total) | `ok { tickets_imported: 15, ... }`; `list_tickets` returns 15 entries. |
   | Run the importer twice on the same fixture | Second run returns `tickets_skipped: 15, tickets_imported: 0, tickets_updated: 0`. |
   | Run the importer after a ticket's file body has changed | Second run returns `tickets_updated: 1`; `get_ticket` reflects the new body. |
   | Import a fixture containing a malformed YAML file | `ok { errors: [{path, reason}], ... }`; other files imported successfully. |
   | Import a fixture containing non-.md files | Non-.md files ignored; no errors. |
   | Import a fixture where frontmatter `id` disagrees with filename | Entry added to `errors`; ticket imported using frontmatter `id`. |
   | `import_from_disk` called from `Caller::Agent` | Returns `not_authorized`. |
   | `import_from_disk` called from `Caller::Human` on the real `plans/` tree | Completes within 30 seconds; populates TM with every current ticket; all original files remain on disk. |

3. After the importer runs on the real substrate `plans/` tree, `synapse tm epic_progress TM` returns accurate counts matching this epic's current state.
4. `git diff plans/` after the importer run is empty (no filesystem mutations from the importer).

## Completion

- PR adds `src/activations/tm/import.rs`, the `import_from_disk` RPC method, and a fixture plan tree under `tests/fixtures/tm_import/`.
- PR description includes `cargo build -p plexus-substrate`, `cargo test -p plexus-substrate`, and a transcript of the importer running against the real `plans/` tree.
- Ticket status flipped from `Ready` → `Complete` in the same commit as the code.
