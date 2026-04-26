---
id: ST-1
title: "Strong typing: domain newtypes for every cross-activation identifier"
status: Epic
type: epic
blocked_by: []
unlocks: [ST-2, ST-3, ST-4, ST-5, ST-6, ST-7, ST-8, ST-9, ST-10]
target_repo: plexus-substrate
---

## Goal

End state: every identifier that crosses an activation boundary in `plexus-substrate` is represented by a domain newtype, not a bare `String`, `Uuid`, `i64`, or `PathBuf`. The compiler catches cross-boundary swaps (SessionId vs StreamId, claude_session_id vs loopback_session_id, ToolUseId vs tool_name). Wire format stays byte-identical via `#[serde(transparent)]`. Each newtype derives the canonical trait set from `skills/strong-typing/SKILL.md`. All cross-boundary IDs live in a single foundation module (ST-2) and are re-exported through each activation's prelude so call sites touch typed names.

The swap-compiles hazard list in `docs/architecture/16670380887168786687_substrate-technical-debt-audit.md` (the "Stringly-typed debt" section) enumerates 29 distinct concepts. This epic covers the 8 highest-priority cross-boundary IDs plus domain-local newtypes (`BackendUrl`, `TemplateId`, `PluginId` wrappers).

## Context

Pinned newtype names and traits live in `plans/README.md` under "Pinned cross-epic contracts". This epic owns those types and other epics (STG, RL, TM) consume them. The README names are authoritative — this epic matches them exactly:

