---
id: RL-8
title: "Task lifecycle cleanup (no orphan tokio::spawn; every task has an owner)"
status: Pending
type: implementation
blocked_by: []
unlocks: [RL-10]
severity: High
target_repo: plexus-substrate
---

## Problem

Substrate spawns tokio tasks in multiple places without a lifecycle owner. Specific audit-flagged sites:

- `plugin_system/conversion.rs:44, 77` — `tokio::spawn()` with no cancellation token, no join handle retained.
- `health/activation.rs:79, 93` — `tokio::spawn()` with no cancellation token, no join handle retained.

Broader category: the audit notes "spawned tokio tasks can outlive the server." There is no per-activation task set, no `JoinSet`, no mechanism for shutdown to wait on or abort in-flight work. When the server exits (panic, SIGINT, `Drop` of the runtime), these tasks are either abandoned mid-flight or forcibly aborted by the runtime with no cleanup.

## Context

This ticket is intentionally *independent* of RL-S01 and RL-9. The fix here is purely about **ownership**: every `tokio::spawn` in substrate's source tree either:

- Runs under a `CancellationToken` held by the owning activation (forward-compatible with RL-9's propagation mechanism), OR
- Is added to the owning activation's `tokio::task::JoinSet` (or `JoinHandle`-tracking equivalent).

The exact cancellation *wiring* from transport → hub → activation is RL-9's job. RL-8 just ensures every task has a home: a field on some activation or subsystem struct that can be iterated at shutdown time.

Recommended patterns (implementor picks per site):

- **Stateless activation with periodic background work** (e.g., Health's liveness ping): hold a `CancellationToken` + `JoinHandle` on the activation struct; check the token in the spawned future's loop.
- **Plugin-system fire-and-forget conversions** (`plugin_system/conversion.rs`): add a `JoinSet` to the plugin system struct; spawn into the set; drain the set on plugin-system shutdown.
- **Orcha's graph runner spawns** (already has `cancel_registry`): no change in this ticket — RL-9 reconciles the registry with the unified mechanism. RL-8 only touches the orphan sites.

Naming: the `CancellationToken` type is pinned in README's cross-epic contracts table as the RL epic's output. Use `tokio_util::sync::CancellationToken` as the concrete type (widely adopted, trivially compatible with `select!`).

## Required behavior

| Spawn site | Current observable behavior | Required observable behavior |
|---|---|---|
| `plugin_system/conversion.rs:44, 77` | Spawned task runs until completion or runtime drop; no way to await or cancel it | Task is added to a `JoinSet` (or holds a clonable token checked in its loop); plugin-system shutdown drains the set / cancels the token; no orphan. |
| `health/activation.rs:79, 93` | Spawned task runs forever until runtime drop | Task checks a `CancellationToken` held by the `Health` activation; activation's shutdown hook triggers the token and joins the handle. |
| Any additional `tokio::spawn` discovered in the substrate source tree during this ticket | n/a | Each is either brought into a task set / given a token, or if it is legitimately a fire-and-forget micro-task with bounded lifetime (< 100 ms, documented), it is annotated with a comment explaining why no owner is needed. |
| Existing Orcha `cancel_registry`-owned spawns | Work as HEAD | Unchanged by this ticket. |

Shutdown semantics RL-8 establishes (RL-10 will consume these):

- Every activation with spawned tasks exposes either a `shutdown(&self)` method or `Drop` impl that cancels its token / joins its task set within a bounded window. RL-8 defines the **shape** (method signature, token field); RL-10 drives it from the binary.

## Risks

- **Task-set proliferation.** Every activation adopting a `JoinSet` adds a field and potentially an async shutdown path. This is the minimum viable approach — a centralized task registry (e.g., `hub.spawn(...)`) is a larger refactor and is explicitly out of scope.
- **Future RL-9 reconciliation.** Orcha's existing `cancel_registry` uses watch channels. RL-8 does not touch it. RL-9 decides whether to migrate it to the unified `CancellationToken` mechanism or leave it as a specialized per-activation mechanism. Either is acceptable — RL-8 does not pre-empt that call.
- **Test observability.** Asserting "no orphan tasks" in unit tests is hard without `tokio-console` or equivalent. Acceptance criterion 4 uses `tokio_util::task::TaskTracker` or a manual join-handle count check as the mechanism.

## What must NOT change

- The set of RPC method names or request/response shapes on any activation.
- Task behaviour in the success path — all spawned tasks do the same work they do at HEAD, just with a lifecycle owner attached.
- Orcha's `cancel_registry` semantics.
- Existing `cargo test` pass rate.
- Files that do not contain a `tokio::spawn` before this ticket and are not touched by it.

## Acceptance criteria

1. Grep for `tokio::spawn` across the substrate source tree identifies every current site. Implementor annotates each with a one-line comment stating its owner (activation struct name + field) or, for documented fire-and-forget sites, the justification.
2. `plugin_system/conversion.rs` tasks are added to a `JoinSet` (or equivalent) held on the plugin-system struct. Dropping the plugin system aborts / awaits the set.
3. `health/activation.rs` tasks hold a `CancellationToken` on the Health struct. The activation exposes a shutdown method that cancels the token and joins the spawned handle within a 1 s window.
4. A unit test spawns a test substrate with only Health registered, triggers Health's shutdown, and asserts (via `JoinHandle::is_finished()` or a `TaskTracker` count) that the spawned task has exited within 1 s.
5. All existing `cargo test` targets pass.
6. No new `tokio::spawn` is introduced in this ticket's diff without a matching owner annotation.

## Completion

Implementor delivers:

- Patches to `plugin_system/conversion.rs` and `health/activation.rs` (and any other spawn sites discovered during the grep sweep of criterion 1).
- A one-line comment at every remaining `tokio::spawn` site naming its owner.
- At least one unit test per criterion 4.
- `cargo test` output confirming criterion 5.
- Status flip to `Complete` in the same commit that lands the code.
