---
id: OB-1
title: "Observability — config, metrics, pagination, structured errors, streaming versioning"
status: Epic
type: epic
blocked_by: []
unlocks: []
target_repo: plexus-substrate
---

## Goal

End state: substrate has grown the production-shaped observability systems the technical-debt audit flagged as absent. Specifically:

- A **TOML config file** at `~/.plexus/substrate/config.toml` with per-activation sections and env-var overrides. Activations pull their configuration from this loader instead of `Default::default()` plus ad-hoc env-var reads scattered across the tree. DB paths, ports, and timeouts become explicit, discoverable, and overridable without recompiling.
- A **metrics baseline** (Prometheus or OpenTelemetry; single spike gates the choice — resolved in this epic's implementation ticket) exposing counters per RPC call (labeled by activation and method), latency histograms, and a handful of business counters (agents spawned, tokens used, approvals granted). `/metrics` is served on a separate port so substrate's main RPC surface stays uncontaminated.
- **Pagination** on every unbounded list RPC (`orcha.list_graphs`, `pm.list_ticket_maps`, and any other list method discovered during the sweep). Lists accept `limit` / `offset` (or cursor — pinned in the ticket), return the requested page plus a total count, and old "return everything" callers migrate to the first page.
- **Structured error context on the wire.** Errors carry structured fields — activation id, method, graph id, session id, stream id, etc., where relevant — instead of collapsing to a flat `String`. Per-activation error enums already exist (`ConeError`, `OrchaError`, etc., per the audit's "patterns that work" section); this epic surfaces their structure on the wire rather than flattening at the dispatch boundary.
- **Streaming protocol versioning.** `PlexusStreamItem` gains a version discriminator (strategy chosen by OB-S01 — `v:` field vs feature handshake vs per-method negotiation). Adding a new variant no longer silently breaks old clients. Old clients continue to work against versioned servers; new clients can negotiate new capabilities.

This epic closes the "missing systems" section of the technical-debt audit (`docs/architecture/16670380887168786687_substrate-technical-debt-audit.md`). When it ships, the operator story for substrate moves from "read the source to find out what env vars apply" to "read one config file and one `/metrics` scrape".

## Context

The audit enumerated five gaps this epic addresses. In the audit's words:

| Gap | Audit summary |
|---|---|
| Config file | Absent. All activations use `Default` plus ad-hoc env vars. DB paths, ports, timeouts scattered. |
| Metrics | Absent. Tracing is extensive (128+ spans) but zero counters, histograms, or `/metrics` endpoint. |
| Pagination | Absent on `orcha.list_graphs`, `pm.list_ticket_maps`, and every other list method. Everything returns full sets. |
| Structured errors on wire | Per-activation error enums exist but are flattened to `String` at the wire. |
| Streaming protocol versioning | `PlexusStreamItem` has no version discriminator. |

**Audit drift note.** The audit's file:line references (`orcha/graph_runner.rs`, `builder.rs`, etc.) are pointers, not ground truth. Each implementation ticket re-verifies its target sites against HEAD before proceeding.

**Cross-epic touchpoints:**

- **ST (strong typing).** ST's newtypes (`GraphId`, `SessionId`, `StreamId`, etc.) appear in OB-5's structured error context fields. If ST ships before OB-5 starts, OB-5 uses the newtypes directly. If OB-5 ships first, ST's migration updates the already-structured error context fields to newtypes without reshaping the contract.
- **RL (resilience).** RL's ticket to kill `.expect()` chains and replace `panic!` / `unreachable!` sites overlaps with OB-5's structured-error surfacing: RL ensures errors are **typed** (no silent `String::from` collapses), OB-5 ensures the typed errors **surface on the wire** with structured fields. Coordinate so neither epic re-flattens what the other structured. Concretely: OB-5 lands after (or alongside) RL's error-typing work on the same activation; if RL hasn't reached an activation yet, OB-5 inherits that activation's current shape and ST/RL later migrate without reflattening.
- **STG (storage abstraction).** STG's storage configuration story — `storage.kind` (SQLite / Postgres / in-memory), `storage.dir`, connection strings — is part of OB-2's config schema. OB-2 reserves the `[storage]` and per-activation `[<name>.storage]` sections; STG populates the keys those sections expose when it defines per-activation storage traits. If STG ships before OB-2, OB-2 inherits the key names STG picked. If OB-2 ships first, OB-2 pins placeholder keys and STG renames as needed at its landing.
- **IR / CHILD / SYN.** No dependencies. OB does not touch schemas, children, or synapse behavior.

## Dependency DAG

```
                         OB-S01 (spike: streaming versioning strategy)
                              │
          ┌───────────────────┼───────────────────┐
          │                   │                   │
          ▼                   ▼                   ▼
        OB-2                OB-3                OB-4                OB-5
     (config             (metrics            (pagination        (structured
      file)               baseline)           on list methods)   error context)
          │                   │                   │                   │
          │                   │                   │                   │
          └───────────────────┴───────────────────┴───────────────────┘
                                        │
                                        ▼
                                      OB-6
                          (streaming protocol versioning impl —
                           depends on OB-S01's decision)
```

- **OB-S01** is the only blocker for OB-6. It does not gate OB-2, OB-3, OB-4, or OB-5 — those fan out in parallel once the epic is promoted.
- **OB-2..OB-5** are four parallel implementation tickets. File-boundary check:
  - OB-2 touches `src/main.rs`, `src/builder.rs`, and introduces new config-loader modules. Each activation's `mod.rs` is read but the main modifications concentrate in the new loader module and `builder.rs`.
  - OB-3 introduces a new `metrics` module, modifies `src/main.rs` (to bind the `/metrics` port), and adds instrumentation at the dispatch boundary. Touches dispatch code, not activation internals.
  - OB-4 touches `list_*` method implementations inside each affected activation (`orcha/activation.rs`, `pm/activation.rs`, and any others discovered). Disjoint from OB-2 and OB-3.
  - OB-5 touches the dispatch boundary's error-serialization path and each activation's error enum surface. File overlap with OB-2 on `builder.rs` is minimal — OB-2 adds config wiring in startup; OB-5 does not touch startup.
  - **File-boundary concurrency:** OB-2 and OB-5 both potentially touch `builder.rs`. Land OB-2 first; OB-5 works against the resulting file. If the risk is acceptable at ticket-write time (contact points in `builder.rs` are small and separable), these can land independently — re-check during ticket promotion.
- **OB-6** depends solely on OB-S01. Its implementation can overlap OB-2..OB-5 once the spike is done; it lands last if file contention emerges with OB-3 (both touch dispatch).

## Phase Breakdown

| Phase | Tickets | Notes |
|---|---|---|
| 0. Decision | OB-S01 | Binary spike: pick streaming-protocol versioning strategy. |
| 1. Parallel fan-out | OB-2, OB-3, OB-4, OB-5 | Four independent implementations. Any or all can be in flight simultaneously. |
| 2. Versioning impl | OB-6 | Serial after OB-S01. Can overlap phase 1 once S01 lands. |

## Tickets

| ID | Summary | Status |
|---|---|---|
| OB-1 | This epic overview | Epic |
| OB-S01 | Spike: streaming protocol versioning strategy | Pending |
| OB-2 | Config file loader (TOML) with per-activation sections and env overrides | Pending |
| OB-3 | Metrics baseline — counters, histograms, `/metrics` endpoint | Pending |
| OB-4 | Pagination on unbounded list RPC methods | Pending |
| OB-5 | Structured error context on the wire | Pending |
| OB-6 | Streaming protocol versioning implementation (depends on OB-S01) | Pending |

## Out of scope

- **Rate limits / quotas.** The audit listed these under "missing systems" alongside OB's scope, but they are not observability — they are resource-governance. Tracked as a future epic, not here.
- **Property / fuzz / load tests.** Also listed in the audit's "missing systems". Testing posture is orthogonal to observability. Out of scope for OB.
- **Distributed tracing (W3C TraceContext, OTel span propagation across nodes).** OB-3 ships local metrics. Cross-node trace propagation is a future epic when substrate grows multi-node concerns.
- **Log aggregation / shipping.** Existing `tracing` spans already emit to stderr/file; shipping to Loki/Datadog/etc. is deployment concern, not in-substrate code.
- **Dashboards.** OB-3 exposes `/metrics`; the Prometheus / Grafana dashboards that consume it are operator-side, not shipped in this repo.
- **Alerting.** Same reasoning as dashboards.
- **Config hot-reload.** OB-2 reads config at startup. SIGHUP / inotify-based reload is a follow-up if it becomes load-bearing.
- **Wire-format migration for existing non-streaming methods.** OB-5 structures error context; OB-6 versions streaming. Non-streaming request/response shapes are unchanged.

## Cross-epic references

- **Audit document.** `docs/architecture/16670380887168786687_substrate-technical-debt-audit.md`, "Missing systems" section — the enumeration this epic closes.
- **README pinned decisions.** OB inherits ST's newtype names (`GraphId`, `SessionId`, `StreamId`, `ApprovalId`, `ToolUseId`, `TicketId`, etc.) for structured-error field types once ST lands. STG's storage-config key names (`storage.kind`, `storage.dir`) appear in OB-2's config schema once STG lands.
- **RL epic.** Error typing overlap — see "Cross-epic touchpoints" above.
- **STG epic.** Storage-config schema ownership — OB-2 reserves the section names; STG populates the keys.

## What must NOT change

- Wire protocol for non-streaming RPC methods. OB-5 surfaces structured error context, but the **success** response shape for every method is untouched.
- Existing `PlexusStreamItem` variants' serialization. OB-6 adds a version discriminator per OB-S01; it does not rename or restructure existing variant payloads.
- Activation namespace strings, method names, schema hashes.
- Default ports. Substrate continues to bind its main RPC port (4444); the `/metrics` port OB-3 introduces is **additional**, not a replacement.
- SQLite-per-activation layout. OB-2 makes paths configurable but the default paths are the current `~/.plexus/substrate/activations/{name}/` layout.
- Existing `cargo test` pass rate across every activation.
- Startup order in `builder.rs`. OB-2 wires config **into** the existing startup; it does not reshape startup dependencies.

## Completion

Epic is Complete when OB-S01 is Complete and OB-2, OB-3, OB-4, OB-5, OB-6 are all Complete. Deliverables:

- A `~/.plexus/substrate/config.toml` with a documented schema, loaded at substrate startup and consumed by every activation.
- A `/metrics` endpoint on a dedicated port exposing per-method RPC counters, latency histograms, and business counters.
- Every `list_*` RPC method across substrate accepts pagination parameters and returns a page plus a total count.
- Every error response on the wire carries structured fields (activation id, method, and any domain-specific ids like graph/session) instead of a flat string.
- `PlexusStreamItem` JSON carries a version discriminator; a synthetic old-client/new-server interop test passes; a synthetic new-client/old-server interop test degrades gracefully.

When all five land, the technical-debt audit's "Missing systems" section marks Config, Metrics, Pagination, Structured error context, and Streaming protocol versioning as resolved; remaining items (rate limits, property tests, fuzz, load tests, per-activation unit test coverage, graceful shutdown) are explicitly out of OB's scope and flow to other epics.
