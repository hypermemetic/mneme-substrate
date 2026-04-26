---
id: OB-3
title: "Metrics baseline — counters, histograms, /metrics endpoint"
status: Pending
type: implementation
blocked_by: []
unlocks: []
severity: High
target_repo: plexus-substrate
---

## Problem

Substrate has 128+ `tracing` spans but zero counters, histograms, or `/metrics` endpoint. An operator asking "how many Orcha graphs were spawned this hour?", "what's the p99 latency of `cone.complete`?", or "how many approvals has Loopback granted today?" has no mechanical answer — they must `grep` through stdout logs.

This ticket adds a metrics baseline: per-RPC-call counters (labeled by activation and method), latency histograms (per activation+method), and a handful of business counters (agents spawned, tokens used, approvals granted). Metrics are exposed on a dedicated port via a `/metrics` HTTP endpoint in the chosen format (Prometheus text, OTel OTLP, or both — single-vendor spike resolved inline; see Context).

## Context

**Vendor choice (pinned for this ticket, no spike).** Use the **`metrics` facade crate** (`metrics = "0.23"` or current) with the **`metrics-exporter-prometheus` exporter**. Rationale:

- `metrics` is the Rust-ecosystem equivalent of `log` / `tracing` — a facade other instrumentation can swap exporters under.
- The Prometheus exporter produces `/metrics` text-format compatible with Prometheus, Grafana Agent, VictoriaMetrics, OTel collectors with Prometheus receiver, and most operator tooling.
- OTel OTLP is future work — the facade lets us add `metrics-exporter-otlp` without changing instrumentation call sites.

