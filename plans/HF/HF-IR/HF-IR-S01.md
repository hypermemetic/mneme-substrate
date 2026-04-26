---
id: HF-IR-S01
title: "Spike: ratify hyperforge child-gate mapping + verify multi-level synapse nesting"
status: Pending
type: spike
blocked_by: [HF-TT-1]
unlocks: [HF-IR-2]
severity: High
target_repo: hyperforge
---

## Question

1. Which of hyperforge's existing flat `list_X` / `get_X` method pairs (across `HyperforgeHub`, `WorkspaceHub`, `RepoHub`, `BuildHub`, `ImagesHub`, `ReleasesHub`, `AuthHub`) are semantically equivalent to a dynamic child gate and should be replaced by `#[child(list = "...")]`, vs which return aggregated / cross-cutting state and must stay flat?
2. For every gate chosen in (1), what are the exact `list_method` and (where applicable) `search_method` names?
3. Does synapse 3.12.0's tree renderer correctly handle multi-level dynamic nesting — specifically `workspace.repo.package` (3 levels deep) and `workspace.repo.artifact` — when each level is declared with `#[child(list = ...)]`?

## Setup

1. Inventory: for each of the 7 hyperforge activations, enumerate every method whose name is `list_X`, `get_X`, `find_X`, or `<verb>_X_by_id` and classify it into one of:
   - **Gate candidate** — returns an enumeration of child-addressable entities (e.g., `list_repos -> Vec<Repo>`).
   - **Aggregate** — returns cross-cutting state keyed by id but not suitable as a child surface (e.g., `get_build_statuses() -> HashMap<PackageName, BuildStatus>`).
   - **Ambiguous** — flag for design review.
2. For every gate candidate, propose a `list_method` name (convention: pluralized lowercase, e.g., `repo_names`, `package_names`, `artifact_ids`) and decide whether a `search_method` is warranted (e.g., `find_credential` for `AuthHub.credential`).
3. Record the mapping in a table mirroring HF-IR-1's "Proposed child-gate mapping" table, with any deviations explicitly called out.
4. In a throwaway substrate branch or fixture server, register three nested fixture activations — `FixtureWorkspaceHub` → `FixtureRepoHub` → `FixturePackageHub` — each with a `#[child(list = "...")]` gate declaring dynamic children. Connect synapse 3.12.0 and run:
   - `synapse <fixture-root>`
   - `synapse <fixture-root> workspace <ws>`
   - `synapse <fixture-root> workspace <ws> repo <r>`
   - `synapse <fixture-root> workspace <ws> repo <r> package <p>`
5. Capture the tree output at each depth; confirm synapse renders the nested dynamic children with correct indentation and without truncation or "unknown method" fallback.

## Pass condition

All three of the following hold:

- The table from step 2 has zero rows marked Ambiguous (every candidate is either Gate or Aggregate with a pinned rationale).
- Every Gate row has a concrete `list_method` name and a concrete `search_method` decision (name or `None`).
- synapse 3.12.0 renders the 3-level fixture tree correctly at every depth exercised in step 4; tree output at depth 3 shows the package children listed beneath the enclosing repo, beneath the enclosing workspace.

Binary: all three hold → PASS. Any failing → FAIL.

## Fail → next

- If the fixture demonstrates synapse cannot render 3-level dynamic nesting (e.g., truncates at depth 2, collapses nested trees, or renders child gates as flat methods): file a synapse follow-up ticket in `plans/SYN/` (or the current synapse epic subdir) capturing the exact failure mode and a minimal reproduction. Add that ticket to HF-IR-10's `blocked_by`. HF-IR-3..8 may still proceed — the Rust-side child gates compile and route correctly even if synapse's rendering lags.
- If the inventory surfaces a method whose semantics are incompatible with a dynamic child gate (e.g., a `list_X` that returns derived aggregates rather than addressable children), document it in the ratified mapping table as "stays flat — not a gate" and do NOT deprecate it in HF-IR-9.

## Fail → fallback

None — the spike output drives HF-IR-2 onward. Without the ratified mapping, the downstream tickets cannot be scoped.

## Time budget

Four focused hours. Two hours for the inventory + mapping; two hours for the synapse fixture test. If the budget overruns, stop and report regardless of pass/fail state.

## Out of scope

- Implementing any of the child gates in hyperforge proper (that's HF-IR-3..8).
- Changing synapse's rendering code even if the fixture fails (that's a synapse follow-up ticket).
- Modifying the `BuildSystemKind::ecosystem()` partitioning of packages (HF-TT-7 territory).

## Completion

Spike delivers:

1. A ratified child-gate mapping table committed to HF-IR-1's Context section (or a new doc at `plans/HF/HF-IR/mapping.md` linked from HF-IR-1), replacing the "Proposed child-gate mapping" placeholder with pinned `list_method` and `search_method` values per row.
2. Synapse multi-level nesting test result: PASS or FAIL with reproduction steps and, if FAIL, a filed synapse follow-up ticket referenced here.
3. Time spent, one-paragraph summary of findings.
4. Status flipped to Complete in the same commit that lands the ratified mapping.

Report lands before HF-IR-2 is promoted to Ready.
