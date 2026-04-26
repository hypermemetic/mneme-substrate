---
id: DC-2
title: "Define library-API conventions and entry points for every activation"
status: Pending
type: implementation
blocked_by: [DC-S01]
unlocks: [DC-3, DC-4, DC-5, DC-6, DC-7]
severity: High
target_repo: plexus-substrate
---

## Problem

Substrate activations have no pinned contract for what "the library API of activation X" means. Each activation's `mod.rs` re-exports a mix of types, storage structs, error enums, and helpers chosen ad-hoc. Siblings importing from an activation cannot distinguish "intended for cross-activation use" from "happens to be `pub`". The decoupling work in DC-3..DC-6 depends on knowing where to draw the library-API line for each of the six activations that participate in sibling-coupling: **Orcha, ClaudeCode, Loopback, Bash, Cone, Arbor**.

## Context

**Inherits DC-S01's decision.** If S01 picks convention-only, DC-2 establishes a single entry-point module per activation with a doc-comment convention. If S01 picks workspace-split, DC-2 establishes per-crate public surfaces and cross-crate dependencies. The body of this ticket is written to apply to either outcome — specific entry-point file names differ between the two but the contract is the same.

**Library-API shape (pinned per `feedback_activation_coupling.md` in memory).** Each activation's library API consists of:

1. **The activation struct** (or a client handle wrapping it) — the type another activation holds to invoke library-level calls.
2. **The constructor signature** — how the activation is built. In the current cyclic-parent-injection model, the constructor takes `Weak<DynamicHub>` and returns `Arc<Self>`. If S01 picks workspace-split, this stays the same but is re-exported via the crate root.
3. **Domain types returned by library methods** — the user-facing enums, IDs, and shapes callers see. Internal storage rows, migration SQL, query helpers are NOT in this set.
4. **The error enum** — e.g., `BashError`, `OrchaError`, `ConeError`. Errors are library-API members because callers pattern-match on them.
5. **Any trait the activation intends siblings to implement** (e.g., if ClaudeCode wants to be polymorphic, it exposes `ClaudeCodeLike` — DC-4 decides). Pure library traits only — wire-level dispatch traits remain internal.

**Explicitly NOT in the library API.**
- Storage structs (`LoopbackStorage`, `ArborStorage`, etc.). Storage is an internal concern per the audit's SQLite-per-activation pattern.
- Database row types, migration helpers, index names.
- Session-management internals (`sessions.rs` types in ClaudeCode).
- Render helpers (`render.rs` in ClaudeCode).
- Internal test utilities.

**Current state (pre-DC):**