No spike is needed on vendor choice. If during implementation the chosen crates prove non-viable (version yanked, incompatible with substrate's tokio/Rust version), the ticket escalates to a spike; until then, proceed.

**Metric naming (pinned):**

Use Prometheus-style `snake_case` with `substrate_` prefix. Labels are `activation` (namespace string) and `method` (method name). Examples:

| Metric | Type | Labels | Purpose |
|---|---|---|---|
| `substrate_rpc_calls_total` | counter | `activation`, `method`, `outcome` (`"ok"` / `"err"`) | Total RPC calls, broken down by outcome. |
| `substrate_rpc_duration_seconds` | histogram | `activation`, `method` | Latency per call. Default buckets. |
| `substrate_rpc_in_flight` | gauge | `activation` | Currently-in-flight RPC calls per activation. |
| `substrate_stream_items_total` | counter | `activation`, `method` | Stream items emitted. |
| `substrate_stream_active` | gauge | `activation` | Currently-open streams per activation. |
| `substrate_orcha_agents_spawned_total` | counter | `model` | Business: Claude Code agents spawned by Orcha. |
| `substrate_orcha_tokens_used_total` | counter | `model`, `kind` (`"input"`/`"output"`) | Business: tokens consumed. |
| `substrate_loopback_approvals_total` | counter | `outcome` (`"granted"`/`"denied"`/`"timeout"`) | Business: approval decisions. |
| `substrate_build_info` | gauge (value = 1) | `version`, `git_sha` | Build identification. |

Additional per-activation business metrics can be added by the activation author; the ticket does not exhaustively enumerate every possible counter. The metrics above are the **required baseline**.

**Port and endpoint.**

| Key | Value |
|---|---|
| Metrics port | `9090` (default; overridable via `[server] metrics_port` after OB-2) |
| Metrics path | `/metrics` |
| Protocol | HTTP (not HTTPS; operator puts behind a reverse proxy if they need TLS) |
| Bind address | Same bind address as the main RPC port (loopback by default) |

The metrics port is **separate from the main RPC port** so that:
- Main RPC traffic and metrics scrapes don't interfere.
- Operators can firewall them independently.
- The main RPC surface stays protocol-pure.

**Instrumentation sites.**

Primary instrumentation is at the **DynamicHub dispatch boundary** — one shared code path measures every RPC call regardless of activation. Per-activation business metrics live inside activation code at the events that matter (agent spawn, approval resolution, etc.).

The dispatch-boundary shim follows this shape (illustrative, not prescriptive):

```rust
// in DynamicHub dispatch path
let start = Instant::now();
metrics::counter!("substrate_rpc_in_flight", "activation" => ns).increment(1);
let result = activation.handle(method, args).await;
let duration = start.elapsed();
metrics::histogram!("substrate_rpc_duration_seconds",
    "activation" => ns, "method" => method).record(duration.as_secs_f64());
metrics::counter!("substrate_rpc_calls_total",
    "activation" => ns, "method" => method,
    "outcome" => if result.is_ok() { "ok" } else { "err" }).increment(1);
metrics::gauge!("substrate_rpc_in_flight", "activation" => ns).decrement(1);
```

Exact API follows the `metrics` crate documentation; the ticket's responsibility is that the labels and metric names match the pinned table.

## Required behavior

### Endpoint

| Request | Response |
|---|---|
| `GET /metrics` on the metrics port | HTTP 200; content-type `text/plain; version=0.0.4`; body is Prometheus text-format output containing every registered metric. |
| `GET /metrics/anything-else` | HTTP 404. |
| `GET /` on the metrics port | HTTP 200 with a short HTML page listing `/metrics` as the only endpoint. |
| Any non-GET method | HTTP 405. |

### Instrumentation coverage

| RPC path | Instrumentation |
|---|---|
| Every method on every activation | `substrate_rpc_calls_total` incremented with outcome label. |
| Every method on every activation | `substrate_rpc_duration_seconds` observed. |
| Every streaming method | `substrate_stream_items_total` incremented per emitted item; `substrate_stream_active` gauge incremented on stream start, decremented on stream close (whether clean, error, or client-disconnect). |
| Orcha agent spawn | `substrate_orcha_agents_spawned_total{model="..."}` incremented. |
| Orcha tokens used (reported per Claude API response) | `substrate_orcha_tokens_used_total{model, kind}` incremented by the reported count. |
| Loopback approval resolution | `substrate_loopback_approvals_total{outcome}` incremented. |

Activations that don't yet have business metrics (Echo, Health, Chaos, Bash, Interactive, Arbor, Cone, ClaudeCode — for metrics beyond the baseline, PM, Mustache, Lattice, MCP, Registry) inherit the baseline RPC metrics automatically via the dispatch shim. Business metrics for those activations are follow-up tickets.

### Startup behavior

| Condition | Result |
|---|---|
| Metrics port binds cleanly | Substrate logs `"metrics server bound to 127.0.0.1:9090"` at `info!`. |
| Metrics port already in use | Substrate logs an `error!` with the conflict, continues running **with metrics disabled**. RPC service keeps working. This is a degraded mode, not a fatal error — the operator can restart with a different port via OB-2's config. |
| Metrics disabled via config (`[server] metrics_enabled = false` after OB-2) | Substrate does not bind the port; `metrics::*` macro calls become no-ops via the `metrics` facade's disabled-recorder pattern. |

### Scrape shape

Running `curl http://127.0.0.1:9090/metrics` against a substrate that has served at least one RPC call returns Prometheus text format including at minimum:

```
# HELP substrate_rpc_calls_total Total RPC calls
# TYPE substrate_rpc_calls_total counter
substrate_rpc_calls_total{activation="echo",method="ping",outcome="ok"} 1

# HELP substrate_rpc_duration_seconds RPC call latency
# TYPE substrate_rpc_duration_seconds histogram
substrate_rpc_duration_seconds_bucket{activation="echo",method="ping",le="0.005"} 1
# ... buckets ...
substrate_rpc_duration_seconds_sum{activation="echo",method="ping"} 0.0001
substrate_rpc_duration_seconds_count{activation="echo",method="ping"} 1

# HELP substrate_build_info Build information
# TYPE substrate_build_info gauge
substrate_build_info{version="0.4.0",git_sha="..."} 1
```

## Risks

| Risk | Mitigation |
|---|---|
| `metrics-exporter-prometheus` version pins conflict with tokio or axum versions substrate uses. | Check at implementation; if conflict, pick the closest-compatible version or switch to the `prometheus` crate directly. Spike only if both paths fail. |
| Instrumenting at dispatch requires modifying code shared with the hub library (not substrate-internal). | The dispatch-boundary code lives in substrate (`src/builder.rs` / dispatch glue); if it actually lives in `plexus-core`, the shim moves there and the PR spans both repos. Re-verify at implementation. |
| High-cardinality labels (e.g., graph IDs as labels) blow up the metric store. | Labels are restricted to activation name and method name — both low-cardinality. Domain IDs (graph, session) never appear as labels; if they need to be observable, they appear in structured logs (OB-5), not metrics. |
| Stream active/closed gauge drifts if a stream closes without the close path running. | Stream tracking uses RAII (a guard struct's `Drop` impl decrements the gauge). Drop runs even on panic or task abort. |
| Duplicate metric registration across hot-reload or restart. | `metrics` facade handles idempotent registration; no-op on duplicate. |
| `/metrics` endpoint becomes a DoS vector (endless queries). | Metrics port binds to loopback by default. Operator who exposes it publicly takes responsibility. |
| Orcha / Loopback business metrics require touching activations already slated for RL or DC changes — file contention. | Business metrics land in small, surgical diffs inside each activation. If a larger refactor is in flight for that activation, land the baseline dispatch-boundary metrics first (no activation code changes) and file follow-ups for the business metrics. |

## What must NOT change

- Wire protocol for any RPC method. Metrics are a side effect; they do not alter request/response shapes.
- Main RPC port binding. Metrics bind to a separate port.
- Activation namespace strings or method names. Metrics labels consume these but do not reshape them.
- Existing `tracing` spans. Metrics are additive; tracing stays.
- Startup success criteria. Metrics port conflict is a warning, not a failure.
- Default behavior when the metrics crate isn't configured (e.g., `metrics` with no recorder) — macro calls are no-ops, zero overhead.

## Acceptance criteria

1. A GET against `/metrics` on the metrics port returns Prometheus text-format output. Verifiable by `curl http://127.0.0.1:9090/metrics | head`.
2. After driving one RPC call (e.g., `synapse echo ping`), the response includes a line like `substrate_rpc_calls_total{activation="echo",method="ping",outcome="ok"} 1`.
3. After driving one streaming call, the response includes non-zero values for `substrate_stream_items_total` and a zero value for `substrate_stream_active` (stream closed after client disconnect).
4. After driving an Orcha graph that spawns a Claude Code agent, the response includes a non-zero `substrate_orcha_agents_spawned_total{model="..."}`.
5. After driving a Loopback approval (either outcome), the response includes a non-zero `substrate_loopback_approvals_total{outcome="granted|denied|timeout"}`.
6. `substrate_build_info` is present with `version` and `git_sha` labels matching the build.
7. Metrics port bound by another process at startup: substrate logs an error, continues to serve RPC traffic without metrics. Verifiable by pre-binding port 9090 and starting substrate; `curl http://127.0.0.1:4444/` (RPC) succeeds; substrate stderr shows the port-conflict error.
8. `cargo test --workspace` passes unchanged.
9. No new high-cardinality labels introduced; grep of `metrics::` call sites shows labels restricted to activation/method/outcome/model/kind — no graph/session/stream ids as labels.
10. A committed example Grafana dashboard JSON at `examples/grafana-dashboard.json` (or documented minimum PromQL queries in `docs/metrics.md`) — the minimum an operator needs to start visualizing. Optional follow-up if scope-tight, in which case the PR description links to the agreed PromQL queries.
11. Metrics can be disabled at startup via the config (after OB-2) with zero code-path overhead beyond the `metrics` facade's no-op calls.

## Completion

PR against `plexus-substrate`. CI green. Demo transcript (captured in PR description) showing a live `/metrics` scrape after driving a representative mix of RPC calls. Status flipped from `Ready` to `Complete` in the same commit that lands the metrics baseline.
