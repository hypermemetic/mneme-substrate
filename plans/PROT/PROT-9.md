---
id: PROT-9
title: "downstream audit: bump plexus-core pins to 0.6 across sibling workspace crates"
status: Pending
type: implementation
blocked_by: [PROT-2, PROT-3, PROT-4]
unlocks: [PROT-10]
severity: Medium
target_repo: multiple
---

## Problem

Every workspace crate that pins `plexus-core`, `plexus-macros`, or `plexus-transport` at 0.5.x hits the dual-version-in-graph failure mode once PROT-2/3/4 ship 0.6.x. HF-0 hit this exact scenario with hyperforge; the version-bump memory (`feedback_version_bumps_as_you_go.md`) codified the audit sweep discipline. PROT-9 applies the sweep for the 0.5 → 0.6 rollover.

Known sibling drift (from earlier surveys):
- plexus-locus: pinned plexus-core 0.3 pre-PROT; 16 real migration errors flagged in HF-AUDIT-2 (not just a pin bump).
- mono-provider, plexus-music-royalty-free, plexus-mono: pinned plexus-core 0.4 per HF-AUDIT-1.
- axon, fidget-spinner, gitvm, jsexec, plexus-listen, plexus-ir, plexus-rust-codegen, plexus-schemars-compat, plexus-comms, plexus-registry, plexus-derive: various 0.4-0.5 pins per the initial survey.

**Codegen tools (first-class callouts):**
- **hub-codegen** (Rust, v0.4.0): mirrors plexus-core schema types in `src/ir.rs` (`MethodSchema`, `MethodRole`, `ParamSchema`, `DeprecationInfo`). Direct plexus-core dep. **Must bump to 0.6.** TypeScript generator templates may have `method_schema` content-type handling — grep and migrate. The generated TypeScript clients it emits become slightly simpler post-PROT (one response type for `.schema` calls).
- **synapse-cc** (Haskell, v0.2.0): depends on `plexus-protocol`, `synapse`, `synapse-types`. **Must bump cabal deps** to plexus-protocol 0.6.0.0 + synapse 4.0.0. Grep for `SchemaResult.*Method` or `method_schema` in its Haskell source; migrate. Its generated `@plexus/client` TypeScript code consumes schemas at runtime — re-run its test harness against a PROT-upgraded backend.
- **Downstream generated-client projects**: any repo with committed output from hub-codegen or synapse-cc may have method-schema-specific type definitions that diverge from the new unified shape. Wire-level method invocation is unchanged, so nothing hard-breaks, but introspection-heavy clients may need regenerating. Track as per-project follow-ups.

## Context

This ticket audits every Rust sibling in the workspace, bumps pins where the fix is cheap, and files audit tickets where the fix requires source-level migration (HF-AUDIT-2-style).

## Required behavior

1. **Grep** every `Cargo.toml` under `/Users/shmendez/dev/controlflow/hypermemetic/` for `plexus-core`, `plexus-macros`, `plexus-transport` deps. Also grep every `*.cabal` for `plexus-protocol`. Build a table: repo / language / current pin / target pin / estimated difficulty.

   Codegen tools get their own rows:
   - `hub-codegen` (Rust): audit `src/ir.rs` for `SchemaResult::Method` or `method_schema`; bump cargo deps; rebuild + test.
   - `synapse-cc` (Haskell): audit source for `SchemaResult.*Method`; bump cabal deps; rebuild + test harness against a PROT-upgraded backend.

2. **Cheap bumps** (pin-only, no source changes): update the Cargo.toml pin, run `cargo build`, verify green. Commit. Patch-bump the sibling's own version. Tag.

3. **Expensive bumps** (source migration needed): file a follow-up ticket per repo in `plans/HF-AUDIT/HF-AUDIT-N.md`. Do NOT attempt the migration in this ticket. Examples (known already):
   - plexus-locus (HF-AUDIT-2 — already filed).
   - Any other discovered during the sweep.

4. **Report** — commit body enumerates:
   - Cheap bumps applied (with commit SHAs and version bumps per crate).
   - Expensive bumps deferred (with ticket IDs).
   - Total delta: N cheap, M deferred.

5. **No-op crates**: if a crate's current pin is `*` or already accepts 0.6 ranges, note it but don't bump the crate's own version.

## Risks

| Risk | Mitigation |
|---|---|
| A "cheap" bump reveals a source-level incompatibility once you actually cargo build. | Demote to "expensive", file audit ticket, move on. |
| Bumping N crates in a single commit is hard to review. | One commit per repo (N commits). Each repo is self-contained. |
| A sibling crate has a `rustflags = ["-D", "dead-code"]` or similar strict lint that trips on macro-generated code from the bumped plexus-macros. | Same as plexus-locus's symptom. Either narrow the rustflags or defer the migration. |
| HF-AUDIT-1 (mono-provider family) and HF-AUDIT-2 (plexus-locus) are already Pending; don't duplicate. | This ticket's report references those by ID rather than filing new ones. |

## What must NOT change

- Any sibling's source code beyond Cargo.toml pins (for cheap bumps).
- Any sibling's public API.
- Any sibling's tagging convention (each uses their own `<crate>-v<version>` pattern).

## Acceptance criteria

1. Every Rust sibling's `Cargo.toml` either (a) pins plexus-* at 0.6+ with a green build, or (b) has a filed HF-AUDIT-N ticket explaining why not.
2. `cargo tree -d` at each bumped sibling shows a single version of each plexus-* crate.
3. Commit body summarizes: N cheap bumps applied, M deferred with ticket IDs, total siblings surveyed.
4. No sibling's build status regresses from whatever it was pre-PROT-9.

## Completion

This ticket is purely tracking — the actual work is per-repo commits. Status flipped to Complete when all siblings are either (a) bumped and green, or (b) have their migration tracked in an audit ticket.
