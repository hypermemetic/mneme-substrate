---
id: HF-CTX-11
title: "Mark plans/TM/*.md as Superseded with superseded_by pointers into HF-CTX"
status: Pending
type: implementation
blocked_by: [HF-CTX-10]
unlocks: []
severity: Medium
target_repo: hyperforge
---

## Problem

The substrate-local TM drafts at `plans/TM/` were frozen inspiration for HF-CTX. With HF-CTX-2 through HF-CTX-10 pinned and concrete implementation tickets landed, the TM drafts are superseded. This ticket flips every `plans/TM/*.md` file's frontmatter to `status: Superseded` and adds a `superseded_by: HF-CTX-<N>` field pointing at the HF-CTX ticket that absorbed each concept. Operation is a bulk frontmatter edit via the `ruamel.yaml` pattern from the ticketing skill.

## Context

Target repo: `hyperforge` (or `plexus-substrate`, depending on where `plans/TM/*.md` lives — these files are under the plexus-substrate repo's `plans/TM/` today). Target files: all `.md` files under `/Users/shmendez/dev/controlflow/hypermemetic/plexus-substrate/plans/TM/`.

Files at drafting time:

| TM file | HF-CTX mapping |
|---|---|
| `TM-1.md` (epic overview) | `HF-CTX-1` |
| `TM-2.md` (TicketStore trait + types) | `HF-CTX-2` |
| `TM-3.md` (CRUD RPC methods) | `HF-CTX-3` |
| `TM-4.md` (query methods) | `HF-CTX-5` |
| `TM-5.md` (watch / stream methods) | `HF-CTX-6` |
| `TM-6.md` (human promotion gate) | `HF-CTX-8` |
| `TM-7.md` (Orcha integration) | *No direct mapping* — integration is workspace-wide and happens as activations consume `hyperforge.ctx` library API. Map to `HF-CTX-1` (epic). |
| `TM-8.md` (filesystem export) | `HF-CTX-9` |
| `TM-9.md` (one-shot import) | `HF-CTX-10` |
| `TM-S01.md` (absorb-vs-coexist spike) | *No direct mapping* — superseded conceptually because HF-CTX doesn't coexist with `orcha/pm`; it is a different concern entirely. Map to `HF-CTX-1`. |
| `TM-S02.md` (typed-vs-DSL spike) | `HF-CTX-S02` (query-surface spike). |

Where a clean 1:1 mapping exists, use it. For TM files that have no direct HF-CTX counterpart, point at `HF-CTX-1` (the epic overview) — the entire HF-CTX epic is the successor.

The ticketing skill's "Bulk frontmatter updates" section provides the `ruamel.yaml` pattern. Use it here to preserve formatting and comments.

## Required behavior

### Bulk edit

For every file matching `plans/TM/*.md`:

1. Load frontmatter via `ruamel.yaml`.
2. Set `status: Superseded`.
3. Add `superseded_by: <HF-CTX-N>` per the mapping table.
4. Leave the body untouched.
5. Write back with preserved formatting.

Script to execute (modeled on the skill's "Bulk frontmatter updates" section):

```python
from ruamel.yaml import YAML
import pathlib, io

yaml = YAML()
mapping = {
    "TM-1":   "HF-CTX-1",
    "TM-2":   "HF-CTX-2",
    "TM-3":   "HF-CTX-3",
    "TM-4":   "HF-CTX-5",
    "TM-5":   "HF-CTX-6",
    "TM-6":   "HF-CTX-8",
    "TM-7":   "HF-CTX-1",
    "TM-8":   "HF-CTX-9",
    "TM-9":   "HF-CTX-10",
    "TM-S01": "HF-CTX-1",
    "TM-S02": "HF-CTX-S02",
}
for p in pathlib.Path("plans/TM").glob("TM-*.md"):
    text = p.read_text()
    if not text.startswith("---\n"):
        continue
    _, front, body = text.split("---\n", 2)
    data = yaml.load(front)
    tid = str(data.get("id", ""))
    data["status"] = "Superseded"
    data["superseded_by"] = mapping.get(tid, "HF-CTX-1")
    buf = io.StringIO()
    yaml.dump(data, buf)
    p.write_text(f"---\n{buf.getvalue()}---\n{body}")
```

### Verification

After the script runs:

- Every `plans/TM/*.md` file has `status: Superseded`.
- Every file has a `superseded_by: HF-CTX-*` field.
- File bodies are unchanged (verified via `git diff plans/TM/` showing only frontmatter edits).
- No files are deleted, moved, or renamed.

### Import consistency

HF-CTX-10 has already imported `plans/TM/*.md` into the `TicketStore` as regular tickets (not marked Superseded, because their frontmatter at import time didn't say so). This ticket's edits trigger HF-CTX-9's real-time exporter — the exporter will observe the file mutations via the filesystem watcher *only if* the exporter also watches inbound file changes (it doesn't: HF-CTX-9 is strictly one-way DB → files).

Therefore this ticket ALSO calls `import_from_disk` (HF-CTX-10) one more time after the bulk edit completes, which re-parses the TM files with their new `status: Superseded` frontmatter and updates the corresponding `Ticket` records in the DB. The re-import is idempotent for all other epics (no changes); for TM, it flips the in-DB status to `Superseded` and writes the `superseded_by` field.

This is the only case where "edit files, then re-import" is the flow. Every other HF-CTX operation is DB → files, one-way.

## Risks

| Risk | Mitigation |
|---|---|
| `ruamel.yaml` normalizes whitespace / quoting, producing a noisy diff. | Review `git diff plans/TM/` before committing; if churn is excessive, fall back to a targeted regex edit for just the `status:` and `superseded_by:` lines. |
| Mapping table has a gap (new TM file appeared between drafting and execution). | Default-to-`HF-CTX-1` branch in the script handles any unmapped id. |
| Re-import after bulk edit produces unexpected DB drift (e.g., body differences). | The bulk edit only touches frontmatter; bodies are byte-identical. Re-import is a pure status/superseded_by update. |
| Exporter runs immediately after re-import and overwrites the freshly-edited TM files with DB content. | Since DB now has `status: Superseded` and `superseded_by` for those tickets, the exported output matches the bulk-edited input. Idempotent. |
| `plans/TM/` files live under `plexus-substrate`, not `hyperforge`. | The bulk edit runs against the filesystem where the files actually live. The HF-CTX-10 re-import is initiated from hyperforge; hyperforge's workspace layout knows `plexus-substrate` as a repo and imports `plans/TM/` as it does every other `plans/` subtree. |

## What must NOT change

- Any `.md` file under `plans/` directories other than `plans/TM/`.
- The body content of any `plans/TM/*.md` file.
- HF-CTX-1 through HF-CTX-10's behavior.
- HF-CTX-2's trait surface.
- Every other hyperforge hub's compile and test behavior.

## Acceptance criteria

1. Every file under `plans/TM/` has `status: Superseded` in its frontmatter. Verified by:

   ```
   grep -L '^status: Superseded' plans/TM/*.md
   ```

   must produce no output.

2. Every file under `plans/TM/` has a `superseded_by: HF-CTX-*` field. Verified by:

   ```
   grep -L '^superseded_by: HF-CTX-' plans/TM/*.md
   ```

   must produce no output.

3. File bodies are unchanged. Verified by `git diff plans/TM/` showing only frontmatter line changes — no body text modifications.

4. No files are deleted, moved, or renamed under `plans/TM/`. Verified by `git status plans/TM/` showing only Modified entries (no Deleted, no Renamed).

5. After re-importing via `synapse hyperforge ctx import_from_disk`, `synapse hyperforge ctx get_ticket TM-2` returns a ticket with `status: Superseded` and `superseded_by: HF-CTX-2`.

6. `cargo build --workspace` + `cargo test --workspace` pass green end-to-end in both hyperforge and plexus-substrate.

## Completion

- Commit (in `plexus-substrate`) contains the edited `plans/TM/*.md` files — one commit, rationale in the message.
- Commit (in `hyperforge`, if any code changes) contains whatever minor HF-CTX fixes are needed to make re-import clean. Likely no hyperforge commit is needed; the import flow was shipped in HF-CTX-10.
- The two commits, taken together, satisfy rule 12's integration gate.
- Commit messages explain the supersession rationale and reference HF-CTX-1.
- Ticket status flipped from `Ready` → `Complete` in the same commit as the bulk edit in `plexus-substrate`.
