---
id: HF-CTX-9
title: "Filesystem exporter: DB → plans/<EPIC>/<EPIC>-N.md (one-way)"
status: Pending
type: implementation
blocked_by: [HF-CTX-8]
unlocks: [HF-CTX-10]
severity: Medium
target_repo: hyperforge
---

## Problem

Hyperforge holds the authoritative ticket state in SQLite. For humans browsing the repo, for git history, and for pre-HF-CTX workflows that still expect files, the `plans/<EPIC>/*.md` tree must be a faithful mirror of the DB. This ticket implements a strictly one-way DB → filesystem exporter that renders every ticket to its frontmatter-plus-body markdown form and keeps the directory in sync as mutations happen. The exporter emits no facts — facts come from the ticket status changes the DB already recorded (via HF-CTX-3 / HF-CTX-4 / HF-CTX-8).

## Context

Target repo: `hyperforge`. Target file: `src/ctx/export.rs`.

Existing ticket file format (per `skills/ticketing/SKILL.md`):

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

Directory convention: `plans/<EPIC>/<EPIC>-N.md`; epic overviews at `plans/<EPIC>/<EPIC>-1.md`.

Export modes: **real-time** (subscribe to `TicketEvent` via HF-CTX-6's `watch_all` and rewrite one file per event) and **on-demand** (RPC method `export_all` that dumps every ticket). Ships both. Real-time is the default; on-demand is recovery / bulk-regenerate.

Direction is **strictly one-way**. This ticket does not read `plans/` files back into the DB. HF-CTX-10 handles the one-shot import; after that, the filesystem is read-only as far as HF-CTX is concerned. Manual edits to exported files are silently overwritten on the next mutation.

**The exporter emits no facts.** All status changes, body updates, and deletes that drive the exporter were already recorded as facts by HF-CTX-3 / HF-CTX-4 / HF-CTX-8 before the `TicketEvent` that triggers the export. The exporter is a pure projection; it observes events and writes files.

## Required behavior

### Rendering

`render_ticket(&Ticket) -> String` produces the full markdown file contents:

1. Opening frontmatter delimiter `---`.
2. Frontmatter fields in fixed order: `id`, `title`, `status`, `type`, `blocked_by`, `unlocks`, `severity` (omitted if `None`), `target_repo` (omitted if `None`), `superseded_by` (omitted if `None`), `scope` (omitted if `TicketScope::default()`).
3. Closing frontmatter delimiter `---`.
4. Blank line.
5. Ticket `body`.
6. Trailing newline.

Rendering is deterministic — rendering the same `Ticket` twice produces byte-identical output. Lists (`blocked_by`, `unlocks`) serialize in stored order (not re-sorted).

The `scope:` block, when present, serializes in a stable field order: `repos`, `packages`, `ecosystems`, `starts_from`, `ends_at`, `versions_before`, `versions_after`, `introduces`, `deprecates`, `removes`, `touches`, `tags_created`.

### Real-time export

A background task in `CtxActivation::start_exporter()`:

1. Subscribes to the ticket-event broadcast (from HF-CTX-6).
2. Per event:
   - `created { ticket }` → write `plans/<epic>/<id>.md`.
   - `body_updated { id }` or `status_changed { id }` or `scope_updated { id }` → load full ticket via store, write the file.
   - `deleted { id }` → delete the file. (Safe: DB is authoritative; if file absent, no error.)
   - `epic_created { epic }` → `mkdir -p plans/<prefix>/`. No file content (epic overview lives in the `-1` ticket).
   - `lagged { missed }` → trigger a full `export_all` pass for reconciliation.
3. File writes are atomic: write to `plans/<epic>/<id>.md.tmp`, then rename. Prevents a partially-written file from being observed mid-write.

Exporter is started when the `hyperforge.ctx` activation starts, cancelled on shutdown.

### On-demand export RPC

| Method | Args | Return | Behavior |
|---|---|---|---|
| `export_all` | `(none)` | `CtxExportResult` | Iterates every ticket in `TicketStore`, writes each to disk via `render_ticket`. Removes any `plans/<epic>/<id>.md` file that does not correspond to a ticket in the store (stale-file sweep, epic-scoped). Returns `ok { tickets_written, files_removed }`. |

Stale-file sweep looks only under `plans/<known_epic>/` for each epic in `list_epics`. Files under an epic directory that has no matching epic record are untouched (they belong to non-HF-CTX epics — HF-CTX-10's importer hasn't absorbed them yet, or they're explicitly out of scope).

### Configuration

| Setting | Default | Notes |
|---|---|---|
| `export_root` | Workspace's `plans/` directory | Relative to the hyperforge working directory. |
| `real_time_enabled` | `true` | Set to `false` for test isolation. |

Configuration surfaced via hyperforge's standard config mechanism.

### Superseded / non-HF-CTX files

Files under epic directories that are not owned by HF-CTX (e.g., `plans/TM/*.md` during the HF-CTX rollout, before HF-CTX-11 marks them Superseded) are left alone by the exporter unless they correspond to tickets imported by HF-CTX-10. This lets the `plans/TM/` tree coexist unchanged until HF-CTX-11 explicitly marks it.

## Risks

| Risk | Mitigation |
|---|---|
| Two activation instances point at the same `plans/` tree and race on writes. | Atomic rename prevents torn files. Last write wins; single-workspace deployment accepts this. |
| Real-time export flushes thousands of events on startup (post-HF-CTX-10 import). | Exporter treats startup flood as normal; atomic writes are cheap. If contention shows up, coalesce with 100ms debounce per-ticket-id (out of scope for this ticket). |
| `plans/` contains files from epics not known to HF-CTX (e.g., pre-HF-CTX-10 epics). | `export_all`'s sweep is epic-scoped, only touches epics in `list_epics`. Unknown-epic directories are left intact. |
| Human edits are silently overwritten. | By design (DB is source of truth). Documented in HF-CTX-1's "What must NOT change". |
| Exporter crashes mid-write, leaving `.md.tmp` files. | Recovery sweep on next startup removes stray `.md.tmp` files older than 60 seconds. Pinned in the exporter init. |

## What must NOT change

- HF-CTX-3/4/5/6/7/8's RPC surface and behavior.
- HF-CTX-2's store trait.
- Any `plans/<EPIC>/*.md` under an epic directory that has no matching epic record in HF-CTX (non-HF-CTX epics are left alone).
- Non-ticket files inside `plans/<epic>/` directories (e.g., `README.md`, design notes, `.png` fixtures). Sweep only removes `<EPIC>-<N>.md` files that don't match a known ticket.
- Every other hyperforge hub's behavior.

## Acceptance criteria

1. `cargo build --workspace` succeeds in hyperforge.
2. `cargo test --workspace` succeeds. New tests cover:

   | Scenario | Expected |
   |---|---|
   | `render_ticket(&t)` called twice on same ticket | Byte-identical output. |
   | `render_ticket` on a ticket with every frontmatter field set | YAML frontmatter parser extracts back the same fields. |
   | `render_ticket` on a ticket with a populated `TicketScope` | Scope block parses back to the same `TicketScope`. |
   | Real-time export: create a ticket via HF-CTX-3, poll `plans/<epic>/<id>.md` | File appears within 1 second with matching content. |
   | Real-time export: `update_status` | File's frontmatter shows new status; file otherwise unchanged. |
   | Real-time export: `delete_ticket` | File absent. |
   | `export_all` with a stale `plans/HF-CTX/HF-CTX-999.md` (no matching ticket) | Stale file removed; `files_removed >= 1`. |
   | `export_all` with a non-HF-CTX epic directory `plans/LEGACY/` | `LEGACY/` files untouched. |
   | Atomic write: simulated crash mid-write (via injected hook) | No `.md.tmp` survives next export; no partially-written `.md`. |
   | Exporter start-up sweep | Removes any stale `.md.tmp` > 60s old. |
   | `export_all` run twice on clean state | Second run is a no-op; `git diff plans/` empty. |

3. `git diff plans/` after running `synapse hyperforge ctx create_ticket` shows a clean, minimal diff.
4. No facts are appended to the fact log as a direct result of `export_all` (regression pin — exporter emits no facts).
5. `cargo build --workspace` + `cargo test --workspace` pass green end-to-end.

## Completion

- Commit adds `src/ctx/export.rs`, the `export_all` RPC method, and the real-time exporter task wiring.
- Commit message includes `cargo build --workspace` + `cargo test --workspace` output, plus a before/after `git diff plans/HF-CTX/` transcript demonstrating real-time export.
- If public-surface changes warrant a version bump within 4.3.x, bump; else contribute to the existing patch line.
- Ticket status flipped from `Ready` → `Complete` in the same commit as the code.
