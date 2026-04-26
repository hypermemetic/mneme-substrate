# Substrate Technical Debt and Architecture Audit

**Date:** 2026-04-16
**Scope:** Full audit of `plexus-substrate` v0.3.0 across all 16 activations, their storage layers, the DynamicHub dispatch path, the streaming protocol, and the session/persistence layer.
**Purpose:** Capture durable findings so future sessions can ticket against a shared picture. This is a compression algorithm for the current state of the code — the specific file:line references will drift, but the *categories* of debt persist.

---

## Architectural patterns that work

These are the decisions that paid off and should be preserved as the codebase evolves.

### Hub/Activation split with cyclic parent injection

- `DynamicHub` owns a registry of activations. Each activation receives a `Weak<DynamicHub>` via `Arc::new_cyclic` in `builder.rs`.
- Parent context is injected via `OnceLock<Weak<DynamicHub>>` with `PhantomData` on the activation — this enforces set-once semantics at compile time and prevents accidental re-injection.
- Activations call into siblings by upgrading the Weak to Arc on demand, which avoids the refcount cycle.

### SQLite-per-activation, actually enforced

Every stateful activation gets its own SQLite file under `~/.plexus/substrate/activations/{name}/`. The `activation_db_path_from_module!` macro (`storage.rs`) keeps the path derivation DRY. Stateless activations (Echo, Health, Bash, Chaos, Interactive) don't take the hit. The pattern is respected without exception — Arbor is the only shared store, and that's intentional (it's the handle backend).

### Per-activation error enums

`ConeError`, `OrchaError`, `ClaudeCodeError`, etc. — no global flat error type. Each activation owns its failure modes, and `From<String>` gives an ergonomic escape when context is scarce.

### State is modeled as enums, not strings

At least ten constrained-value sets are correctly enumed (not stringly typed): `MessageRole`, `Model`, `StreamStatus`, `AgentMode`, `AgentState`, `TokenColor`, `GraphStatus`, `NodeStatus`, `ApprovalStatus`, `ResourceState`, `BodyType`. This is the bright spot of the type system.

### Streaming "caller wraps" pattern

Health and Echo return their domain stream types; the wrapper into `PlexusStreamItem` happens at the dispatch boundary. The domain type never knows about JSON-RPC.

---

## Architectural patterns that don't work

### Activation coupling is the biggest debt

Activations reach directly into each other's storage and types instead of going through the hub. This defeats the whole point of the hub abstraction.

- **Orcha → Loopback:** `graph_runner.rs` imports `LoopbackStorage` directly and queries approval state as if it owned the table.
- **Orcha → ClaudeCode:** same file imports `ClaudeCode` and `Model` concretely; Orcha cannot be used without ClaudeCode compiled in.
- **Cone → Bash:** `cone/activation.rs` hardcodes `use crate::activations::bash::Bash`; Cone's tests construct a Bash instance directly.
- **Cone, ClaudeCode → Arbor schema:** multiple activations walk Arbor's `NodeType` and `NodeId` tree structure. A schema change in Arbor ripples through three or more call sites.

**Rule of thumb to re-establish:** activations communicate via hub-routed calls, not by importing siblings' `pub struct`s. If a struct is `pub` only because another activation needs it, it's a leak.

### Streaming protocol has no versioning

`PlexusStreamItem` is serialized as JSON with no version tag. Adding a new variant silently breaks older clients — no schema negotiation, no feature handshake, no graceful fallback. The protocol is frozen by omission.

### No backpressure anywhere in the dispatch path

- `DynamicHub` dispatch uses unbounded channels. A slow consumer causes unbounded memory growth.
- `ClaudeCodeStorage.streams: RwLock<HashMap<StreamId, ActiveStreamBuffer>>` never evicts. Client disconnect mid-stream = leak.
- No semaphores or concurrency limits on outbound activation work (Orcha can spawn unlimited agents, Bash can run unlimited commands).

### Cancellation is inconsistent

Orcha has a cancel registry via watch channels. Echo, Cone, ClaudeCode streams ignore client disconnects and run to completion regardless. There's no unified cancellation token that flows from transport → hub → activation.

### No graceful shutdown

No activation implements `Drop` or `close()` / `shutdown()`. SQLite pools close implicitly. In-flight Orcha graphs terminate abruptly on SIGINT. Spawned tokio tasks in `plugin_system/conversion.rs` and `health/activation.rs` have no cancellation tokens and can outlive the server.

---

## Stringly-typed debt

The domain has many identifiers that cross activation boundaries as bare `String`, `&str`, or `Uuid` type aliases. The swap-compiles test fails for every row below — two of these passed to the same function in wrong order will silently typecheck.

### Cross-boundary IDs (highest risk)

| Concept | Current | Boundary it crosses |
|---|---|---|
| `SessionId` | `type SessionId = String` (orcha) | Orcha ↔ ClaudeCode ↔ Loopback |
| `GraphId` | `type GraphId = String` (lattice) | Lattice ↔ Orcha cancel registry |
| `NodeId` (lattice) | `type NodeId = String` | DAG edges and ready events |
| `StreamId` | `type StreamId = Uuid` (claudecode) | Chat lifecycle ↔ Orcha polling |
| `ApprovalId` (Orcha-side) | bare `String` | Round-trips with Loopback's `ApprovalId(Uuid)` — same concept, two types |
| `ToolUseId` and `tool_name` | bare `String` | Approval pairing; swapping them silently corrupts permit logic |
| `TicketId` | `Option<String>` | Orcha ↔ PM correlation |
| `claude_session_id` and `loopback_session_id` | both bare `String` in the same struct | Trivially swappable with catastrophic resume failure |
| `model_id` | bare `String` in Cone and Orcha; enum in ClaudeCode | Cross-provider confusion |
| Working directory | bare `String` in multiple places | Should be `PathBuf` newtype |

### Local IDs (medium risk)

- Registry: `protocol`, `host`, `namespace` all as `String`. Invalid values ("http" instead of "ws") are silently accepted.
- Mustache: `TemplateId: String`, `method: String`, `plugin_id: Uuid`. No newtypes distinguish plugin UUIDs from other UUIDs.
- Arbor refcount: `owners: HashMap<String, i64>`. Owner identity is an untyped string.

### Constrained string sets that should be enums

- `spec_type: String` in `chaos/types.rs` — should be an enum over (Task, Scatter, Gather, SubGraph).
- Activation namespace strings ("orcha", "claudecode", etc.) — passed around raw where an enum would be exhaustive.
- Registry `protocol` field — should be an enum over supported transport schemes.

### Already-newtyped concepts (preserve on refactor)

`ArborId(Uuid)`, `ApprovalId(Uuid)` in Loopback, and the state enums listed in the "patterns that work" section. Any newtype unification should use these as the template.

---

## Load-bearing panics and error swallowing

### Panics in production paths

- `orcha/graph_runner.rs` — `unreachable!()` at the end of a match arm. If ever reached, the whole runner dies and no graph progress is saved.
- `orcha/ticket_compiler.rs` — three `panic!("wrong spec")` sites. Not in test code — in the compiler itself.
- `bash/executor/mod.rs` — `panic!("Expected stdout/exit")` on unexpected `BashOutput` variant. If the bash subprocess lifecycle ever changes shape, the executor crashes.
- `src/builder.rs` — startup uses `.expect()` on every storage init. Any partial failure during boot kills the whole server with a generic message.

### Error swallowing at critical boundaries

- **Orcha ticket persistence** — `let _ = pm.save_*()` at ~7 sites in `orcha/activation.rs`. Ticket state can diverge from reality with no logs.
- **Loopback approval resolution** — `let _ = storage.resolve_approval(...)` in `claudecode_loopback/activation.rs`. Timeout resolution failures vanish.
- **MCP session cleanup** — `.ok()` on multiple DB ops in `mcp_session.rs`. Sessions can become stale without observability.
- **Lattice schema migrations** — `let _ = sqlx::query("ALTER TABLE ... ADD COLUMN ...")`. Intended to suppress "column already exists" but eats real schema errors too.
- **Loopback approval reads** — `.read().ok()?` and `.filter_map(|r| self.row_to_approval(r).ok())`. Poisoned RwLock or corrupt row is indistinguishable from "no approvals".
- **Changelog last-hash** — `.await.ok().flatten()` collapses DB error into "no hash set". Can cause changelog loops.
- **Bash stderr truncation** — silently drops stderr lines past 100 with no marker.

### Platform assumptions

`chaos/activation.rs` reads `/proc/{pid}/cmdline` — Linux-only paths on a codebase that targets macOS.

---

## Missing systems

These aren't bugs — they're categories of infrastructure the codebase hasn't grown yet.

| System | Status |
|---|---|
| Config file (TOML/YAML) | Absent. All activations use `Default` plus ad-hoc env vars. DB paths, ports, timeouts scattered. |
| Metrics (Prometheus/OpenTelemetry) | Absent. Tracing is extensive (128+ spans) but zero counters, histograms, or `/metrics` endpoint. |
| Pagination | Absent on `orcha.list_graphs`, `pm.list_ticket_maps`, and every other list method. Everything returns full sets. |
| Rate limits / resource quotas | Absent. Unbounded agents, unbounded bash commands, unbounded DB connections. |
| Streaming protocol versioning | Absent. `PlexusStreamItem` has no version discriminator. |
| Property tests | Absent. No `proptest`, `quickcheck`, or equivalent. |
| Wire-format fuzzing | Absent. Serialization round-trips are tested with fixed inputs only. |
| Load tests | Absent. No tests exercise concurrency in DynamicHub dispatch, session cleanup under load, or connection limits. |
| Activation unit tests | Only Cone and Orcha have unit test suites. Eleven of fifteen activations have none. |
| Graceful shutdown | Absent. No `Drop` impls, no coordinated teardown, no in-flight task tracking. |

---

## Suggested ticket epics

Four natural epic boundaries fall out of the findings. Order by leverage — the strong-typing epic pins contracts that the others depend on.

1. **Strong-typing epic.** Introduce domain newtypes for every cross-boundary ID (`SessionId`, `GraphId`, `NodeId`, `StreamId`, `ApprovalId`, `ToolUseId`, `TicketId`, `WorkingDir`, `ModelId`). Highest-leverage because every subsequent epic benefits from pinned contracts.
2. **Resilience epic.** Kill `.expect()` chains in `builder.rs`. Replace load-bearing `panic!` / `unreachable!` sites. Add cancellation tokens end-to-end. Add graceful shutdown with task tracking.
3. **Decoupling epic.** Route Orcha ↔ Loopback and Cone ↔ Bash through the hub instead of direct imports. Establish public/private boundaries on activation modules. Make Orcha and Cone compilable as standalone units.
4. **Observability epic.** Config file loader. Metrics baseline. Structured error context with graph/session IDs. Pagination on list methods. Streaming protocol versioning.

Stringly-typed debt and load-bearing panics are the two areas where the compiler can be made to help. Coupling and missing systems require design work first.

---

## How this doc ages

The specific file:line references will drift as the code changes — treat them as pointers, not ground truth. Re-run the audit agents against the tree and update this doc when any of the four epics above ship, because the categories themselves are the durable contribution.
