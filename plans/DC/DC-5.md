---
id: DC-5
title: "Decouple Cone from concrete Bash via BashClient"
status: Pending
type: implementation
blocked_by: [DC-2]
unlocks: [DC-7]
severity: Medium
target_repo: plexus-substrate
---

## Problem

Cone reaches into Bash's concrete activation struct to check handle plugin IDs. `cone/activation.rs:8` imports `Bash`, and `cone/activation.rs:658` compares `handle.plugin_id == Bash::PLUGIN_ID` to decide whether a handle is a Bash resource. This couples Cone to Bash's concrete struct — and worse, to a raw `PLUGIN_ID` constant on that struct. Any change in how Bash identifies itself (refactor from a constant to a method, namespace rename, etc.) breaks Cone.

The coupling is smaller than DC-3/DC-4 (one import site, one usage), but the same library-hygiene principle applies: Cone should not know about Bash's internal `PLUGIN_ID` constant. If Cone needs to check "is this handle a Bash handle?", that's a library-level question with a library-API answer.

## Context

**The specific coupling (re-verify against HEAD; audit drift caveat applies):**

- `src/activations/cone/activation.rs:8` — `use crate::activations::bash::Bash;`
- `src/activations/cone/activation.rs:658` — `} else if handle.plugin_id == Bash::PLUGIN_ID {`

**What Cone is actually doing.** Inside Cone's handle-dispatch logic, it needs to distinguish Bash handles from other handle kinds to route operations correctly. The current pattern reaches for the raw plugin ID. The library-API fix is Bash exposing this check as a sanctioned function.

**Two shape options for the library API** (implementor picks at implementation time based on whether there are other similar checks elsewhere):

**Option A — A predicate function.** `bash::is_bash_handle(handle: &Handle) -> bool` re-exported from Bash's entry point. Minimal surface.

**Option B — A client handle.** `BashClient` exposing the operations Cone actually uses. If the only cross-activation touch is "is this mine?", Option A is cheaper. If Cone calls into Bash for other operations (executing a command against a resource, etc.), Option B is the shape that scales.

**Recommendation (pin in DC-5's commit):** check the full grep sweep of `cone/` and `orcha/` for other reaches into Bash. If the `PLUGIN_ID` check is the only one, ship Option A. If there are others (now or imminent), ship Option B and put the predicate on the client.

**`PLUGIN_ID` fate.** In either option, `Bash::PLUGIN_ID` becomes `pub(crate)` or private. It's an internal identity constant. The sanctioned function uses it internally.

## Required behavior

**Option A (predicate-only):**

| Operation | Current shape | New shape |
|---|---|---|
| Test handle is Bash | `handle.plugin_id == Bash::PLUGIN_ID` | `bash::is_bash_handle(&handle)` |

**Option B (client handle):**

| Operation | Current shape | New shape |
|---|---|---|
| Test handle is Bash | `handle.plugin_id == Bash::PLUGIN_ID` | `bash_client.owns_handle(&handle)` or similar |
| Any future library-level Bash call from a sibling | (not applicable today) | `bash_client.<operation>(...)` |

**Cone side (either option):**

| Before | After |
|---|---|
| `use crate::activations::bash::Bash;` | `use crate::activations::bash::{is_bash_handle};` (A) OR `use crate::activations::bash::BashClient;` (B) |
| `Bash::PLUGIN_ID` usage | Function or method call |

**Bash's `mod.rs` after DC-5:**
- Re-exports `is_bash_handle` or `BashClient` (one of them; not both).
- `Bash::PLUGIN_ID` is demoted to `pub(crate)` or private.
- Concrete `Bash` struct remains re-exported (needed by `builder.rs`), but Cone imports only the library-API item.

## Risks

- **Other sibling reaches into Bash not yet discovered.** The audit documented only Cone's reach-in. Before starting, the implementor runs `grep -rn "use crate::activations::bash::" src/activations/` outside `src/activations/bash/**` to confirm Cone is the only reacher. If other activations have grown reach-ins since the audit, they're absorbed into DC-5's scope (not a new ticket — they're the same coupling category).
- **Tests in `cone/tests.rs` construct Bash directly.** Per the audit, Cone's tests instantiate a Bash. That's acceptable within the **test** scope (test code may reach for constructors across activation boundaries), but if the test is checking the same `PLUGIN_ID` equality, it migrates to the library API too. Verify during implementation.
- **File collision with DC-6.** DC-6 also touches `cone/activation.rs`. Cannot land in parallel with DC-5. Pin order: DC-5 first, DC-6 second.

## What must NOT change

- Bash's wire-level RPC methods — request/response shapes identical.
- Bash's handle registration with Arbor (the `PLUGIN_ID` value itself is unchanged; only its visibility changes).
- Cone's handle-dispatch semantics — Bash handles are still routed to Bash the same way.
- Cone's wire API.
- Any passing Cone or Bash test.

## Acceptance criteria

1. `grep -rn "use crate::activations::bash::Bash" src/activations/cone/` returns zero results.
2. `grep -rn "Bash::PLUGIN_ID" src/activations/` returns zero results outside `src/activations/bash/**`.
3. Bash's `mod.rs` exposes the chosen library-API item (`is_bash_handle` or `BashClient`) with a library-API doc comment.
4. `Bash::PLUGIN_ID` is marked `pub(crate)` or private.
5. `cargo test --workspace` passes with zero test failures.
6. Cone's handle-dispatch behavior for Bash handles is unchanged — verified by Cone's existing test suite, re-run and green.
7. Commit message states which option (A or B) was chosen and why.

## Completion

Implementor delivers:

- Commit introducing the library-API item in Bash (`is_bash_handle` or `BashClient`).
- Commit migrating Cone's import and the single usage site.
- Commit demoting `Bash::PLUGIN_ID` to `pub(crate)`.
- `cargo test` output showing green.
- Before/after `grep` output for the import-leak criteria.
- Commit message notes Option A vs Option B with rationale.
- Status flip to `Complete` in the commit that lands the work.
