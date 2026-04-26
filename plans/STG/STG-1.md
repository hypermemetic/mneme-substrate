---
id: STG-1
title: "Storage abstraction — per-activation traits for backend hotswap"
status: Epic
type: epic
blocked_by: []
unlocks: []
target_repo: plexus-substrate
---

## Goal

Every storage-bearing activation in substrate today hand-wires a concrete `SqlitePool` in its constructor. Tests hit a real SQLite file; production uses the same code path. There is no seam for substituting a backend. This epic introduces per-activation storage traits — `ArborStore`, `OrchaStore`, `LatticeStore`, `ClaudeCodeStore`, `ConeStore`, `MustacheStore` (plus `McpSessionStore` for `mcp_session.rs`) — so each activation talks to its persistence layer through a `dyn` trait object. The same trait has at least two implementations: the current SQLite backend (production default) and an in-memory backend (for tests, and as a proof that the abstraction is real). A future Postgres backend implements the same traits without touching activation logic.

**Pinned design decision (from `plans/README.md`):** this is NOT a generic key/value trait. Each activation owns a trait shaped to its own access pattern. `MustacheStore` has `get_template`, `set_template`, `list_templates`, `delete_template`. `ArborStore` has whatever arbor needs. A generic `Store` trait would force activations to serialize every domain type to bytes and lose query shape — unacceptable.

## Context

Target crate: `plexus-substrate` at `/Users/shmendez/dev/controlflow/hypermemetic/plexus-substrate`.

**Current state per activation (pre-epic):**

| Activation | Storage file | Construction |
|---|---|---|
| Arbor | `src/activations/arbor/storage.rs` (1077 lines) | `ArborStorage::new(config)` returns struct with concrete `SqlitePool`. |
| Orcha | `src/activations/orcha/storage.rs` (664 lines) | Same pattern. |
| Lattice | `src/activations/lattice/storage.rs` (1134 lines) | Same pattern. |
| ClaudeCode | `src/activations/claudecode/storage.rs` (1487 lines) | Same pattern. |
| Cone | `src/activations/cone/storage.rs` (664 lines) | Same pattern. |
| Mustache | `src/activations/mustache/storage.rs` (335 lines) | Same pattern. Smallest surface — selected as spike target. |
| MCP session | `src/mcp_session.rs` (412 lines) | Same pattern, outside any activation. |

Every activation constructor signature today is roughly:

```rust
impl MustacheActivation {
    pub async fn new(config: MustacheStorageConfig) -> Result<Self, Error> {
        let storage = MustacheStorage::new(config).await?;
        Ok(Self { storage: Arc::new(storage) })
    }
}
```

Post-epic, the shape is:

```rust
impl MustacheActivation {
    pub async fn new(store: Arc<dyn MustacheStore>) -> Result<Self, Error> {
        Ok(Self { store })
    }
    pub async fn with_sqlite(config: MustacheStorageConfig) -> Result<Self, Error> { /* convenience */ }
    #[cfg(any(test, feature = "test-doubles"))]
    pub fn with_memory() -> Self { /* convenience */ }
}
```

**Cross-epic inputs:**

- **ST epic** owns domain newtypes (`SessionId`, `GraphId`, `NodeId`, `StreamId`, `ApprovalId`, `ToolUseId`, `TicketId`, `WorkingDir`, `ModelId`, `TemplateId`). Trait signatures consume newtypes, not bare strings (`fn get_session(id: &SessionId)` not `fn get_session(id: &str)`). If an activation is migrated to its `*Store` trait before ST finishes the relevant newtype for that activation's IDs, the migration ticket pins the newtype as blocking. See each STG-3..8 ticket's `blocked_by` field.

- **`plans/README.md` cross-epic contracts section** pins the trait names verbatim: `ArborStore`, `OrchaStore`, `LatticeStore`, `ClaudeCodeStore`, `ConeStore`, `MustacheStore`. Use these exact names. Do not invent alternates.

## Dependency DAG

```
STG-S01 (trait shape on Mustache)
   │
   ▼
STG-S02 (hotswap proof: in-memory Mustache + same tests)
   │
   ▼
STG-2 (foundation: pattern + template module)
   │
   ├────┬────┬────┬────┬────┬────┐
   ▼    ▼    ▼    ▼    ▼    ▼    ▼
 STG-3 STG-4 STG-5 STG-6 STG-7 STG-8 STG-9
 Arbor Orcha Latt  CC    Cone  Must  MCP
   │    │    │    │    │    │    │
   └────┴────┴────┴────┴────┴────┴────┘
                     │
                     ▼
                  STG-10 (end-to-end: full substrate test suite
                          against all-in-memory backends)
```

Spikes gate the foundation:

- **STG-S01** answers: can we express a single activation's storage surface as a trait without performance or ergonomic regression? Binary pass: existing Mustache tests green against the trait-based rewrite.
- **STG-S02** answers: does the trait abstraction actually enable hotswap? Binary pass: same Mustache test suite green against a second (in-memory) backend.

