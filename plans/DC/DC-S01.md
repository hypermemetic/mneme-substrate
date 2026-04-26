---
id: DC-S01
title: "Spike: convention-only vs workspace split for activation boundary enforcement"
status: Pending
type: spike
blocked_by: []
unlocks: [DC-2, DC-7]
severity: High
target_repo: plexus-substrate
---

## Question

Can we get compiler-enforced activation boundaries by converting **one** substrate activation into its own workspace crate with acceptable migration cost, **OR** is convention-only enforcement (lint + CI check) the lower-cost path that still achieves the decoupling goal?

This is a binary choice, pinned by comparing the cost of converting **one** activation either way. The spike's output picks the path DC-2 and DC-7 will follow for the remaining five activations.

## Context

Substrate is a single-crate today. `src/activations/<name>/mod.rs` conventions already exist, but `pub(crate)` does not enforce cross-activation boundaries because every activation module is inside the same crate — `pub(crate)` items are visible everywhere in `plexus-substrate`.

Two enforcement options:

**Option A — Convention-only.**
- Each activation's `mod.rs` re-exports only the library API.
- Internals are kept `pub(crate)` but documented as internal.
- A lint (custom clippy rule, rustdoc hygiene check, or grep-based CI script) fails the build when a sibling imports a non-re-exported item.
- No Cargo restructuring.

**Option B — Workspace-split.**
- Convert `plexus-substrate` to a Cargo workspace. Each activation becomes `substrate-<name>` crate.
- `pub(crate)` now truly isolates the activation.
- The top-level `plexus-substrate` crate depends on each activation crate and assembles the DynamicHub.
- Each activation's `Cargo.toml` declares explicit dependencies on other activations — compiler catches any undeclared reach-in at build time.
- Significantly higher migration cost — every activation's module paths change, build times and Cargo.lock shape change.

User prior lean (per `feedback_activation_coupling.md`): convention-only first, workspace-split later if drift recurs. The spike confirms or reverses that lean based on measured migration cost.

**Test activation for the spike: `bash`.** It's the simplest stateful-ish activation (has an executor submodule but no SQLite storage), has exactly one sibling reaching into it (Cone), and has the fewest external deps. Easiest migration candidate either direction.

## Setup

Two parallel sub-experiments. Run both. Whichever finishes under budget and achieves compile-clean first picks the path.

**Sub-experiment A — Convention-only prototype.**

1. In a throwaway branch, add a lint or CI script that greps substrate's `src/` for `use crate::activations::bash::` outside `src/activations/bash/**`.
2. Verify it currently fires (Cone has one such import at `cone/activation.rs:8`).
3. Write Bash's library API: re-export only `Bash` (the activation struct), its constructor, and any domain types Cone needs. Demote `Bash::PLUGIN_ID` to `pub(crate)` or move Cone's handle-plugin-id check to a sanctioned library function.
4. Verify the lint now passes and `cargo test` remains green.
5. **Measure:** wall-clock time for steps 1–4, plus the number of source-file modifications required.

**Sub-experiment B — Workspace-split prototype.**

1. In a throwaway branch, convert `plexus-substrate` to a workspace. Create `crates/substrate-bash/` with Bash's code.
2. Add `substrate-bash` as a path dependency of the top-level `plexus-substrate` crate.
3. Fix all import paths in the top-level crate and in Cone to reference `substrate_bash::Bash` instead of `crate::activations::bash::Bash`.
4. Verify `cargo test` remains green.
5. **Measure:** wall-clock time for steps 1–4, plus the number of source-file modifications required, plus any `Cargo.toml` changes across the workspace.

## Pass condition

**Binary:** PASS for whichever sub-experiment reaches compile-clean + tests green first under the time budget.

- Sub-experiment A passes if: lint catches the reach-in, Bash's library API is narrow, Cone's import satisfies the library API, and `cargo test` is green **in under 2 focused hours**.
- Sub-experiment B passes if: workspace conversion is complete, `substrate-bash` is a standalone crate, Cone imports it by crate name, and `cargo test` is green **in under 4 focused hours**.

If both pass: the one with fewer source-file modifications wins. Tie → workspace-split wins because it gives compiler-enforced guarantees for free going forward.

If only one passes under budget: that one is the chosen path.

If neither passes under budget: the spike **fails**. Document the specific obstacle (circular dep, macro path issue, etc.) and DC-2 gets a replanning trigger — investigate obstacle-specific options (partial workspace, per-module lint rules, etc.).

## Fail → next

Neither sub-experiment reaches compile-clean under budget: write DC-S02 targeting the specific obstacle. Candidate S02 directions:

- If macro path resolution blocks workspace conversion: investigate whether `#[plexus_macros::activation(crate_path = "...")]` needs a syntax update. Feed findings to plexus-macros maintainers.
- If circular deps between activations block workspace conversion: investigate whether DC's other tickets (especially DC-6 for Arbor) resolve the cycle. Re-run S01 after DC-6 lands.
- If convention-only can't reliably lint (e.g., grep false positives): investigate whether a dedicated clippy plugin is viable.

## Fail → fallback

If both paths prove infeasible: DC falls back to **best-effort convention**. Document the coupling sites DC-3..DC-6 resolved, skip DC-7's mechanical enforcement, and flag future hygiene as a manual review responsibility. This is a degraded outcome — document clearly if reached.

## Time budget

6 hours total across both sub-experiments. Stop and report regardless of state at 6 hours.

Preferred sequencing: Sub-experiment A first (cheaper). If A passes quickly, try B only if there's time left in the budget to compare. If A fails, skip B — A's failure mode is more informative.

## Out of scope

- **Converting the other five activations** (Arbor, Cone, ClaudeCode, Loopback, Orcha). The spike only tests **one** activation's migration cost. The chosen path gets applied to the rest in DC-2 onward.
- **Changing any wire-level behavior.** The spike's success criterion is "compile and test clean" — not "ships any new feature".
- **Designing the lint rule** (for Option A) or **the workspace layout** (for Option B). Those designs are DC-2's work. The spike just measures the cost of either path.
- **Feature flags** for conditionally-compiled activations. Orthogonal concern.

## Completion

Spike delivers:

- Throwaway branch(es) with the sub-experiment code.
- A one-page report (added to this ticket body or linked from it) stating: which option passed, measured wall-clock time, number of file modifications, and any obstacles encountered.
- A pinned decision: "DC-2 will proceed with convention-only" OR "DC-2 will proceed with workspace-split".
- README's "Open coordination question #1" updated with the outcome.
- No merge to main. DC-2 inherits the findings and references them in its Context section before being promoted to Ready.
