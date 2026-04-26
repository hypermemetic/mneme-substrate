---
id: IR-1
title: "Unify Plexus RPC IR around methods with roles + deprecation metadata"
status: Epic
type: epic
blocked_by: []
unlocks: []
target_repo: cross-cutting
---

## Goal

Reshape the Plexus RPC intermediate representation (IR) around a single concept: **methods with roles**. Today `PluginSchema` carries `methods: Vec<MethodSchema>` alongside three derived side-tables — `children: Vec<ChildSummary>`, `is_hub: bool`, and `ChildCapabilities` bitflags. These are all expressible as properties of methods tagged with a role: `Rpc`, `StaticChild`, or `DynamicChild { list_method, search_method }`. Collapsing the IR to a single role-tagged method list removes duplication, makes the schema self-describing at the method level, and aligns the IR with the "graph, not tree" mental model: nothing in the parent schema is cached information about children.

A second concern lands in the same epic: every deprecated IR surface must carry structured deprecation metadata — `since` and `removed_in` version strings plus a human-readable message — and that metadata must propagate through synapse (inline warnings) and synapse-cc (codegen annotations) so consumers can plan migrations.

A third concern, also in this epic: synapse-cc's codegen output for `MethodRole::DynamicChild` must be a first-class typed handle (`DynamicChild<T>` plus opt-in capability interfaces like `Listable` and `Searchable`), not a flat method on the parent client. Callers invoking a capability that the activation didn't opt into get a compile-time error in the target language, not a runtime throw.

## Context

**Upstream work already landed:**

| Commit | Repo | Scope |
|---|---|---|
| `624e3c3d` | plexus-core | CHILD-2: ChildRouter capabilities / list_children / search_children |
| `7c2e8ac` | plexus-macros | CHILD-3: `#[child]` attribute |
| `b0b7bf4` | plexus-macros | CHILD-4: list/search opt-in |
| `bd03051` | plexus-macros | CHILD-5: doc-comment extraction |
| `5f847b3` | plexus-macros | CHILD-6: `crate_path` auto-resolve |
| `eac7cb2f` | plexus-macros | CHILD-8: hub-mode inference |
| `e8da2351` | plexus-substrate | CHILD-7: Solar migration |

**Target unified IR shape (pinned):**

```rust
struct PluginSchema {
    namespace: String,
    description: String,
    version: String,
    methods: Vec<MethodSchema>,
    // Deprecated fields — populated from methods during transition window:
    children: Vec<ChildSummary>,            // #[deprecated(since = "0.5", removed_in = "0.6")]
    is_hub: bool,                           // #[deprecated(since = "0.5", removed_in = "0.6")]
}

struct MethodSchema {
    name: String,
    role: MethodRole,
    params: Vec<ParamSchema>,
    return_shape: ReturnShape,              // Bare / Option / Result / Vec / Stream
    description: String,
    deprecation: Option<DeprecationInfo>,
}

enum MethodRole {
    Rpc,
    StaticChild,
    DynamicChild {
        list_method: Option<String>,
        search_method: Option<String>,
    },
}

struct DeprecationInfo {
    since: String,              // plexus-core version when deprecation began
    removed_in: String,          // plexus-core version planned for removal
    message: String,             // human-readable migration hint
}
```

**Deprecation policy (pinned by the user):**

- Backward compatibility is maintained until at least the next major plexus-core release (current is 0.4; expected removal window is 0.6 or 1.0 — the precise target is stored per-field in `DeprecationInfo.removed_in`).
- Every deprecated field carries both `since` and `removed_in`.
- Lifecycle is "fast and loose" — `removed_in` is a plan, not a promise, but it lives in the schema so consumers can track.
- Synapse surfaces deprecation warnings inline when users touch deprecated fields.
- synapse-cc (codegen CLI) flags generated code that consumes deprecated fields. Default severity: WARNING (stderr, non-fatal). `--fail-on-deprecated` escalates to a hard error.

## Dependency DAG

```
  IR-S01   IR-S02   IR-S03           IR-2
 (synapse (syn-cc  (intersection   (plexus-core:
  render   codegen  typing per     MethodRole +
  spike)   spike)   language)      DeprecationInfo)
    │        │        │               │
    │        │        │        ┌──────┴──────┐
    │        │        │        ▼             ▼
    │        │        │     IR-3           IR-5
    │        │        │  (macros         (macros
    │        │        │   emit role)     deprecation)
    │        │        │        │             │
    │        │        │        ▼             │
    │        │        │     IR-4             │
    │        │        │  (plexus-core        │
    │        │        │   shim)              │
    │        │        │        │             │
    ▼        ▼        ▼        │             │
   IR-6   IR-7+IR-9  IR-9      ▼             │
                           ┌───┼─────────┬───┘
                           ▼   ▼         ▼
                         IR-8 (as above) (consumers run
                                          in parallel)
```

Spikes gate the high-risk implementation tickets:
- **IR-S01** blocks IR-6 (synapse rendering).
- **IR-S02** blocks IR-7 and IR-9 (both need synapse-cc emission extensibility).
- **IR-S03** blocks IR-9 (capability-intersection viability per target language).