If either spike fails, the epic's approach must be revisited before the foundation ticket lands — a `dyn`-based per-activation trait may not be viable and we'd need to consider generics, associated-type workarounds, or a different factoring.

STG-3 through STG-9 run in parallel after STG-2 — each migrates a different activation, each touches a disjoint file set (`src/activations/<name>/` per ticket; STG-9 touches `src/mcp_session.rs`). See each ticket's "What must NOT change" and "Acceptance criteria" for file-boundary scope.

STG-10 gates closure — it verifies the whole swap works end-to-end, not just per-activation.

## Phase Breakdown

| Phase | Tickets | Notes |
|---|---|---|
| 1. Spike | STG-S01 | Trait-shape proof on Mustache (smallest storage surface). |
| 2. Spike | STG-S02 | Hotswap proof: in-memory Mustache passes same tests. |
| 3. Foundation | STG-2 | Extract the pattern into a template / shared module. Establish the SQLite + in-memory pattern template. |
| 4. Migrations | STG-3..STG-9 | Parallel — one per activation. |
| 5. Integration | STG-10 | Full test suite against all-in-memory backends. |

## Tickets

| ID | Summary | Target scope | Status |
|---|---|---|---|
| STG-1 | This epic overview | — | Epic |
| STG-S01 | Spike: trait shape for ONE activation (Mustache) | `src/activations/mustache/` | Pending |
| STG-S02 | Spike: hotswap proof (in-memory `MustacheStore`) | `src/activations/mustache/` | Pending |
| STG-2 | Foundation: extract traits into a shared module / template | `src/activations/storage.rs` + docs | Pending |
| STG-3 | Migrate Arbor to `ArborStore` trait | `src/activations/arbor/` | Pending |
| STG-4 | Migrate Orcha to `OrchaStore` trait | `src/activations/orcha/` | Pending |
| STG-5 | Migrate Lattice to `LatticeStore` trait | `src/activations/lattice/` | Pending |
| STG-6 | Migrate ClaudeCode to `ClaudeCodeStore` trait | `src/activations/claudecode/` | Pending |
| STG-7 | Migrate Cone to `ConeStore` trait | `src/activations/cone/` | Pending |
| STG-8 | Migrate Mustache to `MustacheStore` trait (formalize post-spike) | `src/activations/mustache/` | Pending |
| STG-9 | MCP session storage: `McpSessionStore` trait | `src/mcp_session.rs` | Pending |
| STG-10 | End-to-end: full substrate test suite against in-memory backends | `src/builder.rs` + test harness | Pending |

## Out of scope

- **Postgres backend implementations.** This epic establishes the trait seam so a Postgres impl can drop in. Writing the Postgres backend itself is a separate follow-up.
- **Schema migrations across backends.** Each backend owns its own schema. There is no migration tool planned here.
- **ST newtypes.** ST epic defines them; STG consumes them. If ST hasn't landed a newtype by the time a migration ticket starts, the migration ticket is blocked on ST.
- **Activation logic changes.** STG moves storage behind a trait. No behavior changes. No new features on any activation.
- **DC (decoupling) overlap.** DC routes activation ↔ activation calls through the hub. STG is orthogonal — it abstracts each activation's *own* persistence. No coordination needed between DC and STG.
- **Generic KV trait.** Explicitly rejected. Pinned in `plans/README.md`.
- **Stateless activations** (Echo, Health, Bash, Chaos, Interactive). They have no storage — nothing to abstract.

## What must NOT change

- The existing SQLite backend remains the production default. No user-facing behavior change.
- Wire format, schemas, and DB file locations on disk stay identical for the SQLite backend.
- All existing substrate tests continue to pass at every phase boundary.
- Activation public API surface (methods visible via Plexus RPC) is unchanged.
- `builder.rs` startup path for the default (SQLite) configuration is unchanged — it may gain a new alternate path for injecting stores, but the default path produces the same observable behavior.

## Completion

Epic is Complete when STG-2 through STG-10 are all Complete and:

- Each of `ArborStore`, `OrchaStore`, `LatticeStore`, `ClaudeCodeStore`, `ConeStore`, `MustacheStore`, and `McpSessionStore` is a public trait in substrate.
- Each trait has at minimum two implementations in the crate: the existing SQLite backend and an in-memory backend (gated behind `#[cfg(test)]` or a `test-doubles` feature — see STG-2).
- Each affected activation's constructor accepts an `Arc<dyn TraitName>`; a `with_sqlite` convenience constructor preserves the default production path.
- `cargo test -p plexus-substrate` green against an all-in-memory build (STG-10 provides the harness).
- `cargo test -p plexus-substrate` green against the default SQLite build (regression: every prior test still passes).
- A demo or README snippet in the final PR shows one activation instantiated with an injected in-memory store, proving the seam works end-to-end.
