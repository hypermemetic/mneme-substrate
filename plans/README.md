# Plexus Substrate Plans — Roadmap and Coordination

**Purpose:** single anchor document pinning the current roadmap, cross-epic contracts, and pinned design decisions that individual epic overviews reference. Read this before writing new tickets in parallel with other epics.

**Maintained by:** humans + Claude. Update when an epic's scope materially changes or a new cross-epic contract is pinned.

---

## Current state (session close-out snapshot)

### Shipped

| Epic | Status | Notes |
|---|---|---|
| **CHILD** | ✅ Implementation complete (7 tickets: CHILD-2 through CHILD-8) | `#[plexus_macros::child]` attribute, opt-in list/search, doc-comment extraction, `crate_path` auto-resolve, Solar migration, hub-mode inference. CHILD-10 superseded by IR epic. |
| **IR** (implementation spine) | ✅ 8 implementation tickets complete (IR-2 through IR-9) | `MethodRole` enum, `DeprecationInfo`, `ParamSchema`, role-tagged `MethodSchema` emission, backward-compat shim, synapse deprecation rendering, hub-codegen annotations + typed-handle codegen, Solar test migration. |

### Version bumps landed this cycle

| Crate | Before → After |
|---|---|
| plexus-core | 0.4.0 → **0.5.0** |
| plexus-macros | 0.4.0 → **0.5.0** |
| plexus-protocol | 0.3.2.0 → **0.4.0.0** |
| plexus-synapse | 3.10.1 → **3.11.0** |
| hub-codegen | 0.3.0 → **0.4.0** |
| plexus-substrate | 0.3.0 → **0.4.0** |

### In flight / Pending

| Epic | State |
|---|---|
| **IR** (cleanup) | 10 Pending follow-up tickets (IR-10..19) |
| **SYN** (synapse consumes capabilities) | 1 overview + 2 implementation tickets Pending |
| **HASH** (runtime hash aggregation) | 1 overview Pending; sub-tickets not yet filed to disk |
| **DC** (decoupling) | Not yet ticketed |
| **TM** (ticketing activation) | Not yet ticketed |
| **ST** (strong typing) | Not yet ticketed |
| **STG** (storage abstraction) | Not yet ticketed |
| **RL** (resilience) | Not yet ticketed |
| **OB** (observability) | Not yet ticketed |
| **IDY** (identity via PKE) | Deferred "deep future" per user direction |

---

## Roadmap (current execution order)

Sequencing chosen by the user:

1. **Finish IR** — promote and land IR-10..19 (10 Pending follow-ups; see detailed list below). Close the IR epic for good.
2. **DC** — ticket and execute. Library-hygiene epic: narrow `pub` surfaces, demote internals to `pub(crate)`, route cross-activation calls through library APIs.
3. **TM + ST in parallel** — ticket both, execute both. TM ships a ticketing activation; ST ships the domain-newtype discipline.
4. **Then** STG, RL, OB — ticket and execute (order TBD; probably parallel).
5. **Deferred:** IDY (PKE-strong identity).

Each epic's sub-tickets land in `plans/<EPIC>/<EPIC>-N.md`. Epic overview is always `-1`. See `skills/ticketing/SKILL.md` for format.

---

## Pinned cross-epic contracts

These names / concepts appear across multiple epics. Pinning them here prevents drift when epics fan out in parallel.

### Domain newtypes (ST epic owns these; other epics consume)

