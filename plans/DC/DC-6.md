---
id: DC-6
title: "Decouple schema-walking from Cone, ClaudeCode, Orcha via Arbor library traits"
status: Pending
type: implementation
blocked_by: [DC-2, DC-4, DC-5]
unlocks: [DC-7]
severity: High
target_repo: plexus-substrate
---

## Problem

Three sibling activations — Cone, ClaudeCode, and Orcha — walk Arbor's tree structure by directly pattern-matching on Arbor's `NodeType` enum, constructing `Node` structs, and threading `NodeId` / `TreeId` through their internals. This means Arbor's schema enum (`NodeType { Text { content }, External { handle }, ... }`) is effectively a public cross-activation contract: any change to `NodeType`'s variants ripples through three call sites.

The audit flagged this as the most widespread coupling category: seven files across Cone, ClaudeCode, and Orcha pattern-match on Arbor schema internals. This defeats the library-API convention — schema walking is Arbor's internal concern, and siblings should call into Arbor through behavior-shaped traits, not by knowing Arbor's enum layout.

## Context

**The specific coupling (re-verify against HEAD; audit drift caveat applies):**

| File | Line | Pattern |
|---|---|---|
| `src/activations/cone/activation.rs` | 7 | `use crate::activations::arbor::{Node, NodeId, NodeType};` |
| `src/activations/cone/activation.rs` | 634 | `NodeType::Text { content } => { ... }` |
| `src/activations/cone/activation.rs` | 641 | `NodeType::External { handle } => { ... }` |
| `src/activations/cone/storage.rs` | 3 | `use crate::activations::arbor::{ArborStorage, NodeId, TreeId};` |
| `src/activations/cone/types.rs` | 1 | `use crate::activations::arbor::{NodeId, TreeId};` |
| `src/activations/cone/tests.rs` | 8 | Arbor internals in tests |
| `src/activations/claudecode/storage.rs` | 6 | `use crate::activations::arbor::{ArborStorage, NodeId, NodeType, TreeId};` |
| `src/activations/claudecode/render.rs` | 6 | `use crate::activations::arbor::{ArborStorage, NodeId, NodeType, TreeId, CollapseType};` |
| `src/activations/claudecode/sessions.rs` | 14 | `use crate::activations::arbor::{ArborStorage, TreeId};` |
| `src/activations/claudecode/types.rs` | 1 | `use crate::activations::arbor::{NodeId, TreeId};` |
| `src/activations/orcha/graph_runner.rs` | 1 | `use crate::activations::arbor::ArborStorage;` |
| `src/activations/orcha/graph_runtime.rs` | 1 | `use crate::activations::arbor::ArborStorage;` |
| `src/activations/orcha/graph_runtime.rs` | 341 | `use crate::activations::arbor::{ArborId, NodeType};` |
| `src/activations/orcha/context.rs` | 6 | `use crate::activations::arbor::{ArborStorage, NodeId, TreeId};` |
| `src/activations/orcha/orchestrator.rs` | 4 | `use crate::activations::arbor::ArborStorage;` |
| `src/activations/orcha/activation.rs` | 2527 | `use crate::activations::arbor::NodeType;` (in a test) |
| `src/activations/arbor/views.rs` | 175 | `use crate::activations::claudecode::NodeEvent;` (reverse leak — Arbor sees into ClaudeCode) |

**What's legitimate vs. what's a leak:**

