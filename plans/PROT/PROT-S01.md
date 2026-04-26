---
id: PROT-S01
title: "Spike: ratify SchemaResult migration shape across 6 crates (plexus-core, plexus-macros, plexus-protocol, synapse, hub-codegen, synapse-cc)"
status: Pending
type: spike
blocked_by: []
unlocks: [PROT-2]
severity: Critical
target_repo: multiple
---

## Problem

PROT-2 defers the decision "`SchemaResult::Method` variant removal — flatten vs newtype vs single-variant enum" to the implementer. That decision cascades across six artifacts:

1. plexus-core (Rust type definition).
2. plexus-macros (generated code referencing the type).
3. plexus-protocol (Haskell aeson instances).
4. synapse (parser decoding the wire response).
5. hub-codegen (IR mirroring the type).
6. synapse-cc (Haskell decoders + TypeScript template output).

If the implementer picks, say, "flatten to `PluginSchema` directly" for plexus-core but plexus-protocol's author keeps `SchemaResult` as a single-variant enum, the wire format diverges. Round-trip fails. Bug.

PROT-S01 pins the decision upfront.

## Context

The three shapes under consideration:

| Option | plexus-core | Wire JSON | Change impact |
|---|---|---|---|
| (a) Flatten | `.schema` returns `PluginSchema` directly; `SchemaResult` type deleted. | `{"namespace": ..., "methods": [...], "children": [...], ...}` | Every consumer of `SchemaResult` gets renamed to `PluginSchema`. Broader rename churn. |
| (b) Newtype alias | `type SchemaResult = PluginSchema;` or `pub type SchemaResult = PluginSchema;` | Same as (a). | Minimal churn — existing `SchemaResult` references keep working via the alias. |
| (c) Single-variant enum | `enum SchemaResult { Plugin(PluginSchema) }` — Method variant removed. | `{"Plugin": {...}}` (tag-wrapped). | Wire format DIFFERS from (a)/(b): still tag-wrapped. Breaks any consumer expecting the unwrapped shape. |

The wire format implication is the key decider. (a) and (b) both emit unwrapped JSON. (c) emits tagged JSON. For consumer simplicity, (a) or (b) is better.

Between (a) and (b): (b) is less churn. But it leaves `SchemaResult` as a vestigial name — future readers wonder why the alias exists. (a) is honest: the concept is gone.

## Required behavior

1. **Read** each of the 6 codebases' current handling of `SchemaResult`:
   - plexus-core: grep `src/plexus/plexus.rs` and nearby for `SchemaResult` definition + usage.
   - plexus-macros: grep `src/codegen/activation.rs` for `SchemaResult` emission.
   - plexus-protocol: grep Haskell sources for `SchemaResult` ADT + `ToJSON`/`FromJSON` instances.
   - synapse: grep for `SchemaResult` pattern matches, parse logic.
   - hub-codegen: grep `src/ir.rs` and templates for schema-response handling.
   - synapse-cc: grep for `SchemaResult` + any TypeScript template interpolations.

2. **Decide** which shape (a/b/c) minimizes churn while maintaining wire-format unification. Default recommendation: (a) flatten. Document the choice in the commit body with the reasoning.

3. **Pin the wire JSON exactly** in this spike's output, e.g., "every `.schema` response is a PluginSchema serialized with default serde rules, content_type suffix `.schema`, no outer tag wrapper."

4. **Prototype in a branch** (optional but recommended): apply the chosen migration to plexus-core only. Run `cargo check` in every workspace crate that directly imports `SchemaResult`. Verify the blast radius matches expectation.

5. **Update PROT-2 through PROT-6** with the ratified decision before any implementation ticket is promoted Ready.

## Risks

| Risk | Mitigation |
|---|---|
| Different consumers converge on different shapes despite this spike. | Spike output is written to PROT-2/5 tickets verbatim; any divergence is flagged in implementation review. |
| The decision turns out wrong post-implementation. | Spikes have known unknowns. If option (a) is chosen and introduces unexpected breakage, rollback cost is one commit in plexus-core. |

## Acceptance criteria

1. A decision document: option (a / b / c), reasoning, impact per crate.
2. Updated text in PROT-2 and PROT-5 matching the ratified choice.
3. Example of the chosen wire JSON format (committed as a fixture if helpful).
4. No actual code changes in this spike — decisions only.

## Completion

Spike concludes with a decision pinned in PROT-2's context. Status flipped Complete; PROT-2 is now unblocked to implement the pinned choice.
