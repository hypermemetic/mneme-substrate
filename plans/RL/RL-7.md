---
id: RL-7
title: "Fix remaining error swallowing (mcp_session, lattice migrations, loopback reads, changelog, bash stderr)"
status: Pending
type: implementation
blocked_by: [RL-4]
unlocks: []
severity: Medium
target_repo: plexus-substrate
---

## Problem

Five additional audit-flagged error-swallowing sites remain after RL-5 and RL-6:

1. **MCP session cleanup** — `.ok()` on multiple DB operations in `mcp_session.rs`. Sessions can become stale without observability.
2. **Lattice schema migrations** — `let _ = sqlx::query("ALTER TABLE ... ADD COLUMN ...")` at `lattice/storage.rs:108-115` (approximate). Intended to tolerate "column already exists" but eats real schema errors too.
3. **Loopback approval reads** — `.read().ok()?` at `claudecode_loopback/storage.rs:68` and `.filter_map(|r| self.row_to_approval(r).ok())` at `:192` (approximate). Poisoned RwLock or corrupt row is indistinguishable from "no approvals".
4. **Changelog last-hash** — `.await.ok().flatten()` on `get_last_hash()` at `changelog/activation.rs:140, 164` (approximate). Collapses DB error into "no hash set" — can cause changelog loops because the changelog appends instead of resuming.
5. **Bash stderr truncation** — `bash/executor/mod.rs:77, 82` (approximate) silently drops stderr lines past 100 with no marker. Diagnosis of failing commands loses context when stderr is large.

Each site has a distinct fix pattern — they are grouped into one ticket because each is small and disjoint, but together they close out the "Error swallowing at critical boundaries" audit section.

## Context

**Per-site fix patterns:**

- **MCP session** — replace `.ok()` on DB ops with `Result` propagation or a structured tracing ERROR event (per RL-5's convention). If the enclosing function is a background cleanup task, log and continue with structured context (`SessionId` if ST has shipped; bare `String` otherwise).
- **Lattice migrations** — the intent is "tolerate 'column already exists'". Replace `let _ = sqlx::query(...)` with a match on the error that passes through `SQLITE_ERROR` with message containing "duplicate column name" (or equivalent) and returns `Err` on anything else. Wrap in a small helper (e.g., `fn add_column_idempotent(pool: ..., table: ..., column: ..., decl: ...) -> Result<(), LatticeError>`) if it is used at multiple sites.
- **Loopback reads** — `.read().ok()?` silently treats a poisoned RwLock as empty. Replace with an explicit match on the `PoisonError` that logs at ERROR and either returns an empty result *with a log* (documented degradation) or returns `Err` depending on the method's signature. `.filter_map(|r| self.row_to_approval(r).ok())` silently drops corrupt rows — replace with `.filter_map(|r| match self.row_to_approval(r) { Ok(a) => Some(a), Err(e) => { tracing::error!(...); None } })` so corruption is logged.
- **Changelog last-hash** — `.await.ok().flatten()` conflates a DB error with "no hash recorded yet". Replace with explicit match: propagate DB errors as `ChangelogError::...`; treat only `Ok(None)` as "no hash set".
- **Bash stderr truncation** — when truncating past line 100, append a marker line indicating how many lines were dropped (e.g., `[... N lines of stderr truncated ...]`). Append at the end of the truncated buffer. This is the smallest possible change that restores observability.

`bash/executor/mod.rs` is **shared with RL-4**. RL-4 lands first (panic replacement), RL-7 rebases and adds the stderr marker.

## Required behavior

| Site | Input | Current behavior | Required behavior |
|---|---|---|---|
| MCP session DB op | `.ok()` on a failed DB op | Session becomes stale silently | Tracing ERROR event with session id; propagate where the signature allows |
| Lattice `ALTER TABLE` | Column already exists | Swallowed (intended) | Still tolerated, but only for the specific "duplicate column name" error; other errors propagate |
| Lattice `ALTER TABLE` | Disk full / permissions / syntax error | Swallowed (unintended) | Propagates as `LatticeError::...` |
| Loopback `RwLock::read()` | Lock is poisoned | `.ok()?` returns empty | Tracing ERROR event; per-method decision on return shape (documented in code comment) |
| Loopback `row_to_approval` | Row deserialisation fails | Row silently skipped | Tracing ERROR event per skipped row with the row identifier |
| Changelog `get_last_hash` | DB query errors | Collapsed into `None` | Propagates as `ChangelogError::...`; only true `Ok(None)` is treated as "no hash yet" |
| Bash stderr buffer | More than 100 stderr lines | Lines 101+ silently dropped | Lines 101+ dropped but a marker `[... N lines of stderr truncated ...]` is appended to the buffer |

## Risks

- **Behavioural change in lattice migrations.** If the original intent was "tolerate any error from `ALTER TABLE`" (broader than "tolerate duplicate column"), the stricter matching here will cause startup failures on real schema drift — which is exactly the intent of this ticket, but may expose latent schema issues in production databases. Implementor verifies against a fresh SQLite (should produce no error) and a pre-existing SQLite (should hit the "duplicate column" path). If other errors emerge, replan — they are real bugs.
- **Changelog error-propagation ripple.** The audit notes `.await.ok().flatten()` "can cause changelog loops." Fixing the swallow may surface the underlying DB error as a visible failure. That is the correct behaviour; if it breaks the changelog activation under normal operation, there is a pre-existing DB issue to address — not this ticket's scope.
- **Stderr marker placement.** Truncating at line 100 and appending a marker changes the byte count of what callers see. If any caller asserts on exact stderr byte length in tests, that test updates.

## What must NOT change

- The set of RPC method names or request/response shapes on MCP, Lattice, Loopback, Changelog, or Bash activations.
- Any SQLite schema.
- The bash executor's stdout handling (stderr-only change).
- The semantics of a successful (non-erroring) call at any of these sites.
- Existing `cargo test` pass rate beyond tests that assert exact stderr length (which may be updated for criterion 7).
- Files outside the five listed: `mcp/mcp_session.rs`, `lattice/storage.rs`, `claudecode_loopback/storage.rs`, `changelog/activation.rs`, `bash/executor/mod.rs`, plus each activation's `error.rs` as needed.

## Acceptance criteria

1. Grep for `.ok()` applied to DB-op results in `mcp/mcp_session.rs` returns zero matches at DB-op call sites (acceptable elsewhere).
2. Grep for `let _ = sqlx::query("ALTER TABLE` in `lattice/storage.rs` returns zero matches.
3. Grep for `.read().ok()?` and for `.filter_map(|r| self.row_to_approval(r).ok())` (or the more general pattern `self.row_to_approval(r).ok()`) in `claudecode_loopback/storage.rs` returns zero matches.
4. Grep for `.await.ok().flatten()` on `get_last_hash` (or any DB-returning call) in `changelog/activation.rs` returns zero matches.
5. The bash executor, when fed stderr containing > 100 lines, produces output whose last line is the truncation marker `[... N lines of stderr truncated ...]` with `N` equal to the count of dropped lines.
6. A unit test per site (five tests) drives each site into its previously-swallowed failure and asserts the new observable behaviour (tracing event, error propagation, or truncation marker).
7. All existing `cargo test` targets pass.

## Completion

Implementor delivers:

- Patches to the five files listed.
- Five unit tests (criterion 6).
- Any `error.rs` additions needed.
- `cargo test` output confirming criterion 7.
- Status flip to `Complete` in the same commit that lands the code.