| Newtype | Wraps | First introduced by | Consumed by |
|---|---|---|---|
| `SessionId` | `String` | ST | Orcha, ClaudeCode, Loopback |
| `GraphId` | `String` | ST | Lattice, Orcha |
| `NodeId` | `String` | ST | Lattice |
| `StreamId` | `Uuid` | ST | ClaudeCode, Orcha |
| `ApprovalId` | `Uuid` | ST (canonicalize on Loopback's existing `ApprovalId(Uuid)`) | Orcha, Loopback |
| `ToolUseId` | `String` | ST | ClaudeCode, Loopback |
| `TicketId` | `String` | ST (matches TM's ticketing identifier) | Orcha/PM, TM |
| `WorkingDir` | `PathBuf` | ST | Orcha, ClaudeCode |
| `ModelId` | `String` | ST | Cone, ClaudeCode, Orcha |
| `BackendUrl` | structured (host + port + protocol) | ST | Registry |
| `TemplateId` | `String` | ST | Mustache |

**Derives on every newtype:** `Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Display` with `#[serde(transparent)]` for wire-format compatibility. Follow `skills/strong-typing/SKILL.md`.

### Trait surfaces (STG, TM, RL epics own these)

| Trait | Owner epic | Purpose |
|---|---|---|
| `ArborStore`, `OrchaStore`, `LatticeStore`, `ClaudeCodeStore`, `ConeStore`, `MustacheStore` | STG | Per-activation storage abstraction; each backend (SQLite, in-memory, Postgres) implements one of these. Pinned: per-activation traits, NOT a generic KV. |
| `TicketStore` | TM | Ticket persistence trait for the ticketing activation. Follows the same per-activation shape as STG. |
| `CancellationToken` | RL | Unified cancellation propagation from transport → hub → activation. Details in RL's spike. |

### Terminology

- **Plexus RPC** — protocol name. Always.
- **DynamicHub** — in-process router. Always.
- **Activation** — service implementing `plexus_core::Activation`.
- **Hub activation** — activation with children (via `#[plexus_macros::child]` methods).
- **Child gate** — a `DynamicChild` method; the name-parameter lookup point.

**Never say "hub RPC".**

### Macro attributes (pinned canonical forms)

- `#[plexus_macros::activation(namespace = "...", version = "...")]`
- `#[plexus_macros::method]` — regular RPC method
- `#[plexus_macros::child]` — static child (zero-arg)
- `#[plexus_macros::child(list = "...", search = "...")]` — dynamic child with opt-in capabilities
- `#[plexus_macros::removed_in("X")]` — companion to `#[deprecated(...)]` for `removed_in` versioning

**Deprecated (still accepted with warnings; removed in plexus-macros 0.6):**
- `hub = true`
- `children = [field, ...]`
- `crate_path = "..."` — redundant after CHILD-6's auto-resolution

### Deprecation policy

- Deprecation messages include `since: String`, `removed_in: String`, `message: String`.
- `since` matches the version where the deprecation lands.
- `removed_in` is a **plan**, not a promise — lives in the schema so consumers can plan, but may slip.
- Crate versions bump in the **same commit** as the work adding the deprecation (see `feedback_version_bumps_as_you_go.md` in memory).
- Downstream dependency declarations in sibling Cargo.toml files are audited via `rg` sweep and bumped in the same PR.

### Substrate `.cargo/config.toml` patch

Hyperforge-generated. Gitignored. Must include (currently hand-maintained by agents when they hit missing patches):

```toml
[patch.crates-io]
plexus-core = { path = "/Users/shmendez/dev/controlflow/hypermemetic/plexus-core" }
plexus-macros = { path = "/Users/shmendez/dev/controlflow/hypermemetic/plexus-macros" }
plexus-transport = { path = "/Users/shmendez/dev/controlflow/hypermemetic/plexus-transport" }
plexus-registry = { path = "/Users/shmendez/dev/controlflow/hypermemetic/plexus-registry" }
cllient = { path = "/Users/shmendez/dev/controlflow/juggernautlabs/cllient" }
```

Hyperforge's autogen pass misses some of these. When a new cross-repo dep is added, confirm the patch entry exists locally before running `cargo build`.

---

## Epic-by-epic scope (brief)

Each sub-section is a 2–4 sentence sketch. Detailed scope lives in each epic's `<EPIC>-1.md` overview.

### IR (mostly done)

Unify Plexus RPC IR around methods with roles. Deprecation metadata flows from macros → wire → synapse (rendering) and synapse-cc / hub-codegen (codegen annotations). Typed-handle codegen for `MethodRole::DynamicChild` emits capability-intersection types. **10 follow-ups Pending** — see `IR-10..19`.

### SYN (synapse consumes capabilities at runtime)

Synapse calls `list_children` / `search_children` during tab-completion; uses the role metadata to distinguish static children, dynamic child gates, and RPC methods in the tree view. Epic overview + 2 implementation tickets Pending.

### HASH (runtime hash aggregation)

Move child-schema hashes out of the `ChildSummary` wire format; each activation exposes `plugin_hash()` at runtime; parent aggregates tolerantly over children, skipping failures. Aligns hash semantics with the graph-not-tree mental model. **Overview only on disk; sub-tickets to be filed before execution.**

### DC (decoupling)

Activations call each other as in-process Rust libraries against curated public APIs, not by reaching into sibling internals. Specific coupling sites to untangle: Orcha→Loopback (storage reach-in), Orcha→ClaudeCode (concrete import), Cone→Bash (concrete import), Cone/ClaudeCode/Orcha→Arbor (schema walking). One decision: convention-only vs workspace split. See `feedback_activation_coupling.md` in memory.

### TM (ticketing activation)

Linear-style ticket management as a Plexus RPC activation. Standalone (CRUD, queries, streams, human promotion gate) and Orcha-integrated (Orcha pulls ready tickets via RPC instead of reading `plans/` files). Filesystem export for git visibility; DB is source of truth. One-shot importer migrates existing `plans/<EPIC>/*.md` into TM. Namespace name TBD (candidates: `thread`, `tix`, `weave`, `plan`).

### ST (strong typing)

Introduce domain newtypes for every identifier crossing activation boundaries. Follows `skills/strong-typing/SKILL.md`. Critical swap-compiles-silently hazards (SessionId, GraphId, NodeId, StreamId, ApprovalId, ToolUseId, TicketId, WorkingDir, ModelId). Fans out one migration per activation after a foundation ticket defines the types.

### STG (storage abstraction)

Trait-based per-activation storage so SQLite can be swapped for Postgres, in-memory test doubles, or other backends without touching activation logic. Trait-shape spike and hotswap-proof spike gate the fan-out. Uses ST's newtypes for keys where applicable.

### RL (resilience)

Kill `.expect()` chains in `builder.rs`. Replace load-bearing `panic!` / `unreachable!` sites in Orcha and Bash. Fix `let _ = pm.save_*` style error swallowing. End-to-end cancellation tokens (spike-gated). Graceful shutdown with task tracking.

### OB (observability)

Config file loader (TOML). Metrics baseline (Prometheus or OTel). Pagination on `list_*` RPC methods. Structured error context. Streaming protocol versioning (spike-gated).

### IDY (identity, deferred)

PKE-strong cryptographic identity for Plexus RPC nodes. libp2p PeerId pattern. URI form `plexus://{pubkey_multihash}/activation/child/...`. Deferred per user direction. See `feedback_identity_model.md` in memory.

---

## Open coordination questions

These decisions will pin contracts when the relevant epics start ticketing:

1. **DC's convention vs workspace split.** Keep substrate a single crate with lint-enforced boundaries, OR split each activation into its own crate under a workspace so `pub(crate)` enforces? Spike gates this.
2. **TM's namespace name.** `thread` / `tix` / `weave` / `plan`. Aesthetic + ecosystem-fit call.
3. **TM absorbs or coexists with `orcha/pm`.** Two spikes in TM's sub-ticket queue — will decide.
4. **ST's foundation scope.** Single-crate with all newtypes (`plexus-domain` or similar) vs. per-activation type modules. Affects how ST-2's foundation ticket lands.
5. **HASH's removal target version for `ChildSummary.hash`.** Likely `removed_in = "0.7"` to match the broader deprecation plan but pinned as soon as HASH implementation starts.

---

## Pointers

- **Substrate technical debt audit** — `docs/architecture/16670380887168786687_substrate-technical-debt-audit.md`. The original ground-truth map of what's broken / missing. Still faithful.
- **Ticketing skill** — `~/dev/controlflow/hypermemetic/skills/skills/ticketing/SKILL.md`. Rules 10 (file-boundary concurrency) and 11 (split along file boundaries) matter when fanning tickets into parallel execution.
- **Strong-typing skill** — `~/dev/controlflow/hypermemetic/skills/skills/strong-typing/SKILL.md`. ST epic follows this exactly.
- **Planning skill** — `~/dev/controlflow/hypermemetic/skills/skills/planning/SKILL.md`. DAG construction + spike conventions.
- **Memory feedbacks** (`~/.claude/projects/.../memory/`) — Plexus RPC terminology, activation coupling, identity model, file-boundary concurrency, epic completeness, write cleanup tickets immediately, version bumps as you go. Future sessions load MEMORY.md automatically.

---

## How to use this doc when writing tickets

1. Read this doc top-to-bottom.
2. Locate your epic's section. If you're writing a ticket referencing a cross-epic concept (e.g., `SessionId` in RL), use the exact name from the "Pinned cross-epic contracts" section.
3. If you need a concept this doc doesn't cover, add it here first (one-line in a new subsection) and commit before writing the ticket. Never invent a name in a ticket without pinning it here.
4. For deprecation, set `since` to the crate's current version in `Cargo.toml` (bump it first if adding new public surface).
5. Never set `status: Ready` on a new ticket — user promotes. Ref `SKILL.md`.
