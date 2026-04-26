---
id: CHILD-2
title: "Extend ChildRouter trait: capabilities + opt-in list/search"
status: Complete
type: implementation
blocked_by: []
unlocks: [CHILD-3]
severity: Medium
target_repo: plexus-core
---

## Problem

`ChildRouter` today exposes only `router_namespace()` and `get_child(name)`. A caller holding a `ChildRouter` handle has no way to ask "what children do you have?" or "do any of your children match this query?". Dynamic child sets are therefore invisible — every caller must know child names out-of-band. Synapse, codegen, and any interactive client are stuck; they cannot offer completion, browse, or search without bespoke per-activation hacks.

The fix must respect that the Plexus RPC network is a **graph**, not a tree: children may be remote, infinite, or deliberately private. Listing and searching must be **opt-in** and advertised by the router, never assumed by the caller.

## Context

Target crate: `plexus-core` at `/Users/shmendez/dev/controlflow/hypermemetic/plexus-core`.

Current trait shape (as it exists before this ticket):

| Method | Return | Semantics |
|---|---|---|
| `router_namespace()` | `&'static str` | Namespace prefix for routing |
| `get_child(name)` | `Option<Handle>` | Lookup child by exact name |

Upstream consumer note: CHILD-3 (the `#[plexus_macros::child]` macro) reads the new trait surface and will by default emit `ChildCapabilities::empty()` plus the default `None` implementations of the list/search methods. CHILD-4 later opts in when the author passes `list = ...` / `search = ...`.

Bitflags dependency: use the `bitflags` crate, **v2.x**. This is already in the plexus workspace's transitive graph; add it as a direct dependency of `plexus-core` if not already present.

Stream type: `futures_core::stream::BoxStream<'a, String>` is the canonical return for async cursors in this workspace. Methods return `Option<BoxStream<'_, String>>`; `None` means "this capability is not implemented by this router."

## Required behavior

Extend `ChildRouter` with the following methods, **all with default implementations** so existing implementors keep compiling:

| Method | Signature (behavioural) | Default | Meaning of return |
|---|---|---|---|
| `capabilities` | sync, returns `ChildCapabilities` | `ChildCapabilities::empty()` | Which optional operations this router actually supports. |
| `list_children` | async, returns `Option<BoxStream<'_, String>>` | `None` | `Some(stream)` yields every child name the router is willing to enumerate. `None` = not supported. |
| `search_children` | async, takes a query string, returns `Option<BoxStream<'_, String>>` | `None` | `Some(stream)` yields child names matching the router-defined query semantics. `None` = not supported. |

Introduce a new public type `ChildCapabilities` in `plexus-core`:

| Flag | Meaning |
|---|---|
| `ChildCapabilities::LIST` | Router promises `list_children()` returns `Some(...)`. |
| `ChildCapabilities::SEARCH` | Router promises `search_children(query)` returns `Some(...)` for any query. |
| `ChildCapabilities::empty()` | No opt-in capabilities; default. |

Contract rules:

| Condition | Expected |
|---|---|
| `capabilities().contains(LIST)` is `true` | `list_children().await` returns `Some(stream)` |
| `capabilities().contains(LIST)` is `false` | `list_children().await` returns `None` |
| `capabilities().contains(SEARCH)` is `true` | `search_children(q).await` returns `Some(stream)` for every `q` |
| `capabilities().contains(SEARCH)` is `false` | `search_children(q).await` returns `None` for every `q` |

`ChildCapabilities` is a bitflags-style value type implementing `Copy`, `Clone`, `Debug`, `Eq`, `PartialEq`, `Hash`, plus the standard bitwise operators from `bitflags` v2.

## Risks

- **bitflags v2 trait derivations.** Older workspace code may use v1. Risk: a dependency conflict. Default if encountered: keep `plexus-core`'s direct dep on v2 and let cargo resolve; if a downstream breaks, open a spike. This ticket pins v2.
- **Stream lifetime.** `BoxStream<'_, String>` borrows from `&self`. Implementors holding non-`Sync` data may need owned streams. Default: accept `BoxStream<'_, String>` now; revisit if CHILD-4 or CHILD-7 hit a real wall.
- **Capability drift.** Author overrides `list_children` but forgets to set `LIST` in `capabilities()`. Mitigation: this is a correctness bug in the implementor, caught by CHILD-4's macro codegen (which sets both together). For hand-written impls, document the contract in rustdoc on the trait. No runtime enforcement in this ticket.

## What must NOT change

- `router_namespace()` signature is unchanged.
- `get_child(name)` signature is unchanged.
- All existing `ChildRouter` implementors in the workspace compile without edits after this ticket — specifically including the macro-generated impls emitted by today's `plexus-macros` and substrate's manual Solar `ChildRouter` impl.
- Public re-exports from `plexus-core` keep their current names; this ticket only adds `ChildCapabilities` to the public surface.
- Wire format / transport: no Plexus RPC wire changes. This ticket is trait-level only.

## Acceptance criteria

1. `cargo build -p plexus-core` succeeds.
2. `cargo test -p plexus-core` succeeds.
3. `cargo build -p plexus-substrate` succeeds with **no source edits** to any of substrate's 16 activations — regression gate for "default implementations preserve existing behaviour".
4. `cargo build -p plexus-macros` succeeds — the proc-macro crate's existing generated `ChildRouter` impls continue to compile.
5. A new unit test in `plexus-core` exercises a minimal `ChildRouter` impl that overrides **only** `router_namespace` and `get_child`. Observable results:

   | Call | Expected |
   |---|---|
   | `router.capabilities()` | `ChildCapabilities::empty()` |
   | `router.list_children().await` | `None` |
   | `router.search_children("anything").await` | `None` |

6. A second new unit test in `plexus-core` exercises a `ChildRouter` impl that overrides all three new methods. Observable results:

   | Call | Expected |
   |---|---|
   | `router.capabilities()` | `ChildCapabilities::LIST \| ChildCapabilities::SEARCH` |
   | `router.list_children().await` | `Some(stream)` that yields a non-empty, finite sequence of `String` values |
   | `router.search_children("x").await` | `Some(stream)` that yields a `String` sequence filtered by the query |

7. `ChildCapabilities` is exported from `plexus-core`'s public prelude (or root module, matching existing convention for public types).

## Completion

Implementor delivers:

- A PR against `plexus-core` containing the trait extension, `ChildCapabilities` type, and the two new unit tests.
- PR description includes the output of `cargo build -p plexus-core`, `cargo test -p plexus-core`, `cargo build -p plexus-substrate`, and `cargo build -p plexus-macros` — all green.
- Ticket status flipped from `Ready` → `Complete` in the same commit as the code change.
- CHILD-3 is unblocked; the implementor notes this in the PR description.