Spike execution is parallel (three different investigation axes, no shared files); they must complete before their downstream implementation tickets can be promoted.

- IR-3 and IR-5 both touch `plexus-macros`. They have overlapping file scope — see each ticket's Risks section. Recommendation: land IR-3 first, then IR-5 against the resulting files.
- IR-6, IR-7, IR-8, IR-9 are independent target repos after IR-3/IR-4/IR-5 land — they can proceed in parallel.
- IR-7 and IR-9 both target synapse-cc but touch different codegen concerns (deprecation annotation emission vs typed-handle generation for dynamic children). They may collide at the file level; check file-boundary disjointness before firing concurrently.

## Phase Breakdown

| Phase | Tickets | Notes |
|---|---|---|
| 1. Types | IR-2 | Add `MethodRole`, `DeprecationInfo`, extend `MethodSchema`. Pure additive. |
| 2. Codegen | IR-3 → IR-5 | Macros emit role tags, then deprecation metadata. Serialize on the macros crate. |
| 3. Shim | IR-4 | Keep `children` / `is_hub` on the wire but populate them from role-tagged methods. |
| 4. Consumers | IR-6, IR-7, IR-8 | Parallel — synapse, synapse-cc, substrate each consume independently. |

## Tickets

| ID | Summary | Target repo | Status |
|---|---|---|---|
| IR-1 | This epic overview | — | Epic |
| IR-S01 | Spike: synapse deprecation rendering viability | synapse | Pending |
| IR-S02 | Spike: synapse-cc codegen extensibility | synapse-cc | Pending |
| IR-S03 | Spike: capability-intersection typing viability per target language | synapse-cc | Pending |
| IR-2 | `MethodRole` + `DeprecationInfo` in plexus-core; extend `MethodSchema` | plexus-core | Pending |
| IR-3 | plexus-macros emits `MethodRole` + deprecation on generated `MethodSchema` | plexus-macros | Pending |
| IR-4 | Backward-compat shim: populate deprecated `PluginSchema` fields from methods | plexus-core | Pending |
| IR-5 | Deprecation metadata capture in plexus-macros | plexus-macros | Pending |
| IR-6 | Synapse surfaces deprecation warnings inline | synapse | Pending |
| IR-7 | synapse-cc tracks IR version and flags deprecated consumption in codegen | synapse-cc | Pending |
| IR-8 | Substrate: Solar migrates to method-role IR; tests updated | plexus-substrate | Pending |
| IR-9 | synapse-cc: typed-handle codegen for dynamic children (`DynamicChild<T>` + capability intersections) | synapse-cc (+ per-language runtime) | Pending |

## Out of scope

- **IDY epic** (identity / strong signing). Separate epic, unaffected by this work.
- **CHILD tickets that do not interact with the IR** — CHILD-9 et seq are untouched.
- **HASH tickets that do not interact with the IR** — runtime `plugin_hash()` aggregation is HASH-1's territory; the schema surgery portion of HASH has folded into this epic.
- Cryptographic or identity semantics attached to methods. Deprecation metadata is purely informational.
- Wire format breaking changes. `PluginSchema` remains serde-compatible across the epic; the deprecated fields remain on the wire until a future plexus-core major release.

## Supersedes

**CHILD-10** (`plugin_children` override as a child macro extension) is **superseded by this epic**. The unifying IR removes `plugin_children` from the schema entirely — children are derived from role-tagged methods — which obviates the override concept CHILD-10 was designed to solve. CHILD-10's frontmatter has been flipped to `status: Superseded` with `superseded_by: IR-1`.

## What must NOT change

- Wire compatibility with pre-IR clients: `PluginSchema` continues to serialize `children`, `is_hub`, and `ChildCapabilities` through the transition window (IR-4 pins this).
- All existing activations in substrate compile and pass tests through every phase of this epic.
- The handle / routing / transport layers of plexus-core are untouched.
- Synapse's behavior against a schema with no deprecation metadata is identical to its behavior before this epic.
- synapse-cc's codegen output against a pre-IR schema (no version field) is byte-identical to current output.

## Completion

Epic is Complete when IR-2 through IR-9 are all Complete and:

- `plexus-core` exports `MethodRole` and `DeprecationInfo`; `MethodSchema` carries both.
- `plexus-macros` emits role-tagged methods and captures `#[deprecated]` on activations, methods, and input-type fields.
- `PluginSchema.children` and `PluginSchema.is_hub` are populated from role-tagged methods, marked `#[deprecated]` with `since` and `removed_in`, and still serialize on the wire.
- Synapse prints a visible deprecation marker when rendering or invoking deprecated surfaces.
- synapse-cc annotates generated code that consumes deprecated fields; `--fail-on-deprecated` escalates to error.
- Substrate's Solar activation has migrated its tests to role-based queries, its hand-written `plugin_children()` is annotated `#[deprecated(since = "0.5", removed_in = "0.6")]`, and all substrate tests pass.
- A demo transcript in the final PR shows synapse rendering a deprecated method with the warning marker and printing the migration message on invocation.
