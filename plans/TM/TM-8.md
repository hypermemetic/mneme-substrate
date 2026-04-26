---
id: TM-8
title: "TM filesystem export (DB → plans/<EPIC>/*.md)"
status: Pending
type: implementation
blocked_by: [TM-3, TM-5]
unlocks: []
severity: Medium
target_repo: plexus-substrate
---

## Problem

TM holds the authoritative ticket state in SQLite. For humans browsing the repo, for git history, and for pre-TM workflows that still expect files, the `plans/<EPIC>/*.md` tree must be a faithful mirror of the DB. This ticket implements a one-way DB → filesystem exporter that renders every ticket to its frontmatter-plus-body markdown form and keeps the directory in sync as mutations happen.

## Context

Target repo: `plexus-substrate`. Target file: `src/activations/tm/export.rs`.

Existing ticket file format (see `skills/ticketing/SKILL.md`):

```markdown
---
id: EPIC-N
title: "Short description"
status: Pending
type: implementation
blocked_by: []
unlocks: []
severity: Medium
target_repo: some-repo
superseded_by: null
---

[body markdown]
```

Directory convention: `plans/<EPIC>/<EPIC>-N.md`, epic overview at `plans/<EPIC>/<EPIC>-1.md`.

Open design choice: **real-time export** (subscribe to `TicketEvent` via `TmHandle::subscribe()` and rewrite one file per event) vs **on-demand export** (RPC method `export_all` that dumps every ticket). Pin: this ticket ships **both**. The real-time path is the default; the on-demand path is the recovery / bulk-regenerate tool.

Reason for both: real-time is the operational default (human runs `git diff plans/` and sees the change immediately after any mutation). On-demand is needed for recovery (filesystem and DB drifted apart, operator wants to regenerate) and for bootstrapping (after TM-9 importer runs, the first exporter pass fills in any tickets that were DB-only).

Direction of sync is **strictly one-way**. This ticket does not read `plans/` files back into the DB. Manual edits to exported files are silently overwritten on the next mutation. TM-9 handles the one-shot import; after that, the filesystem is read-only as far as TM is concerned.

## Required behavior

### Rendering

`render_ticket(&Ticket) -> String` produces the full markdown file contents:

1. Opening frontmatter delimiter `---`.
2. Frontmatter fields in fixed order: `id`, `title`, `status`, `type`, `blocked_by`, `unlocks`, `severity` (omitted if `None`), `target_repo` (omitted if `None`), `superseded_by` (omitted if `None`).
3. Closing frontmatter delimiter `---`.
4. Blank line.
5. Ticket `body`.
6. Trailing newline.

Rendering is deterministic — rendering the same `Ticket` twice produces byte-identical output. Lists (`blocked_by`, `unlocks`) serialize in the order stored in the `Ticket` (not re-sorted).

### Real-time export

A background task in `TmActivation::start_exporter()`:

1. Subscribes to `TmHandle::subscribe()` (TM-5's broadcast channel).
2. Per event:
   - `created { ticket }` → write `plans/<epic>/<id>.md`.
   - `body_updated { id }` or `status_changed { id }` → load full ticket via store, write the file.
   - `deleted { id }` → delete the file. (Safe because DB is authoritative; if the file is absent, no error.)
   - `epic_created { epic }` → `mkdir -p plans/<prefix>/` (no file content yet; epic overview lives in the `-1` ticket).
   - `lagged { missed }` → trigger a full `export_all` pass for reconciliation.
3. File writes are atomic: write to `plans/<epic>/<id>.md.tmp`, then rename. This prevents a partially-written file from being observed by a human running `cat` mid-write.

The exporter is started when the TM activation starts and cancelled on shutdown.

### On-demand export RPC

| Method | Args | Return | Behavior |
|---|---|---|---|
| `export_all` | `(none)` | `TmExportResult` | Iterates every ticket in `TicketStore`, writes each to disk via `render_ticket`. Removes any `plans/<epic>/<id>.md` file in the tree that does not correspond to a ticket in the store (stale-file sweep, epic-scoped). Returns `ok { tickets_written, files_removed }`. |

The stale-file sweep looks only under `plans/<known_epic>/` for each epic present in `list_epics`. Files under an epic directory that has no matching epic record are left untouched (they belong to non-TM epics — the importer has not yet absorbed them).

### Configuration

| Setting | Default | Notes |
|---|---|---|
| `export_root` | Workspace's `plans/` directory | Relative to the substrate working directory. |
| `real_time_enabled` | `true` | Set to `false` for environments that don't want filesystem writes on every mutation (e.g., test isolation). |

Configuration is surfaced via the standard substrate config mechanism — if OB's config loader has landed, via TOML; otherwise, via constructor args.

## Risks

| Risk | Mitigation |
|---|---|
| Two TM instances point at the same `plans/` tree and race on writes. | Atomic rename prevents torn files. Last write wins; for a single-workspace deployment this is acceptable. |
| Real-time export flushes thousands of events on startup (post-TM-9 import). | The exporter treats a startup flood as normal; atomic writes are cheap enough. If contention becomes real, coalesce with a 100ms debounce per-ticket-id (out of scope for this ticket). |
| `plans/` contains files from epics not known to TM (e.g., pre-TM-9 epics not yet imported). | `export_all`'s sweep is epic-scoped and only touches epics present in `list_epics`. Unknown-epic directories are left intact. |
| A human edit to a `plans/` file is silently overwritten. | By design (DB is source of truth). Document this in TM-1's "What must NOT change" — it is a policy, not a bug. |

## What must NOT change

- TM-3/4/5/6's RPC surface.
- TM-2's store trait.
- Any `plans/` file under an epic directory that has no matching epic record in TM (non-TM epics are left alone).
- Every other substrate activation's behavior.
- Non-TM files inside `plans/<epic>/` directories (e.g., `README.md`, design notes). Sweep only removes `<EPIC>-<N>.md` files that don't match a known ticket.

## Acceptance criteria

1. `cargo build -p plexus-substrate` succeeds.
2. `cargo test -p plexus-substrate` succeeds. Tests cover:

   | Scenario | Expected |
   |---|---|
   | `render_ticket(&t)` called twice on the same ticket | Identical byte output. |
   | `render_ticket` on a ticket with every frontmatter field set | Output passes a YAML frontmatter parser that extracts back the same fields. |
   | Real-time export: create a ticket via TM, poll `plans/<epic>/<id>.md` | File appears within 1 second, with matching content. |
   | Real-time export: `update_ticket_status` | File's frontmatter shows the new status; file content otherwise unchanged. |
   | Real-time export: `delete_ticket` | File is absent. |
   | `export_all` with a stale `plans/TM/TM-999.md` (no matching ticket) | Stale file removed; returns `files_removed >= 1`. |
   | `export_all` with a non-TM epic directory `plans/LEGACY/` | `LEGACY/` files are untouched. |
   | Atomic write: simulated crash mid-write (via injected hook) | No `.md.tmp` file survives after the next successful export; no partially-written `.md`. |

3. `git diff plans/` after running `synapse tm create_ticket` shows a clean, minimal diff (no whitespace churn).
4. A follow-up `synapse tm export_all` run on a clean state is a no-op (`git diff plans/` empty).

## Completion

- PR adds `src/activations/tm/export.rs`, the `export_all` RPC method, and the real-time exporter task wiring.
- PR description includes `cargo build -p plexus-substrate`, `cargo test -p plexus-substrate`, and a before/after `git diff plans/TM/` transcript demonstrating real-time export.
- Ticket status flipped from `Ready` → `Complete` in the same commit as the code.
