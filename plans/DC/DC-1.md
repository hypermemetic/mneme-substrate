---
id: DC-1
title: "Decoupling — curated library APIs across substrate activations"
status: Epic
type: epic
blocked_by: []
unlocks: []
target_repo: plexus-substrate
---

## Goal

End state: every substrate activation exposes a deliberate **library API** (a curated set of re-exported types, traits, constructors, and client handles) and keeps everything else `pub(crate)` or below. Other activations inside the same substrate binary call siblings through this library API — they do **not** reach into sibling storage structs, schema enums, internal session types, or concrete activation structs beyond what the library surface sanctions. Per-activation compilation posture is preserved: removing one activation should not break sibling compilation beyond the clearly-documented library contract.

This epic fixes the four coupling sites the technical-debt audit called out:

1. **Orcha → Loopback storage.** `orcha/graph_runner.rs` imports `LoopbackStorage` (a storage struct) and queries approval state as if it owned the table.
2. **Orcha → ClaudeCode concrete.** `orcha/activation.rs` and `orcha/graph_runner.rs` import the concrete `ClaudeCode` activation struct plus the `Model` enum directly.
3. **Cone → Bash concrete.** `cone/activation.rs` imports `Bash` to read `Bash::PLUGIN_ID`.
4. **Cone / ClaudeCode / Orcha → Arbor schema walking.** Three sibling activations pattern-match on Arbor's `NodeType` enum and thread `NodeId` / `TreeId` through their internals. A schema change in Arbor ripples through three sibling call sites.

The fix shape (pinned per `feedback_activation_coupling.md` in memory): in-process Rust library calls against a curated public API. **Not** hub-routed RPC. **Not** trait-indirection for its own sake. The goal is library hygiene — narrow the public surface, demote internals to `pub(crate)`, and call siblings against the intentional surface only.

## Context

