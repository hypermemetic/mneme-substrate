---
id: HF-IR-10
title: "Synapse integration verification: nested tree + deprecation warnings end-to-end"
status: Pending
type: implementation
blocked_by: [HF-IR-9]
unlocks: []
severity: Medium
target_repo: hyperforge
---

## Problem

HF-IR-3..9 landed Rust-side child gates, method extractions, and deprecation metadata. The end-state user experience — `synapse hyperforge` renders the new nested tree with deprecation markers, and invoking a deprecated flat method emits a stderr notice — depends on synapse's rendering (IR-6) and invocation-warning (IR-15) pipelines consuming the new schema correctly over the wire. This ticket verifies the full end-to-end integration against a live hyperforge server, rather than the per-hub unit-level `plugin_schema()` assertions in HF-IR-3..9.

It is the integration gate for the sub-epic. Without it, "HF-IR is Complete" rests on per-hub asserts — not on a user actually seeing the tree.

## Context

Synapse `3.12.0` (or the pinned version at HF-IR's land time) consumes hyperforge's `PluginSchema` via the Plexus RPC introspection path. HF-IR-S01 verified — or filed a synapse follow-up ticket — that the multi-level nested tree (workspace → repo → package, 3 levels) renders correctly. If a synapse follow-up was filed, it must land before this ticket can PASS; that dependency is added to `blocked_by` when the follow-up ticket is written.

IR-6 shipped the marker + info-view rendering. IR-15 shipped the stderr invocation notice. Both consume `MethodSchema.deprecation` and `PluginSchema.deprecation`. HF-IR-9 populated those fields on hyperforge's schema. This ticket runs the end-to-end verification; no code changes expected on the hyperforge side unless verification surfaces a bug.

## Required behavior

Running `synapse hyperforge` against a live hyperforge server (at the version from HF-IR-9) produces the behaviors in the tables below.

**Tree rendering:**

| Invocation | Expected output contains |
|---|---|
| `synapse hyperforge` | A child entry named `workspace` (per HF-IR-3). Deprecated flat methods (e.g., `list_repos` if it lives on `HyperforgeHub`) prefixed with `⚠` or `[DEPRECATED]`. Non-deprecated methods render plain. |
| `synapse hyperforge workspace <ws>` | Child entries including `repo`. Deprecated `list_repos`, `get_repo`, etc. prefixed with the marker. |
| `synapse hyperforge workspace <ws> repo <r>` | Child entries including `package` and `artifact`. Deprecated flat methods prefixed. |
| `synapse hyperforge workspace <ws> repo <r> package <p>` | Methods extracted into `PackageActivation` (e.g., `build`, `test`, `publish`) — none deprecated. |
| `synapse auth` (or `synapse secrets`) | Child entry named `credential`. Deprecated flat credential methods prefixed. |
| `synapse hyperforge releases` | Child entry `release`. Deprecated `list_releases`, `get_release`, etc. prefixed. |
| `synapse hyperforge images` | Child entry `image`. Deprecated `list_images`, `get_image`, etc. prefixed. |

**Info view:**

| Invocation | Expected output contains |
|---|---|
| `synapse info hyperforge/workspace/<ws>/list_repos` (or equivalent synapse info path syntax for a deprecated method) | A `Deprecation:` section with `since: 4.2.0`, `removed_in: 5.0.0`, and the migration message. |
| `synapse info hyperforge/workspace/<ws>/repo` (the child gate itself) | No `Deprecation:` section. Non-deprecated. |

**Invocation:**

| Invocation | Expected behavior |
|---|---|
| `synapse hyperforge workspace <ws> list_repos` | Response payload to stdout (byte-identical to `synapse hyperforge workspace <ws> repo_names` via the child gate). Stderr contains: marker, substring `DEPRECATED`, method name `list_repos`, `since 4.2.0`, `removed in 5.0.0`, and the migration message pointing at `workspace.repo`. |
| `synapse hyperforge workspace <ws> repo_names` (the child gate's list stream) | Same stdout payload. Zero deprecation bytes on stderr. |
| `synapse hyperforge workspace <ws> repo <r> build pkg=<p>` | Response payload to stdout equivalent to pre-ticket `synapse hyperforge workspace <ws> build repo=<r> pkg=<p>`. Zero deprecation bytes (the nested path is not deprecated). |

## Risks

| Risk | Mitigation |
|---|---|
| Synapse 3.12.0 doesn't render 3-level nested trees. | Expected to have been caught by HF-IR-S01. If S01 filed a synapse follow-up, that ticket is on this ticket's `blocked_by` at the time of S01's completion. Update `blocked_by` here if needed. |
| Deprecation markers don't render because `plexus-protocol` schema-type version in hyperforge pins predates `DeprecationInfo`. | Hyperforge updated its `plexus-protocol` / `plexus-core` pin as part of HF-IR-4 / HF-IR-9 so the schema type carries `deprecation`. If the pin is stale, this ticket catches it and bumps the pin before PASS. |
| End-to-end invocation flow differs from local `plugin_schema()` asserts — wire serialization may drop `deprecation` if `#[serde(default)]` is wrong. | Verify `PluginSchema` round-trips via a serialization fixture in hyperforge's test suite. If the field drops, fix the serde attribute in plexus-protocol upstream and bump. |
| Synapse version available in the test environment differs from what IR-6 / IR-15 were verified against. | Pin synapse version at the top of the verification transcript; if the pinned version predates IR-6 / IR-15, bump locally before running. |

## What must NOT change

- The Rust-side schema contents landed in HF-IR-9 — this ticket is verification only, no schema edits.
- Child-gate registration from HF-IR-3..8 — this ticket exercises them, doesn't rewire.
- Hyperforge version — stays `4.2.0`.
- Synapse itself — unless a defect is surfaced and a synapse fix ticket is filed. Fix ticket work does not happen inside this ticket.

## Acceptance criteria

1. `cargo build --workspace` in hyperforge passes.
2. `cargo test --workspace` in hyperforge passes.
3. `cargo build` green in every sibling repo that depends on hyperforge.
4. A transcript (saved into the commit as a fixture or captured in the PR description) demonstrates: `synapse hyperforge workspace <ws>` tree output shows `repo` as a child entry and shows deprecated flat methods (e.g., `list_repos`) prefixed with the marker.
5. A transcript demonstrates: `synapse info` on a deprecated method shows the `Deprecation:` section with `since: 4.2.0`, `removed_in: 5.0.0`, and the migration message.
6. A transcript demonstrates: invoking a deprecated method writes the stderr notice per IR-15 while producing a byte-identical stdout payload to the equivalent child-gate invocation. Captured stdout and stderr are both included in the transcript.
7. A transcript demonstrates: synapse renders the 3-level nested path `workspace.repo.package` correctly (tree view at each depth).
8. A regression check: running synapse against a pre-HF-IR fixture schema (archived schema from before HF-IR-9) renders no deprecation markers — the behavior gate from HF-IR is purely additive.
9. If HF-IR-S01 filed a synapse follow-up ticket for multi-level nesting, that ticket is Complete before this ticket PASSes. This ticket's `blocked_by` is updated accordingly at S01's close.
10. Hyperforge version remains `4.2.0`. Local tag `hyperforge-v4.2.0` already exists from HF-IR-9; this ticket does not re-tag.

## Completion

Commit lands the transcripts + any test fixture used for the regression check. `cargo build --workspace` + `cargo test --workspace` green. Status flipped to Complete in the same commit. HF-IR sub-epic is now Complete — update HF-IR-1's status to Complete in the same commit (or a follow-up commit bundled with this one).