| Activation | Internals currently `pub` that should be narrowed |
|---|---|
| Orcha | `storage.rs` module (row types), `context.rs`, `orchestrator.rs`, `graph_runner.rs` types |
| ClaudeCode | `sessions.rs`, `render.rs`, `storage.rs`, `NodeEvent` (leaks to Arbor's `views.rs`) |
| Loopback | `storage.rs` including `LoopbackStorage` struct |
| Bash | `PLUGIN_ID` constant (sibling check in Cone reaches for this), `executor` submodule |
| Cone | internals mostly scoped; library API is narrow already |
| Arbor | `NodeType` enum, `Node` struct, `views` module, `ArborStorage` (shared with siblings — separate call in DC-6) |

## Required behavior

| Activation | Library-API entry point | Re-exports (library API) |
|---|---|---|
| Orcha | `orcha/mod.rs` (convention) or crate root (workspace) | Activation struct, constructor, `OrchaError`, graph/node/ticket domain types exposed by library methods |
| ClaudeCode | `claudecode/mod.rs` | Activation struct, constructor, `ClaudeCodeError`, `ChatEvent`, `CreateResult`, `Model`, `StreamId` — or the trait that DC-4 introduces in place of concrete types |
| Loopback | `claudecode_loopback/mod.rs` | Activation struct, constructor, `LoopbackError`, approval domain types (`ApprovalId`, `ApprovalStatus`, etc.) — NOT `LoopbackStorage` |
| Bash | `bash/mod.rs` | Activation struct, constructor, `BashError`, output/input domain types — NOT `PLUGIN_ID` as a raw constant; expose a sanctioned library function if Cone needs a handle-kind check |
| Cone | `cone/mod.rs` | Activation struct, constructor, `ConeError`, cone domain types (branch/operation types) |
| Arbor | `arbor/mod.rs` | Activation struct, constructor, `ArborError`, handle types (`ArborId`, trait surface defined in DC-6) — `NodeType` and internal `Node`/`TreeId` schema walking is NOT re-exported here (DC-6 replaces with library traits) |

For each activation:

- **When** the entry-point module is compiled, **it re-exports exactly the library-API set** named above, with `pub use` or, in the workspace case, items `pub` at the crate root.
- **When** any item not in the library API set is used, it is marked `pub(crate)` (single-crate case) or `pub(crate)` / private (workspace case).
- **When** a sibling activation imports from `crate::activations::<name>::` (single-crate) or `substrate_<name>::` (workspace), it only sees the library-API items.

This ticket is scaffolding: it declares the library API and narrows visibility. It does **not** migrate sibling call sites — that's DC-3..DC-6.

## Risks

- **Some current reach-ins are load-bearing.** If Orcha's `graph_runner.rs` directly pattern-matches on a ClaudeCode internal type, narrowing that type to `pub(crate)` breaks the build until DC-3/DC-4 lands. **Mitigation:** stage the visibility narrowing behind a `#[allow(dead_code)]` shim during DC-2 — add the library-API items, leave the old items public but doc-comment them as "deprecated / internal — use <library API>". The actual `pub(crate)` flip happens in DC-3/DC-4/DC-5/DC-6 as those tickets migrate each call site. This keeps DC-2 pure-additive and unblocks parallel work.
- **Workspace split (if S01 chose B) has a longer tail.** DC-2 converts every activation, not just one. Budget accordingly. If migration for any one activation exceeds 2× the spike's measured cost, pause and write a DC-S02 recon.
- **`Model` enum coupling.** ClaudeCode's `Model` enum is imported by Orcha directly. DC-4 replaces the concrete import with a trait or model ID, but DC-2 must decide whether `Model` stays in the library API (consumer-facing value) or gets hidden behind a `ModelId` newtype. Pin the call in DC-2's final form based on what's actually exposed by ClaudeCode's library methods.

## What must NOT change

- Wire-level RPC behavior. Every `#[plexus_macros::method]` signature on every activation is unchanged. Schema hashes do not shift.
- Activation startup order in `builder.rs`. Cyclic-parent injection unchanged.
- SQLite schema and migrations. DC-2 is a pure-Rust surface refactor.
- Existing `cargo test` pass rate.
- The `Activation` trait impl for each activation.

## Acceptance criteria

1. Each of the six activations (Orcha, ClaudeCode, Loopback, Bash, Cone, Arbor) has a single documented library-API entry point. In the single-crate case, that's its `mod.rs`; in the workspace case, that's its crate root. The entry point carries a doc comment stating it is the library API.
2. The library-API set listed in the "Required behavior" table above is re-exported from each entry point. No storage struct is re-exported. No session-management internal is re-exported.
3. Every non-library item still-`pub` carries either a `#[doc(hidden)]` marker or a doc comment tagging it as internal pending migration (this is the DC-2 shim; the actual `pub(crate)` flip happens in DC-3..DC-6).
4. `cargo test --workspace` passes with zero test failures.
5. `cargo doc --no-deps` produces a rustdoc where each activation's entry-point page has a "Library API" heading listing the re-exports.
6. A grep check — `grep -rn "pub use" src/activations/*/mod.rs` — shows one re-export block per activation entry point, matching the Required-behavior table.
7. README's "Pinned cross-epic contracts" section gains a subsection titled "Activation library-API entry points" that pins the six entry points.

## Completion

Implementor delivers:

- A single commit per activation (or one commit per crate, if workspace-split) introducing the entry-point module, the library-API re-exports, and the doc comments.
- `cargo test` output showing green.
- `cargo doc` output with the library-API headings visible (screenshot or link to the generated HTML).
- README update.
- Status flip to `Complete` in the commit that lands the work.