- `NodeId`, `TreeId`, `ArborId` — these are Arbor's public domain types. Legitimate library-API exports. Remain in the library API. Consumers keep importing them.
- `ArborStorage` — storage trait/struct. **This is the leak.** Siblings use `ArborStorage` as the way to read/write Arbor data, which means they know about the storage shape. Library-API alternative: a trait or client handle that exposes **operations** (read a node's domain shape, walk ancestors, traverse children, store content, etc.) without committing to the storage implementation.
- `NodeType` — the schema enum. **This is the deepest leak.** Siblings pattern-match on `Text { content }` and `External { handle }` variants. Library-API alternative: expose domain operations that abstract over the node kind. E.g., `fn read_text_content(node_id) -> Option<String>`, `fn read_external_handle(node_id) -> Option<Handle>`, or a visitor trait with variant-specific callbacks. Siblings stop pattern-matching on `NodeType` directly.
- `Node` struct — the concrete row type. **Leak.** Same library-API fix as `NodeType`.
- `CollapseType` — another internal Arbor detail used by ClaudeCode's render.rs. **Leak.**

**Reverse leak.** `arbor/views.rs:175` imports `crate::activations::claudecode::NodeEvent` — Arbor depending on ClaudeCode's internal event type. This is a separate direction of coupling; it's absorbed into DC-6's scope because it's the same schema-walking category (Arbor pattern-matches on ClaudeCode's `NodeEvent` inside a view). Library-API fix: ClaudeCode exposes `NodeEvent` as a library-API domain type (re-exported from `claudecode/mod.rs` via DC-2), or Arbor's view operates on an abstracted event trait ClaudeCode implements.

**Library-API traits (new in DC-6).** Arbor's library API gains:

1. **`ArborRead` trait** — read-only access to Arbor (methods to fetch a node's content, ancestors, children, tree head) returning domain types, not storage rows. What Cone and ClaudeCode need for their walks.
2. **`ArborWrite` trait** — mutating access (store a node, advance a tree head) returning `Result<_, ArborError>`. What ClaudeCode and Orcha need for their writes.
3. **`NodeView` or similar** — a value type (NOT the internal `Node` struct) exposing node fields abstractly. Consumers hold a `NodeView`, not an `arbor::Node`.
4. **Visitor or downcast methods** — `fn as_text(&self) -> Option<&str>`, `fn as_external(&self) -> Option<&Handle>`, etc., on `NodeView`. Replaces direct `match NodeType` pattern matching.

The concrete shape — whether these are traits, handle types, free functions, or a combination — is picked by the implementor during DC-6 based on what the existing call sites actually need. The contract is: **no sibling pattern-matches on `NodeType`; no sibling holds `ArborStorage`; no sibling instantiates `Node` directly**.

## Required behavior

**Arbor side:**

| Item | Today | After DC-6 |
|---|---|---|
| `NodeType` | `pub enum NodeType { Text { ... }, External { ... }, ... }` re-exported | `pub(crate)` or private; replaced by `NodeView::as_text()` / `as_external()` accessors on the library-API node view |
| `Node` struct | `pub` re-exported | `pub(crate)`; replaced by `NodeView` library type |
| `ArborStorage` | `pub` re-exported | `pub(crate)`; sibling access replaced by `ArborRead` / `ArborWrite` traits or a `ArborClient` handle |
| `NodeId`, `TreeId`, `ArborId` | `pub` | `pub` — remain library API |
| `CollapseType` | `pub` re-exported | Either stays library API (if ClaudeCode legitimately needs to name it) or gets wrapped in a library-API accessor |

**Cone side:**

| Before | After |
|---|---|
| `use crate::activations::arbor::{Node, NodeId, NodeType};` | `use crate::activations::arbor::{NodeId, NodeView};` |
| `match node.node_type { NodeType::Text { content } => ..., NodeType::External { handle } => ... }` | `if let Some(content) = node.as_text() { ... } else if let Some(handle) = node.as_external() { ... }` (or visitor pattern equivalent) |
| `use crate::activations::arbor::ArborStorage` in storage.rs | `use crate::activations::arbor::ArborRead` (or `ArborClient`) |

**ClaudeCode side:**

| Before | After |
|---|---|
| `use crate::activations::arbor::{ArborStorage, NodeId, NodeType, TreeId}` in storage.rs and render.rs | Import only the library-API traits and domain types |
| Direct `NodeType` pattern matching in render.rs | `NodeView` accessor methods |
| `CollapseType` direct import | Either library API or wrapped |

**Orcha side:**

| Before | After |
|---|---|
| `use crate::activations::arbor::ArborStorage` in graph_runner.rs, graph_runtime.rs, context.rs, orchestrator.rs | `use crate::activations::arbor::ArborRead` (or `ArborClient`) |
| `use crate::activations::arbor::NodeType` in graph_runtime.rs:341 and activation.rs:2527 | Library-API view accessors |

**Reverse leak (Arbor → ClaudeCode):**

- `arbor/views.rs:175`: replace `use crate::activations::claudecode::NodeEvent` with either (a) a trait ClaudeCode implements and Arbor's view takes as a generic parameter, or (b) removing the coupling entirely by re-locating the view logic.

## Risks

- **The fix is bigger than DC-3/DC-4/DC-5 combined.** Seven files change across three activations, plus Arbor gains new trait surfaces, plus a reverse-direction leak. Implementation budget: expect this to be 2-3× any other DC ticket. Consider splitting into sub-tickets if the diff exceeds review capacity (e.g., DC-6a Cone, DC-6b ClaudeCode, DC-6c Orcha). **Pin decision:** implementor splits if (a) HEAD-verification reveals more coupling sites than the audit listed, or (b) the single-ticket diff exceeds ~800 lines changed. Splits inherit DC-6's acceptance criteria pro-rated to the covered sites.
- **`NodeView` might not be a clean abstraction.** If Cone and ClaudeCode have different readiness requirements for what a "node view" looks like (e.g., Cone wants lazy content, ClaudeCode wants eager handles), forcing a single `NodeView` type into both creates an impedance mismatch. **Mitigation:** start with the union of access patterns and narrow if one consumer needs a subset. If the impedance is genuine, ship two views (e.g., `TextNodeView`, `ExternalNodeView`) as sum-type-exhaustive alternatives to pattern-matching `NodeType`.
- **`ArborStorage` is currently a trait-like shared surface.** The audit notes Arbor is the shared store — multiple activations already hold `Arc<dyn ArborStorage>` or similar. Narrowing it to `pub(crate)` requires every holder to switch to `ArborRead` / `ArborWrite` / `ArborClient`. That's more invasive than a re-export swap. Verify by counting current holders before committing to the shape.
- **File collisions with DC-4 and DC-5.** DC-6 touches `orcha/graph_runner.rs` (also DC-4) and `cone/activation.rs` (also DC-5). DC-6 is `blocked_by: [DC-2, DC-4, DC-5]` — enforced serialization. Do not start DC-6 until DC-4 and DC-5 are both Complete, then re-verify HEAD line anchors.

## What must NOT change

- Arbor's wire-level RPC methods — request/response shapes identical.
- Arbor's SQLite schema, migrations.
- Cone / ClaudeCode / Orcha wire APIs.
- Arbor's cross-activation semantic behavior — same nodes are readable, same writes persist, same collapse/fork/join operations produce the same outcomes.
- `NodeId`, `TreeId`, `ArborId` shapes — these remain identical and library-API.

## Acceptance criteria

1. `grep -rn "use crate::activations::arbor::.*NodeType" src/activations/` returns zero results outside `src/activations/arbor/**`.
2. `grep -rn "use crate::activations::arbor::.*Node[^IVd]" src/activations/` shows only `NodeId`, `NodeView` (or whatever DC-6 chose), and `NodeType`-absent imports outside `src/activations/arbor/**`. No sibling imports the concrete `Node` struct.
3. `grep -rn "use crate::activations::arbor::.*ArborStorage" src/activations/` returns zero results outside `src/activations/arbor/**`.
4. `grep -rn "match.*NodeType::" src/activations/` returns zero results outside `src/activations/arbor/**`.
5. Arbor's `mod.rs` exposes the new library-API trait(s) and view type(s) with library-API doc comments. `NodeType`, concrete `Node`, and `ArborStorage` are demoted to `pub(crate)` or removed from re-exports.
6. The reverse leak at `arbor/views.rs:175` is resolved — either Arbor no longer imports ClaudeCode's `NodeEvent`, or it imports it via a ClaudeCode-library-sanctioned trait / re-export.
7. `cargo test --workspace` passes with zero test failures.
8. Cone's branch-traversal behavior, ClaudeCode's render behavior, and Orcha's graph-walk behavior are unchanged — verified by each activation's existing test suite, re-run and green.
9. `cargo doc --no-deps` output for Arbor's crate shows the new library-API traits and view types with clear documentation; `NodeType` no longer appears as a public item.

## Completion

Implementor delivers:

- Commit introducing Arbor's library-API traits and view types in Arbor crate / module.
- Commit(s) migrating Cone, ClaudeCode, Orcha call sites to use the new surface (may be split per activation if the diff warrants — see Risks).
- Commit demoting `NodeType`, concrete `Node`, `ArborStorage` to `pub(crate)`.
- Commit resolving the reverse leak in `arbor/views.rs`.
- `cargo test` output showing green.
- Before/after `grep` output for each acceptance-criteria grep.
- Commit message notes whether the ticket was split and why.
- Status flip to `Complete` in the commit (or final commit of a split) that lands the work.
