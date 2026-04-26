---
id: CHILD-1
title: "Ergonomic child activations: #[child] method attribute + opt-in list/search"
status: Epic
type: epic
blocked_by: []
unlocks: [CHILD-2, CHILD-3, CHILD-4, CHILD-5, CHILD-6, CHILD-7]
target_repo: cross-cutting
---

## Goal

End state: authors define child activations by annotating lookup methods with `#[plexus_macros::child]` inside an `#[plexus_macros::activation]` impl block. The proc macro scans the impl and synthesises a `ChildRouter` implementation automatically. The older `children = [field_a, field_b]` list syntax and the `hub` flag are **superseded** by `#[child]` — they continue to compile without changes during this epic; a separate follow-up epic will introduce deprecation warnings and eventual removal. `ChildRouter` gains opt-in `list_children` and `search_children` streams gated by a `ChildCapabilities` bitflags value — clients discover capabilities at runtime and never assume a tree. Doc comments (`///`) on activations and methods are extracted as descriptions and surface through Plexus RPC introspection. The `crate_path` macro argument has a sensible default so authors don't have to write `crate_path = ::plexus_core` boilerplate. Substrate's Solar activation is migrated to the new syntax and ships as the reference example.

## Context

The Plexus RPC network is a **graph**, not a tree. Children may live on remote nodes; a parent's child set may be uncountable or infinite. Synapse traverses blindly and performs cycle detection — it cannot assume a known shape. Therefore `ChildRouter::get_child(name)` remains the only mandatory operation; listing and searching are **opt-in** and advertised via `ChildCapabilities`.

Terminology used in this epic:
- **Plexus RPC** — the protocol.
- **DynamicHub** — the in-process router inside substrate.
- **Activation** — a service implementing `plexus_core::Activation`.
- **Hub activation** — an activation that also implements `ChildRouter`.

Crates touched:

| Crate | Path | Role |
|---|---|---|
| `plexus-core` | `/Users/shmendez/dev/controlflow/hypermemetic/plexus-core` | Trait definitions (`Activation`, `ChildRouter`, `DynamicHub`) |
| `plexus-macros` | `/Users/shmendez/dev/controlflow/hypermemetic/plexus-macros` | Proc-macro crate (`#[activation]`, `#[method]`) |
| `plexus-substrate` | `/Users/shmendez/dev/controlflow/hypermemetic/plexus-substrate` | Reference Plexus RPC server; Solar is the marquee consumer |

## Dependency DAG

```
              CHILD-2
           (core trait ext)
                 │
                 ▼
              CHILD-3                CHILD-5     CHILD-6
         (basic #[child] attr)      (doc cmt)  (crate_path)
                 │                      │           │
                 ▼                      │           │
              CHILD-4                   │           │
        (list/search/capabilities)      │           │
                 │                      │           │
                 └──────┬───────────────┴───────────┘
                        ▼
                     CHILD-7
                 (Solar migration)
```

CHILD-5 and CHILD-6 have `blocked_by: []` — they are independent of the macro-core chain and can run in parallel with CHILD-2, CHILD-3, and CHILD-4. CHILD-7 integrates everything and is the only ticket that consumes all prior outputs.

## Phase Breakdown

| Phase | Tickets | Parallelism |
|---|---|---|
| 1. Foundation | CHILD-2 | Single ticket; must complete before phase 2 starts. |
| 2. Core macro | CHILD-3 → CHILD-4 | Serial; CHILD-4 extends CHILD-3's generated code. |
| 3. DX polish | CHILD-5, CHILD-6 | Parallel to each other and to phase 2. |
| 4. Integration | CHILD-7 | Consumes outputs of phases 2 and 3. |

## Tickets

| ID | Summary | Target repo | Status |
|---|---|---|---|
| CHILD-1 | This epic overview | — | Epic |
| CHILD-2 | Extend `ChildRouter` trait with `capabilities()`, opt-in `list_children`, `search_children` | plexus-core | Pending |
| CHILD-3 | Basic `#[plexus_macros::child]` method attribute (lookup-only codegen) | plexus-macros | Pending |
| CHILD-4 | `list = ` / `search = ` attribute args and `ChildCapabilities` bitflags wiring | plexus-macros | Pending |
| CHILD-5 | Extract `///` doc comments as activation/method descriptions | plexus-macros | Pending |
| CHILD-6 | Fix `crate_path` default so boilerplate can be omitted | plexus-macros | Pending |
| CHILD-7 | Migrate substrate's Solar activation to `#[child]` syntax | plexus-substrate | Pending |

## Out of scope

- **IDY (identity) epic** — deferred; not resolved by this epic.
- **Sweeping every activation in substrate to the new syntax** — only Solar (CHILD-7) and any activations flagged as affected while auditing CHILD-2. A substrate-wide sweep is a follow-up epic.
- **Remote-child transport semantics** — unchanged. This epic does not touch how child routers reach remote nodes; it only changes how they are declared and introspected in-process.
- **Replacing `#[activation]` or `#[method]`** — untouched.

`#[child]` mutual exclusion with `#[method]` on the same function **is in scope** — the decision is made in CHILD-3 and becomes the contract for downstream tickets.

## What must NOT change

- Existing `ChildRouter` implementors (manual or macro-generated) continue to compile after CHILD-2 lands without edits.
- Substrate's current Solar manual `ChildRouter` impl works until CHILD-7 replaces it.
- `router_namespace()` and `get_child(name)` signatures are invariant across the epic.
- The `children = [...]` list syntax and the `hub` flag continue to compile without warnings or errors through this epic. Deprecation warnings and eventual removal are out of scope here and are tracked as a follow-up epic.

## Completion

Epic is Complete when CHILD-2 through CHILD-7 are all Complete and Solar is running on the new syntax in substrate's CI (`cargo test -p plexus-substrate` green with Solar migrated). Deprecation of the legacy `children = [...]` syntax is out of scope; a follow-up epic will own it.