| Newtype | Wraps | Cross-boundary consumers |
|---|---|---|
| `SessionId` | `String` | Orcha, ClaudeCode, Loopback |
| `GraphId` | `String` | Lattice, Orcha |
| `NodeId` | `String` (lattice node; `arbor::NodeId` stays `ArborId`-based) | Lattice |
| `StreamId` | `Uuid` | ClaudeCode, Orcha |
| `ApprovalId` | `Uuid` (canonicalize from Loopback's existing `ApprovalId(Uuid)`) | Orcha, Loopback |
| `ToolUseId` | `String` | ClaudeCode, Loopback |
| `TicketId` | `String` | Orcha/PM, TM |
| `WorkingDir` | `PathBuf` | Orcha, ClaudeCode |
| `ModelId` | `String` | Cone, ClaudeCode, Orcha |
| `BackendUrl` | structured (host + port + protocol enum) | Registry |
| `TemplateId` | `String` | Mustache |

Non-goals:

- Replacing `arbor::ArborId`, `arbor::NodeId` (alias to `ArborId`), or `arbor::TreeId` — they're already newtyped and keep their shape.
- Replacing the already-enum `Model` in ClaudeCode — canonicalize to flow through `ModelId` at boundaries but keep the enum for internal validation.
- Rewriting SQLite schemas — storage columns remain `TEXT`/`BLOB`; only Rust-side types change.
- New wire-format breaking changes — `#[serde(transparent)]` is mandatory for every newtype.

Crates touched: `plexus-substrate` only. No plexus-core, plexus-macros, or plexus-protocol changes.

## Dependency DAG

```
                ST-2
        (foundation: all newtypes)
                 │
       ┌─────┬──┼──┬─────┬─────┬─────┐
       ▼     ▼  ▼  ▼     ▼     ▼     ▼
     ST-3  ST-4 ST-5 ST-6  ST-7  ST-8  ST-9
    (Arbor)(Orcha)(Lattice)(CC)(Loopback)(Cone+)(Registry+Mustache)
       │     │    │    │     │     │     │
       └─────┴────┴────┼─────┴─────┴─────┘
                       ▼
                     ST-10
             (wire-format roundtrip tests)
```

ST-3 through ST-9 run in parallel once ST-2 lands — each owns a disjoint set of files (one activation per ticket). ST-10 consumes all prior outputs.

## Phase breakdown

| Phase | Tickets | Parallelism |
|---|---|---|
| 1. Foundation | ST-2 | Single ticket; must complete before phase 2. Defines `substrate::types` module with all newtypes. |
| 2. Per-activation migration | ST-3, ST-4, ST-5, ST-6, ST-7, ST-8, ST-9 | Parallel; each ticket owns one activation's files. |
| 3. Integration | ST-10 | Serial after phase 2. Serde roundtrip fixtures proving wire-format byte-identity. |

## Tickets

| ID | Summary | Target files | Status |
|---|---|---|---|
| ST-1 | This epic overview | — | Epic |
| ST-2 | Foundation: newtype module with all cross-boundary IDs | `src/types.rs` (new), `src/lib.rs` | Pending |
| ST-3 | Migrate Arbor to use typed IDs | `src/activations/arbor/*` | Pending |
| ST-4 | Migrate Orcha to use typed IDs | `src/activations/orcha/*` | Pending |
| ST-5 | Migrate Lattice to use typed IDs | `src/activations/lattice/*` | Pending |
| ST-6 | Migrate ClaudeCode to use typed IDs | `src/activations/claudecode/*` | Pending |
| ST-7 | Migrate Loopback to use typed IDs | `src/activations/claudecode_loopback/*` | Pending |
| ST-8 | Migrate Cone + remaining activations to use typed IDs | `src/activations/cone/*`, other leftovers | Pending |
| ST-9 | Registry + Mustache local newtypes | `src/activations/registry/*`, `src/activations/mustache/*` | Pending |
| ST-10 | Wire-format integration test: serde roundtrip for every newtype | `tests/strong_typing_wire_format.rs` (new) | Pending |

## Cross-epic references

- **Pinned newtype names** — `plans/README.md` "Pinned cross-epic contracts" table. This epic matches exactly.
- **STG (storage)** — `ArborStore`, `OrchaStore`, etc. will take these newtypes as key parameters once STG starts. STG must not restart ST's definitions.
- **RL (resilience)** — error variants like `OrchaError::SessionNotFound { session_id }` will carry `SessionId`, not `String`.
- **TM (ticketing)** — TM's `TicketId` is the same concept. TM consumes ST-2's `TicketId` directly; no duplicate type.
- **Strong-typing skill** — `~/dev/controlflow/hypermemetic/skills/skills/strong-typing/SKILL.md`. Rules: `#[serde(transparent)]` mandatory; required traits `Debug + Clone + PartialEq + Eq + Hash + Serialize + Deserialize + Display`; construction via `new()`; access via `as_str()` or `inner()`.

## Wire-format preservation promise

Every newtype in ST-2 is annotated `#[serde(transparent)]`. This means:

- A `SessionId("abc-123")` serializes to the JSON string `"abc-123"` — no wrapping object, no `{"0": "abc-123"}`.
- A client on the previous substrate version sending a plain JSON string to an endpoint that now expects `SessionId` deserializes successfully.
- A newer substrate serializing a `SessionId` to a client still on bare-String types parses successfully.
- No JSON-RPC method gains a new required field; no existing field changes type on the wire.

ST-10 proves this with fixture JSON files that exercise every newtype in both directions.

## What must NOT change

- JSON-RPC wire format — byte-identity before and after this epic for every method in every activation.
- SQLite schemas — no column type or constraint changes.
- Public re-exports already stable (like `arbor::ArborId`, `arbor::NodeId`, `arbor::TreeId`) — their names and shapes stay identical.
- Activation namespaces (`"orcha"`, `"claudecode"`, etc.) — those remain strings pending a separate decision.
- Enum types already in place (`Model`, `SessionState`, `ApprovalStatus`, etc.) — unchanged shape.

## Completion

Epic is Complete when:

1. ST-2 through ST-10 are all Complete.
2. `cargo build -p plexus-substrate` succeeds.
3. `cargo test -p plexus-substrate` succeeds, including ST-10's new roundtrip test.
4. Grep audit: no `type SessionId = String;`, `type GraphId = String;`, `type NodeId = String;`, `type StreamId = Uuid;`, `type ApprovalId = Uuid;` aliases remain in activation `types.rs` files.
5. Grep audit: no function signature in any activation's public surface takes two or more consecutive `String`/`&str`/`Uuid` parameters that represent different domain concepts.
