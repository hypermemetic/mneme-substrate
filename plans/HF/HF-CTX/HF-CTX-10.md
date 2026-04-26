---
id: HF-CTX-10
title: "One-shot importer: plans/<EPIC>/*.md → DB + seeded facts"
status: Pending
type: implementation
blocked_by: [HF-CTX-9]
unlocks: [HF-CTX-11]
severity: High
target_repo: hyperforge
---

## Problem

The workspace has dozens of tickets across ~20+ epic directories under `plans/`. Before HF-CTX can be the source of truth, every one of those files must be ingested into `TicketStore` without data loss, and the ticket-lifecycle / file-touch facts that the HF-CTX-4 emission hooks would have produced (had they been live at the time) must be seeded from git history. This ticket ships a one-shot, idempotent importer that walks `plans/<EPIC>/*.md`, parses each frontmatter + body, and writes a `Ticket` / `Epic` record plus seeded `TicketCreated`, `TicketStatusChanged`, `TicketLanded`, and `TouchedPath` facts per repo. Source files are **not** deleted — they become the exported mirror owned by HF-CTX-9.

## Context

Target repo: `hyperforge`. Target file: `src/ctx/import.rs`.

Existing ticket files live in `plans/<EPIC>/<EPIC>-<N>.md` (plus `<EPIC>-S<NN>.md` for spikes). Current epics across the workspace at time of drafting include: `HF`, `HF-CTX`, `HF-DC`, `HF-TT`, `HF-IR`, `TM`, `ARBOR`, `CHILD`, `CONE`, `IR`, etc. (The exact list is whatever is present at import time.)

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
scope: { ... }                                   # optional (HF-CTX-2 schema)
```

The body is everything after the closing `---`. The importer preserves it verbatim.

Idempotency: re-running the importer produces the same `TicketStore` state. Existing tickets are updated if their file representation differs; absent tickets are inserted.

### Fact seeding from git log

For each repo the importer traverses (it traverses all repos in the workspace via hyperforge's workspace discovery), it walks `git log` from the epic's starting commit forward and seeds:

- `TicketCreated { ticket_id, title, epic, scope }` at the timestamp of the commit that introduced the ticket's file. Look up via `git log --diff-filter=A --follow -- <path>`.
- `TicketStatusChanged { ticket_id, from, to, at_commit }` for each commit that modified the ticket's frontmatter's `status:` line. Reconstruct the sequence from git blame + frontmatter diffs.
- `TicketLanded { ticket_id, commits }` at the commit that flipped the ticket to `Complete`. `commits` is a `HashMap<RepoName, CommitRef>` — the repo of the ticket file plus any other repo touched in the same commit or the immediately preceding commit. Approximation; acceptable for historical seeding.
- `TouchedPath { ticket_id, path, change_kind }` for every file modified in every commit associated with the ticket. Heuristic: commits whose subject mentions the ticket id, or commits that land between a status-change pair for that ticket.

Seeding is best-effort: git history has gaps, human error, and pre-HF-CTX commits may not reference ticket ids. The importer emits whatever facts it can confidently reconstruct and records the rest as a warning.

**Seeding runs per repo.** Hyperforge knows the workspace layout; the importer enumerates repos and walks each one's git log independently. Ticket-to-repo attribution uses the `target_repo` frontmatter field plus file-path heuristics.

## Required behavior

### RPC method

| Method | Args | Return | Behavior |
|---|---|---|---|
| `import_from_disk` | `root: Option<String>` | `CtxImportResult` | Walks `<root>/<EPIC>/*.md` for every epic subdirectory. Parses each file. Inserts or updates `Ticket` / `Epic` records. Runs git-log fact seeding per repo. Returns `ok { epics_found, tickets_imported, tickets_updated, tickets_skipped, facts_seeded, errors }`. |

`root` defaults to the workspace `plans/` directory.

### Parsing

1. For each `<root>/<EPIC>/` directory:
   - Treat `<EPIC>` as the epic prefix.
   - Enumerate files matching `<EPIC>-*.md`.
   - Parse each file's frontmatter block (between the first two `---` lines) with `serde_yaml`.
   - The closing `---` and everything after is the `body`.
   - Construct a `Ticket` with every field from the frontmatter.
   - Extract the `scope:` block if present and deserialize to `TicketScope`; else `TicketScope::default()`.
   - `created_at` = file mtime (or `now()` if unavailable). `updated_at` = same.

2. Epic overview records (`<EPIC>-1.md` with `type: epic`):
   - Additionally construct an `Epic` record with `prefix = <EPIC>`, `title = ticket.title`, `goal` = content of `## Goal` section (greedy match until next `##` heading or end of file), `ticket_ids` populated by scanning the directory.

### Idempotency

For each parsed `Ticket`:

1. Look up `get_ticket(id)`.
2. If absent: `create_ticket(ticket)` — this is a privileged path that bypasses the "new tickets always land Pending" rule of HF-CTX-3 (since we're re-materializing the historical status).
3. If present and the stored ticket differs only in `updated_at`: skip.
4. If present and the stored ticket differs in any other field: apply `update_body`, `update_status`, `update_ticket_scope` as needed.

The importer does not invoke HF-CTX-8's promote gate — it writes directly through `TicketStore` as a privileged bootstrap operation. This is the only code path in HF-CTX that bypasses state-machine and auth restrictions. It is auth-gated itself (see "Auth gating" below).

### Fact seeding

For each ticket just imported / updated:

1. Resolve the ticket's target repo(s) via `target_repo` + file-path heuristic (e.g., `plans/<EPIC>/` under the hyperforge repo → `hyperforge`; exceptions noted by `target_repo`).
2. Walk `git log` on each resolved repo.
3. Emit `TicketCreated`, `TicketStatusChanged`, `TicketLanded`, `TouchedPath` facts as described in Context.
4. All facts carry `valid_at` = the commit's author timestamp.
5. Each seeded fact is de-duplicated against existing fact records (if `list_facts(filter { ticket: Some(id), kind: Some(k) })` returns a record with matching `valid_at` and payload, skip).

### Auth gating

`import_from_disk` is auth-gated the same way `promote_ticket` is (HF-CTX-8): only `Caller::Human` may invoke. Reason: the importer writes wholesale ticket state, and a misconfigured client running it after drift would clobber live work.

### Files not touched

- Non-`.md` files in `<root>/<EPIC>/` (e.g., `README.md`, diagrams, `.png`) are ignored.
- `<root>/README.md` is ignored (workspace roadmap, not a ticket).
- Subdirectories beyond one level are ignored.
- Files that don't parse (missing frontmatter, malformed YAML, unknown `status` value) are recorded in `errors` and skipped without halting the import.

### Error reporting

`CtxImportResult::ok { ..., errors: Vec<ImportError> }` where `ImportError = { path: String, reason: String }`. Empty `errors` = clean import. Non-empty = partial success, inspect `errors`.

### Does not delete

Every existing `plans/<EPIC>/*.md` file remains on disk after import. No deletions, no moves. They become the exported mirror owned by HF-CTX-9. The importer's subsequent `git diff plans/` must be empty (no filesystem mutations from the importer).

## Risks

| Risk | Mitigation |
|---|---|
| A human edits a file between the importer's read and write. | Import is a one-shot operation, not a loop. Guidance: run once at HF-CTX rollout, then only for explicit recovery. |
| Idempotency fails on ordering — a ticket references a `blocked_by` that isn't imported yet. | Store accepts the ticket; references validated at query time, not write time. All tickets end up imported; cross-references resolve on any subsequent `blocked_on_tickets` call. |
| Malformed YAML in a subset of files halts the whole import. | Per-file error handling: log, add to `errors`, continue. |
| A ticket's frontmatter `id` doesn't match its filename. | Importer warns (adds to `errors`) and uses frontmatter `id` as authoritative. |
| Git-log fact seeding is slow on large repos. | Scoped to the commits that touch `plans/<EPIC>/<ticket-id>.md` (targeted `git log -- <path>`). Fast even on large repos. |
| Seeding over-emits facts on re-run. | Dedup via `list_facts(filter)` check before each append. |
| Seeded `TouchedPath` facts miss commits that don't reference the ticket id. | Accepted: best-effort historical reconstruction. Go-forward emission via HF-CTX-4 is authoritative; historical gaps are acknowledged. |
| Importer overwrites HF-CTX's own plans (recursion: `plans/HF-CTX/`). | Expected: HF-CTX epic directory is imported same as any other. Idempotency guarantees end state matches file state. |

## What must NOT change

- Every existing `plans/<EPIC>/*.md` file remains on disk after import.
- Non-ticket files (`plans/README.md`, `docs/`, `.png`) are not touched.
- HF-CTX-2 trait surface. This ticket consumes `create_ticket`, `update_body`, `update_status`, `update_ticket_scope`, `create_epic`, `list_tickets`, `append_fact`, `list_facts` — all existing.
- HF-CTX-8's promote gate for every path except this importer's explicit bypass. The bypass is documented and auth-gated.
- Every other hyperforge hub's behavior.
- `plans/TM/*.md` is imported but not Superseded (HF-CTX-11 owns Supersession).

## Acceptance criteria

1. `cargo build --workspace` succeeds in hyperforge.
2. `cargo test --workspace` succeeds. New tests cover:

   | Scenario | Expected |
   |---|---|
   | Import a fixture `plans/` with 3 epics × 5 tickets (15 total) | `tickets_imported: 15`; `list_tickets` returns 15 entries. |
   | Run importer twice on the same fixture | Second run: `tickets_skipped: 15, tickets_imported: 0, tickets_updated: 0`. |
   | Run importer after a ticket's body changed | Second run: `tickets_updated: 1`; `get_ticket` reflects new body. |
   | Fixture with malformed YAML file | `errors: [{path, reason}]`; other files imported. |
   | Fixture with non-.md files | Ignored; no errors. |
   | Fixture where frontmatter `id` disagrees with filename | Entry in `errors`; ticket imported using frontmatter `id`. |
   | `import_from_disk` from `Caller::Agent` | `not_authorized`. |
   | `import_from_disk` from `Caller::Human` on real `plans/` tree | Completes; populates HF-CTX with every current ticket; all original files remain on disk. |
   | Git-log fact seeding: fixture repo with a commit that created `plans/E/E-1.md` | `TicketCreated` fact appended with `valid_at` == commit timestamp. |
   | Fact seeding re-run on same fixture | No duplicate facts (dedup via `list_facts`). |
   | Fixture repo with commits marching a ticket through `Pending → Ready → Complete` | Three `TicketStatusChanged` facts seeded in order. |
   | `git diff plans/` after importer run | Empty (no filesystem mutations). |

3. After import against the real workspace `plans/` tree, `synapse hyperforge ctx epic_progress HF-CTX` returns accurate counts matching this epic's current state (adapt to whatever progress query HF-CTX-5 or follow-up exposes).
4. `cargo build --workspace` + `cargo test --workspace` pass green end-to-end.

## Completion

- Commit adds `src/ctx/import.rs`, the `import_from_disk` RPC method, the git-log fact seeder, and a fixture plan tree under `tests/fixtures/ctx_import/`.
- Commit message includes `cargo build --workspace` + `cargo test --workspace` output and a transcript of the importer running against the real `plans/` tree.
- If public-surface changes warrant a version bump within 4.3.x, bump; else contribute to the existing patch line.
- Ticket status flipped from `Ready` → `Complete` in the same commit as the code.
