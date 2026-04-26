---
id: HF-IR-3
title: "#[child] gate: workspace (static) under HyperforgeHub"
status: Pending
type: implementation
blocked_by: [HF-IR-2]
unlocks: [HF-IR-4]
severity: Medium
target_repo: hyperforge
---

## Problem

`HyperforgeHub` (namespace `hyperforge`) currently reaches its single or multiple workspaces via hardcoded references inside `HyperforgeState`. Synapse users cannot address `hyperforge workspace <ws>` as a nested namespace — the workspace is implicit. With `WorkspaceHub` already a distinct activation type, the bridge from `HyperforgeHub` to `WorkspaceHub` should be a static `#[plexus_macros::child]` gate: one child per known workspace, statically registered at startup rather than looked up dynamically.

This ticket adds that static child gate and is the first link in the nested-addressing chain.

## Context

Static `#[child]` (no `list = ...`) attaches a child activation that's known at construction time, mirroring CHILD-3's static-child pattern. Precedent: existing hub-plugin parents pre-IR registered children via hand-written `plugin_children()` impls; post-CHILD-3/IR-17 the macro handles registration. HyperforgeHub presently does neither — the workspace is an implementation detail of `HyperforgeState`. HF-IR-S01's mapping pins whether hyperforge supports multiple workspaces today or only one; this ticket honors that decision.

If single-workspace: the gate is `fn workspace(&self) -> WorkspaceHub` returning the one instance.

If multi-workspace: the gate is parameterized by `WorkspaceName` but still static (the set of workspaces is known at startup). The macro form is `#[plexus_macros::child]` without `list = ...`, registering one child per enumerated workspace instance. If HF-IR-S01 determines the set of workspaces is not actually fixed at startup, this ticket converts to dynamic with `list = "workspace_names"` — ticket scope includes either shape.

Handle back to parent storage: `WorkspaceHub` already holds its storage handle. This ticket rewires construction so `HyperforgeHub` owns the `WorkspaceHub` instance(s) as child activations visible through the gate.

## Required behavior

After this ticket:

| Invocation | Behavior |
|---|---|
| `synapse hyperforge` tree-lists top-level methods and includes `workspace` as a child entry (static or dynamic per HF-IR-S01). | Shown in the nested tree. |
| `synapse hyperforge workspace <name>` (multi-workspace) or `synapse hyperforge workspace` (single) returns a `WorkspaceHub` activation view, exposing `WorkspaceHub`'s methods as-is. | Same method set as pre-ticket. |
| Any flat `HyperforgeHub` method that reaches into the workspace today (e.g., `status`, `refresh`) continues to work unchanged at the wire layer. | No deprecation in this ticket — HF-IR-9 handles deprecation wiring. |
| `plugin_schema()` on `HyperforgeHub` contains a method/child entry named `workspace` with role `MethodRole::StaticChild` (or `MethodRole::DynamicChild { list_method: "workspace_names", search_method: None }` if HF-IR-S01 pinned dynamic). | Assertable in a new test. |

## Risks

| Risk | Mitigation |
|---|---|
| `HyperforgeHub` already has a flat method named `workspace` (or `workspaces`). | Spike surfaced naming conflicts. Rename the flat method (with a deprecation shim in HF-IR-9) or pick a different gate name (`ws` is a fallback — HF-IR-S01 decides). |
| Static-child registration with multi-workspace needs per-workspace construction at startup. | The workspace list is materialized from config when `HyperforgeHub` is constructed; iterate and register each. If construction is async, follow the pattern used by existing hub-plugin parents that return futures. |
| Some workspace-spanning methods on `HyperforgeHub` (e.g., `refresh`) cannot be pushed into `WorkspaceHub` without changing semantics. | Those methods stay on `HyperforgeHub`. This ticket only adds the gate; method migration (if any) is out of scope. |

## What must NOT change

- Every existing method signature and wire behavior on `HyperforgeHub` and `WorkspaceHub`.
- `HyperforgeState`'s internal layout beyond wiring through the new gate.
- Hyperforge CLI top-level grammar — users invoking existing methods see no change.
- Activation namespaces at root (`hyperforge`, `auth`, `secrets`).

## Acceptance criteria

1. `cargo build --workspace` in hyperforge passes.
2. `cargo test --workspace` in hyperforge passes.
3. `cargo build` green in every sibling repo that depends on hyperforge.
4. A test (or an extended existing test) asserts that `plugin_schema()` on `HyperforgeHub` contains a child entry named `workspace` with the correct `MethodRole` per HF-IR-S01.
5. An integration test or end-to-end fixture demonstrates that invoking an existing `WorkspaceHub` method via `hyperforge workspace <name> <method>` (or `hyperforge workspace <method>` if single) resolves to the same result as invoking that method directly on `WorkspaceHub` pre-ticket.
6. Hyperforge version remains at `4.2.0` (bumped in HF-IR-2).
7. `synapse hyperforge` tree output lists `workspace` as a child entry and nested invocations route correctly (manual verification sufficient; automated synapse verification is HF-IR-10).

## Completion

Commit lands the gate + any wiring changes in `hubs/hyperforge.rs` (or equivalent) and `hubs/workspace.rs` only. `cargo build --workspace` + `cargo test --workspace` green. Status flipped to Complete in the same commit.
