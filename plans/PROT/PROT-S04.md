---
id: PROT-S04
title: "Spike: workspace-wide grep for direct SchemaResult::Method usage"
status: Pending
type: spike
blocked_by: []
unlocks: [PROT-2]
severity: Medium
target_repo: multiple
---

## Problem

PROT-2 removes `SchemaResult::Method` from plexus-core. PROT-1 asserts "no external consumers pinning 0.5.x" but I haven't actually verified. Any Rust crate in the workspace that directly pattern-matches on this variant breaks at compile time.

A 10-minute grep produces a definitive list. PROT-S04 is that grep.

## Required behavior

1. Run:
   ```
   grep -rn 'SchemaResult::Method\|SchemaResult::Plugin\|SchemaResult{' \
     --include='*.rs' \
     /Users/shmendez/dev/controlflow/hypermemetic/
   ```

2. Run:
   ```
   grep -rn 'SchemaResult' --include='*.hs' \
     /Users/shmendez/dev/controlflow/hypermemetic/
   ```
   (Haskell consumers — synapse, plexus-protocol, synapse-cc.)

3. Produce a table: file:line / language / current usage / migration impact.

4. **Categorize** each consumer:
   - **Macro emission**: plexus-macros itself. Fixed by PROT-3.
   - **Direct pattern match**: needs migration in the consumer's PR.
   - **Serde round-trip only**: probably no code change needed post-PROT-2 (the removed variant simply stops appearing in responses).
   - **Re-export**: PROT-2 drops the re-export; consumers that re-export from plexus-core break.

5. **File per-consumer tickets** if any consumer needs non-trivial migration beyond a pin bump.

## Acceptance criteria

1. Complete table of all `SchemaResult` references across the workspace.
2. Each reference categorized per the list above.
3. Per-consumer tickets filed for any non-trivial migration.
4. PROT-2's "Risks" section updated with the concrete consumer list.

## Completion

Spike concludes with a documented inventory. Implementation tickets (PROT-2, PROT-3, etc.) use this to bound their scope precisely.
