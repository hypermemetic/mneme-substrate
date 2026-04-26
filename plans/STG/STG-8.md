---
id: STG-8
title: "Formalize Mustache migration: MustacheStore + test fixtures + ST integration"
status: Pending
type: implementation
blocked_by: [STG-2]
unlocks: [STG-10]
severity: Low
target_repo: plexus-substrate
---

## Problem

STG-2 absorbed STG-S01's and STG-S02's spike work, landing Mustache's `MustacheStore` trait, `SqliteMustacheStore`, and `InMemoryMustacheStore` as the pattern reference. This ticket is the formalization pass: apply any follow-ups that emerge from STG-2's pattern doc to Mustache, thread ST newtypes (`TemplateId`) through the trait signatures when/if ST has landed them, and publish `InMemoryMustacheStore` as a test fixture for STG-10's end-to-end harness.

This ticket is deliberately small. Its scope is cleanup + ST integration, not re-doing the migration.

## Context

Target file set: `src/activations/mustache/` (activation.rs, mod.rs, storage.rs, types.rs — and whatever layout STG-2 pinned).

Pattern doc: `docs/architecture/<nanotime>_storage-trait-pattern.md` (STG-2).

**Cross-epic inputs:**

- **ST newtypes** relevant to Mustache: `TemplateId` (pinned in `plans/README.md` as wrapping `String`). If ST has landed `TemplateId` at the time this ticket is promoted, thread it through `MustacheStore`'s method signatures (replacing the current `name: &str` parameter). If ST has not landed `TemplateId`, accept a bare-string interim — document in the PR.
- **`plans/README.md`** pins `MustacheStore` exactly and lists `TemplateId` in the newtypes table.

## Required behavior

- Audit `src/activations/mustache/` against STG-2's pattern doc migration checklist. Apply any items not already landed in STG-2 (e.g., doc comments, re-export layout, test harness consistency).
- If ST has landed `TemplateId`: update `MustacheStore`'s trait method signatures to consume `&TemplateId` where `name: &str` currently appears. Update both backends and all tests.
- Confirm `InMemoryMustacheStore` is exposed with whatever feature gate STG-10 requires to wire it into the integration harness.
- Re-verify the full Mustache test suite passes against both backends.

## Regression guarantees

| Surface | Post-ticket behavior |
|---|---|
| Mustache Plexus RPC methods | Unchanged. |
| Mustache on-disk SQLite schema | Unchanged. |
| Template resolution semantics | Unchanged. |
| All existing Mustache tests | Pass against both backends. |

## Risks

| Risk | Mitigation |
|---|---|
| STG-2 landed everything; this ticket has no material work. | That's acceptable. If nothing is needed, the ticket closes with a trivial PR (or merges into STG-10). The implementor decides based on state at promotion time. |
| ST's `TemplateId` newtype introduces ripple changes in activation method signatures. | Scope this ticket to the storage trait boundary. If activation surface needs ST threading, it's a separate ST-epic ticket. |

## What must NOT change

- Any Plexus RPC method on Mustache.
- SQLite schema or file path.
- Template resolution semantics.
- Any file outside `src/activations/mustache/` unless thread-through of `TemplateId` reaches further.

## Acceptance criteria

1. `cargo build -p plexus-substrate` succeeds.
2. `cargo test -p plexus-substrate` — all pre-epic tests pass.
3. `cargo test -p plexus-substrate --features test-doubles` — Mustache's test suite runs against both backends, all green.
4. If `TemplateId` has landed via ST: `MustacheStore` signatures use `&TemplateId` (or similar owning the domain type). Confirm via source inspection in the PR description.
5. `InMemoryMustacheStore` is exposed in a way that STG-10's integration harness can consume it.
6. `src/activations/mustache/` conforms to STG-2's pattern doc checklist.

## Completion

- PR against `plexus-substrate` with whatever formalization deltas are needed (possibly small, possibly zero).
- PR description explicitly states whether ST's `TemplateId` was threaded through or deferred.
- Ticket status flipped from `Ready` → `Complete` in the same commit as the code.
