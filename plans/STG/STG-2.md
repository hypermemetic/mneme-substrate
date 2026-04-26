---
id: STG-2
title: "Foundation: storage trait pattern + shared template module"
status: Pending
type: implementation
blocked_by: [STG-S01, STG-S02]
unlocks: [STG-3, STG-4, STG-5, STG-6, STG-7, STG-8, STG-9]
severity: High
target_repo: plexus-substrate
---

## Problem

STG-S01 and STG-S02 prove that the per-activation `*Store` trait shape works in principle. This ticket establishes the canonical pattern that STG-3 through STG-9 apply: how the trait is defined, how the SQLite backend is structured, how the in-memory backend is gated (feature flag, `#[cfg(test)]`, or separate type), how activation constructors consume `Arc<dyn *Store>`, and how tests exercise both backends.

Without a single source of truth for the pattern, the seven migration tickets risk drifting into seven different shapes — defeating the maintainability argument for the whole epic.

## Context

Target crate: `plexus-substrate` at `/Users/shmendez/dev/controlflow/hypermemetic/plexus-substrate`.

This ticket's implementor reads the STG-S01 and STG-S02 spike reports first. The spikes produce findings on:

- Async trait mechanism (`async_trait` macro vs native async-fn-in-trait).
- Trait-object `Send + Sync` bounds.
- Test parameterization (duplicate tests per backend vs harness).
- Any contract gaps between SQLite and in-memory semantics that needed tightening.

The foundation ticket codifies these findings into an explicit pattern document and a shared helper module.

**Existing shared storage module:** `src/activations/storage.rs` (currently ~small file with `init_sqlite_pool` and the `activation_db_path_from_module!` macro). This is the natural home for new shared pieces.

**Cross-epic inputs:**

- **ST newtypes** for `TemplateId` land separately. The foundation pattern shows trait signatures using newtypes generically (e.g., "ID types are domain newtypes when ST has landed them; bare `String`/`Uuid` as a transition").
- **`plans/README.md`** pins trait names and the "not a generic KV" rule.

## Required behavior

Deliver three artifacts:

### 1. A shared helpers / conventions module

At `src/activations/storage.rs` (or a new `src/activations/storage/` subdirectory), add:

- Re-exports or shared items supporting the pattern (e.g., a `StoreBackendKind` enum for diagnostics, if useful; an `in_memory_timestamp()` helper shared across in-memory backends if a common concern emerges).
- No trait definitions live here — traits live with their activation. This module is plumbing only.

### 2. A documented pattern in a short markdown note

At `docs/architecture/<nanotime>_storage-trait-pattern.md` (per substrate's reverse-chronological naming convention — see `CLAUDE.md`). Contents:

| Section | What it pins |
|---|---|
| Trait shape | Exact form a `*Store` trait takes: `pub trait FooStore: Send + Sync { ... }`, one `async fn` per current storage operation, mirroring today's concrete methods. |
| Async mechanism | `async_trait` vs native async fn in trait — pin the winner from STG-S01. |
| Backend impls | How SQLite and in-memory backends are named (`SqliteFooStore`, `InMemoryFooStore`) and where they live (trait module). |
| In-memory gating | `#[cfg(any(test, feature = "test-doubles"))]` on the in-memory type, or an unconditional `pub` type — pin the decision. |
| Activation constructor | Signature: `pub async fn new(store: Arc<dyn FooStore>) -> Result<Self, Error>`. Plus `with_sqlite(config) -> Result<Self, Error>` convenience. Plus `with_memory() -> Self` (cfg-gated) convenience. |
| Test parameterization | Exact form — the one chosen by STG-S02. Example with a harness function or a test-macro if that's the pick. |
| Migration checklist | Step-by-step: what to change in `storage.rs`, in `activation.rs`, in the mod-level public API, and in tests. This is the template STG-3..9 follow. |

The doc is linked from `CLAUDE.md`'s "Key Architecture Documents" section in the same commit.

### 3. Mustache migrated to the final pattern (promoting STG-S01+S02's spike work)

Land the spike work as production code:

- `MustacheStore` trait is public in `src/activations/mustache/store.rs` (or `storage.rs` — pin the file layout in the pattern doc).
- `SqliteMustacheStore` is the production default.
- `InMemoryMustacheStore` is available for tests (per the in-memory gating decision above).
- `Mustache::new(store: Arc<dyn MustacheStore>)` is the primary constructor.
- `Mustache::with_sqlite(config)` preserves the pre-epic default path for `builder.rs`.
- The Mustache tests run against both backends per the pattern.

This duplicates STG-8's scope at the spike level — STG-8 will then be a small formalization ticket (update docs, ensure ST newtypes are threaded through, final cleanup).

## Regression guarantees

| Surface | Post-ticket behavior |
|---|---|
| `cargo build -p plexus-substrate` | Succeeds. |
| `cargo test -p plexus-substrate` | All pre-epic tests green. |
| `builder.rs` default startup | Produces a fully-functional substrate with SQLite-backed Mustache — zero behavior change from the user's perspective. |
| Mustache Plexus RPC surface | Unchanged. |
| On-disk template DB file | Same path, same schema. |

## Risks

| Risk | Mitigation |
|---|---|
| Pattern doc written after the spike findings drift from the code. | Land doc + shared module + Mustache migration in one commit. |
| `#[cfg(test)]` on `InMemoryMustacheStore` means STG-10 cannot wire it into a test binary. | Use a `test-doubles` feature flag rather than `#[cfg(test)]` — enabled in the dev-dependencies cycle and by STG-10's integration harness. Pin this in the pattern doc. |
| Activation constructors gain two convenience methods (`with_sqlite`, `with_memory`) but existing callers use the old signature. | STG-2 updates `builder.rs` to use `with_sqlite` (identical behavior); subsequent migration tickets do the same for their activation. |

## What must NOT change

- The SQLite file path for Mustache's database.
- Any Plexus RPC method exposed by Mustache.
- Any wire-format types.
- Tests that existed pre-epic continue to pass.
- `builder.rs`'s default startup produces an identically-configured substrate.
- Other activations (Arbor, Orcha, Lattice, ClaudeCode, Cone, MCP session) are untouched by this ticket — their migration is STG-3..9.

## Acceptance criteria

1. `cargo build -p plexus-substrate` succeeds.
2. `cargo test -p plexus-substrate` succeeds — all pre-epic tests pass.
3. `cargo test -p plexus-substrate --features test-doubles` (or the chosen gating mechanism) succeeds — Mustache tests run against both backends.
4. `src/activations/mustache/` exposes `MustacheStore` (trait), `SqliteMustacheStore` (impl), and `InMemoryMustacheStore` (impl, gated) as public items.
5. The architecture doc at `docs/architecture/<nanotime>_storage-trait-pattern.md` exists, is linked from `CLAUDE.md`'s "Key Architecture Documents" section, and contains the seven sections listed in "Required behavior" part 2.
6. `Mustache::with_sqlite(MustacheStorageConfig::default())` produces behavior indistinguishable from pre-epic `Mustache::with_defaults()` — verified by an integration test that exercises `register_template_direct` → `get_template` round trip.
7. An explicit migration-checklist section in the doc is cross-referenced by STG-3..9's body as the template to follow.

## Completion

- PR against `plexus-substrate` landing the pattern doc, the shared module changes, and Mustache's migration (inheriting the spike work).
- PR description includes `cargo build -p plexus-substrate` and both test-feature-gated runs — all green.
- PR notes that STG-3, STG-4, STG-5, STG-6, STG-7, STG-8, STG-9 are unblocked.
- Ticket status flipped from `Ready` → `Complete` in the same commit as the code.