**Single-crate vs. workspace-split — the open question.** Substrate is currently a single crate. `pub(crate)` inside a single crate does not enforce activation boundaries (every module in the crate can see every other module's `pub(crate)` items). Two options:

- **A. Convention-only.** Keep substrate single-crate. Each activation exposes its library API in its `mod.rs` and documents internals as `pub(crate)` for Rust's module hierarchy, plus a lint or rustdoc hygiene check that fails CI when a sibling imports a non-re-exported item. User leaned toward this option first (per `feedback_activation_coupling.md`).
- **B. Workspace-split.** Split each activation into its own crate under a Cargo workspace. `pub(crate)` at the crate level becomes compiler-enforced. Higher migration cost, stronger guarantees. Reserved for later if drift recurs under convention-only.

**DC-S01 is the spike that decides** between A and B — binary pass condition in the spike ticket.

**Library-API convention (pinned for both branches of the spike).** Each activation `mod.rs` (or a dedicated `api.rs`) is the single entry point. Re-exports from it are the library API. Everything else in the activation is `pub(crate)` at most. Siblings importing anything not re-exported from the entry point is a hygiene violation.

The audit file drift note applies: audit file:line references may have drifted since 2026-04-16. Each implementation ticket (DC-3..DC-6) re-verifies the specific reach-in sites against HEAD before proceeding — the **categories** of coupling are durable; the exact line numbers are not.

## Dependency DAG

```
          DC-S01 (spike: convention vs workspace)
                │
                ▼
          DC-2 (library-API conventions per activation)
                │
      ┌─────────┼─────────┬─────────┐
      ▼         ▼         ▼         ▼
    DC-3      DC-4      DC-5      DC-6
  (Orcha→   (Orcha→   (Cone→   (schema-
   Loopback) ClaudeCode) Bash)   walking)
      │         │         │         │
      └─────────┴────┬────┴─────────┘
                    ▼
                  DC-7 (lint / enforcement)
```

- **DC-S01** gates everything: its pass/fail picks the enforcement mechanism used in DC-2 and DC-7.
- **DC-2** is the foundation: it pins what "library API" means per activation (Orcha, ClaudeCode, Loopback, Bash, Cone, Arbor) and sets the convention files will follow.
- **DC-3, DC-4, DC-5, DC-6** are parallel. Each targets a distinct coupling site with disjoint file scope:
  - DC-3: `orcha/graph_runner.rs`, `claudecode_loopback/mod.rs`, new `LoopbackClient` module.
  - DC-4: `orcha/activation.rs`, `orcha/graph_runner.rs`, `orcha/orchestrator.rs`, `claudecode/mod.rs`, new `ClaudeCodeClient` module.
  - DC-5: `cone/activation.rs`, `bash/mod.rs`, new `BashClient` module.
  - DC-6: `arbor/mod.rs` (new traits), consumers in `cone/activation.rs`, `claudecode/storage.rs`, `claudecode/render.rs`, `orcha/graph_runtime.rs`, `orcha/context.rs`.
  - File-boundary check: DC-4 and DC-6 both touch `orcha/graph_runner.rs`. They are **file-collision concurrent** — land DC-4 first, then DC-6 against the resulting file.
  - File-boundary check: DC-5 and DC-6 both touch `cone/activation.rs`. Same rule — DC-5 first.
- **DC-7** lands last. It encodes the hygiene rule as a mechanical check so regressions fail CI.

## Phase Breakdown

| Phase | Tickets | Notes |
|---|---|---|
| 0. Decision | DC-S01 | Binary spike: convention-only vs workspace split. Result pins DC-2 and DC-7 shape. |
| 1. Foundation | DC-2 | Define per-activation library-API entry point and downgrade internals to `pub(crate)`. Pure-additive scaffolding — no call-site changes yet. |
| 2. Decouple | DC-3, DC-4, DC-5, DC-6 | Parallel where file-disjoint. Each removes one category of sibling reach-in. |
| 3. Enforce | DC-7 | Lint / rustdoc / CI mechanism prevents regression. |

## Tickets

| ID | Summary | Status |
|---|---|---|
| DC-1 | This epic overview | Epic |
| DC-S01 | Spike: convention-only vs workspace split for boundary enforcement | Pending |
| DC-2 | Define library-API conventions and entry points for every activation | Pending |
| DC-3 | Decouple Orcha from `LoopbackStorage` via `LoopbackClient` | Pending |
| DC-4 | Decouple Orcha from concrete `ClaudeCode` / `Model` via `ClaudeCodeClient` | Pending |
| DC-5 | Decouple Cone from concrete `Bash` via `BashClient` | Pending |
| DC-6 | Decouple schema-walking from Cone / ClaudeCode / Orcha via Arbor library traits | Pending |
| DC-7 | Lint / rustdoc hygiene check prevents boundary-violation regressions | Pending |

## Out of scope

- **Hub-routed RPC for intra-substrate calls.** User explicitly directed against this — see `feedback_activation_coupling.md`. Library calls stay in-process. The only shape change is narrowing the public surface.
- **Strong-typing migration (ST epic).** DC does not introduce `SessionId`, `GraphId`, `NodeId`, `StreamId`, `ApprovalId` newtypes. Those are ST's scope. DC inherits whatever bare types exist at the time of execution; post-ST, DC's library-API signatures will use the newtypes, but DC itself does not drive that change.
- **Storage abstraction (STG epic).** DC does not introduce `ArborStore` / `OrchaStore` / `LatticeStore` traits. Those are STG's scope. DC defines per-activation **library** APIs (client handles, re-exported domain types); STG defines per-activation **storage** traits.
- **Removing activations from the default substrate build.** Orcha still depends on ClaudeCode and Loopback at the Cargo level — DC does not introduce feature flags to compile Orcha without them. That's a future epic if it surfaces.
- **Cross-repo activation coupling.** Only substrate-internal activation boundaries are in scope. Third-party activations that link against substrate inherit the library APIs DC pins; their own hygiene is out of scope.

## Cross-epic references

- **README pinned decision.** DC's convention-vs-workspace call (question 1 in README's "Open coordination questions") is resolved by DC-S01. Pin the outcome back into the README as soon as S01 lands.
- **`feedback_activation_coupling.md` in memory.** This epic operationalizes that feedback. The library-API convention pinned in DC-2 matches the three-layer model in that feedback file: (1) library API, (2) wire API, (3) internal.
- **Audit document** (`docs/architecture/16670380887168786687_substrate-technical-debt-audit.md`). Section "Activation coupling is the biggest debt" enumerates the four coupling categories DC-3..DC-6 address.
- **ST epic**. If ST ships before DC's implementation phase starts, DC's library-API signatures use ST's newtypes directly. If DC ships first, ST's per-activation migration tickets update the already-narrow surface without changing other call sites.
- **STG epic**. STG's `ArborStore` / `OrchaStore` / etc. traits will be declared in the same `mod.rs` entry points DC-2 establishes. Entry points are DC's contribution; the traits themselves are STG's.

## What must NOT change

- Wire-level RPC behavior. Every `#[plexus_macros::method]` on every activation continues to serve the same request/response shape. DC is a source-code refactor; the external surface is untouched.
- Activation startup order in `builder.rs`. Cyclic-parent injection via `OnceLock<Weak<DynamicHub>>` is unchanged.
- SQLite-per-activation layout. DC does not touch `~/.plexus/substrate/activations/{name}/` paths, migrations, or schema.
- Existing `cargo test` pass rate. All currently-passing tests pass after every DC ticket lands.
- Activation namespace strings, method names, schema hashes.

## Completion

Epic is Complete when DC-S01 is Complete, DC-2 through DC-6 are all Complete, and DC-7 is Complete and the enforcement mechanism is running in CI. Deliverables:

- One entry-point module per activation (from DC-2), re-exporting only library-API items.
- Zero imports of `LoopbackStorage`, concrete `ClaudeCode`, concrete `Model`, or `Bash::PLUGIN_ID` from sibling activations (verified by grep after DC-3/DC-4/DC-5).
- Zero sibling `use crate::activations::arbor::{NodeType, ...}` outside Arbor-library-sanctioned call sites (verified by grep after DC-6).
- CI lint / rustdoc hygiene check fails a synthetic regression PR that re-introduces a reach-in (DC-7).
- README's "Open coordination question #1" (convention-vs-workspace) is updated with the S01 outcome.
