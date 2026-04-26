---
id: HF-AUDIT-2
title: "plexus-locus: migrate 0.3 → 0.5 plexus pins + call-site deltas (Activation::call, ChildRouter auto-impl)"
status: Pending
type: implementation
blocked_by: []
unlocks: []
severity: High
target_repo: plexus-locus
---

## Problem

`plexus-locus` fails to compile (63 errors). Two layered causes:

1. **Stale version pins** (HF-0 sibling-drift pattern):
   ```toml
   plexus-macros    = "0.3"   # current workspace: 0.5.1
   plexus-core      = "0.3"   # current workspace: 0.5.0
   plexus-transport = "0.1"   # current workspace: 0.2.0
   ```
   The workspace-level patch at `/Users/shmendez/dev/controlflow/hypermemetic/.cargo/config.toml` already points `[patch.crates-io]` at the local 0.5.x paths, but the patch is discarded because `^0.3` doesn't match `0.5.x`. Cargo emits:
   ```
   warning: patch `plexus-core v0.5.0 (...)` was not used in the crate graph
   warning: patch `plexus-macros v0.5.1 (...)` was not used in the crate graph
   warning: patch `plexus-transport v0.2.0 (...)` was not used in the crate graph
   ```
   Result: plexus-locus compiles against crates.io 0.3.x where `plexus_macros::activation` / `plexus_macros::method` don't exist yet (old names were `hub_methods` / `hub_method`). Manifests as 63 `E0433 could not find 'activation' in 'plexus_macros'` errors.

2. **Real 0.4 → 0.5 call-site migration work** (surfaced only after the pin bump):
   - `Activation::call` trait signature changed. It now takes 5 args: `(self, method, params, Option<&AuthContext>, Option<&RawRequestContext>)`. plexus-locus's 8 activations still call it with the old 3-arg form:
     ```
     error[E0061]: this function takes 5 arguments but 3 arguments were supplied
        --> src/activations/sessions.rs:98, tabs.rs:162, workspace.rs:529, ... (8 sites)
     ```
   - `ChildRouter` is now auto-emitted by `#[plexus_macros::activation]`, conflicting with plexus-locus's 8 hand-written impls:
     ```
     error[E0119]: conflicting implementations of trait 'plexus_core::ChildRouter' for type 'InfoActivation'
     ```
     Sites: `info.rs:62`, `observation.rs:260`, `panes.rs:1171`, `recording.rs:404`, `render.rs:386`, `sessions.rs:88`, `tabs.rs:152`, `workspace.rs:519`.

So plexus-locus is one layer deeper than mono-provider / plexus-music-royalty-free / plexus-mono in HF-AUDIT-1 — the pin bump exposes actual 0.5 migration work, not just a mechanical version change. Equivalent to hyperforge's HF-0 scope (which also had to apply 0.4 → 0.5 call-site adjustments).

## Context

- Root path: `/Users/shmendez/dev/controlflow/hypermemetic/plexus-locus/`
- Current version: `0.1.0`.
- Workspace patch already in place at `/Users/shmendez/dev/controlflow/hypermemetic/.cargo/config.toml` pointing at sibling paths for plexus-core, plexus-macros, plexus-transport. No local `.cargo/config.toml` patch needed — just the manifest pins.
- plexus-locus's own `.cargo/config.toml` is *committed* and sets `rustflags = ["-D", "dead-code", ...]`. This applies to patched-dep builds too, and plexus-core 0.5.0 currently has a `dead_code` warning (unused `plexus_error_to_jsonrpc` fn) that trips it. Either (a) the plexus-core warning gets fixed upstream, (b) plexus-locus narrows its rustflags scope to non-patched deps, or (c) plexus-core's dead code gets removed. Simplest path: delete the unused fn in plexus-core if truly unused (cross-repo edit), else scope the rustflag via `target.'cfg(...)'.rustflags` pattern. **Note: this may be separately resolved by HF-LINT-SWEEP** in substrate, since that epic targets workspace-wide zero-warnings.
- Recent plexus-locus commits show the author was mid-migration:
  ```
  de8afa1 chore: remove [patch.crates-io] — use pinned crates.io versions
  a9c3492 chore: migrate hub_methods→activation, hub_method→method
  ```
  Macro renames landed (`a9c3492`), and `[patch.crates-io]` was removed (`de8afa1`), but the manifest pins were left at `0.3` — so the local workspace patch is silently dropped. Previous contributor assumed crates.io would have 0.5 published; it hasn't.

## Required behavior

1. **Bump manifest pins** in `/Users/shmendez/dev/controlflow/hypermemetic/plexus-locus/Cargo.toml`:
   ```toml
   plexus-macros    = "0.5"
   plexus-core      = "0.5"
   plexus-transport = "0.2"
   ```
2. **Run `cargo build`** to surface the real migration delta (should expose the 16 E0119/E0061 errors listed above once pins are aligned).

3. **Resolve `Activation::call` signature change (8 sites)**. The new signature is:
   ```rust
   async fn call(
       &self,
       method: &str,
       params: serde_json::Value,
       auth: Option<&AuthContext>,
       ctx: Option<&RawRequestContext>,
   ) -> Result<Value, PlexusError>;
   ```
   Each site in plexus-locus currently reads:
   ```rust
   Activation::call(self, method, params).await
   ```
   Update to pass `None, None` (no auth/ctx propagation in these bridge call paths) unless the surrounding function has an auth/ctx in scope to forward.
   Sites: `sessions.rs:98`, `tabs.rs:162`, `workspace.rs:529`, `info.rs:?`, `observation.rs:?`, `panes.rs:?`, `recording.rs:?`, `render.rs:?` (8 total — survey per-file for exact line).

4. **Resolve `ChildRouter` duplicate impl (8 sites)**. The `#[plexus_macros::activation]` macro now auto-emits a `ChildRouter` impl. plexus-locus's 8 hand-written impls conflict. Options:
   - If the manual impl matches what the macro emits: delete the manual impl.
   - If the manual impl has custom routing logic: examine whether the macro attrs (`#[child]`, `MethodRole::DynamicChild`, etc.) can express the same — migrate the custom logic into macro attrs and delete the hand impl.
   - If macro can't express it: keep the hand impl, suppress the macro's auto-emission (if there's a flag) OR report back as a blocker for further plexus-macros work.
   Sites: `info.rs:62`, `observation.rs:260`, `panes.rs:1171`, `recording.rs:404`, `render.rs:386`, `sessions.rs:88`, `tabs.rs:152`, `workspace.rs:519`.

5. **Handle deprecation warnings** from 0.5. Wrap any 0.5-deprecated symbols plexus-locus consumes (e.g., `ChildCapabilities`) in `#[allow(deprecated)]` with a `// TODO(HF-IR): migrate to MethodRole::DynamicChild` marker. Do NOT migrate to the replacement in this ticket — that's HF-IR's scope.

6. **Resolve rustflags-vs-patched-dep friction**. plexus-locus's `.cargo/config.toml` denies `dead-code` globally, which trips on plexus-core 0.5.0's unused `plexus_error_to_jsonrpc`. Options (pick minimum-diff):
   - (a) Verify HF-LINT-SWEEP has cleaned the plexus-core dead-code warning. If yes, rebuild and continue.
   - (b) Remove `"-D", "dead-code"` from plexus-locus's `.cargo/config.toml`. Low-cost, low-value-lost; plexus-locus's own dead code will still be caught by `cargo clippy` in CI.
   - (c) Use `target.'cfg(all())'.rustflags` + cargo's unstable features to scope rustflags — more complex, defer.
   Prefer (a) if possible, (b) if not. Document the choice in the commit body.

7. **Version bump** plexus-locus: `0.1.0` → `0.1.1` (patch — this is a compat/migration fix, no public-surface change plexus-locus adds itself). Per `feedback_version_bumps_as_you_go.md`.

8. **Tag locally**: annotated tag `plexus-locus-v0.1.1`. plexus-locus currently has no tags; establish the convention. Do NOT push.

## Risks

| Risk | Mitigation |
|---|---|
| Hand-written `ChildRouter` impls encode routing behavior the macro can't replicate. | Per-site audit: read each manual impl. If custom (dynamic child dispatch, filtering, side effects), don't naively delete. Migrate to `#[child]`/`MethodRole::DynamicChild` if possible, else surface as a plexus-macros deficiency ticket and mark the activation as-is with `#[allow(...)]` or a compiler hint. |
| The `Activation::call` bridge pattern in plexus-locus activations is *internal* forwarding (e.g., activation N routes into activation M via `Activation::call`). If so, `None, None` for auth/ctx *drops* upstream auth on the floor — silent auth escalation / bypass bug. | Read each call site's caller chain. If upstream has `auth: Option<&AuthContext>` / `ctx: Option<&RawRequestContext>` available, thread them through. If not, add a TODO-marked `None, None` with a note that the calling fn needs auth plumbing. |
| plexus-core dead-code fix (HF-LINT-SWEEP) and plexus-locus build depend on each other. | Either coordinate (do HF-LINT-SWEEP first) or apply workaround (b) to decouple. |
| Untested activations (recording, render) may have subtle behavior changes post-migration. | Run `cargo test` per activation. Enumerate any test gaps in the commit body (no new tests in scope). |
| Bumping plexus-transport 0.1 → 0.2 exposes additional API deltas (TransportServer builder, etc.). | Compare `src/main.rs:235` (`TransportServer::builder(hub, rpc_converter)`) against substrate's `src/main.rs:119`. They should match; if not, migrate. |

## What must NOT change

- plexus-locus's public CLI behavior (args, stdio/websocket modes, `--template` / `--params`, backend selection).
- Activation namespaces (`sessions`, `tabs`, `panes`, `workspace`, `info`, `recording`, `render`, `observation`).
- Method names or schemas (i.e., no breaking wire changes to existing RPC surface).
- Any non-plexus dep versions.
- Rhai scripting engine behavior (separate subsystem; untouched).

## Acceptance criteria

1. `cargo build` at `/Users/shmendez/dev/controlflow/hypermemetic/plexus-locus/` exits 0. Full output captured in commit body.
2. `cargo test` at same path exits 0, or pre-existing failures are enumerated with `git stash` demonstration of identical pre-fix failure.
3. `cargo tree -d` shows a single version of each plexus-* crate.
4. Integration gate: verify the sibling workspace (`plexus-substrate`, `plexus-core`, `plexus-macros`, `plexus-transport`) still builds after this ticket lands. No regression.
5. Commit body includes:
   - Root-cause summary (from this ticket's Problem section, one paragraph).
   - Exact Cargo.toml version bumps.
   - Per-site summary of `Activation::call` adjustments (8 sites) with auth/ctx propagation decisions documented.
   - Per-site summary of `ChildRouter` resolution (8 sites): deleted manual impl vs migrated to macro attrs vs kept as-is.
   - Each `#[allow(deprecated)]` added, with `// TODO(HF-IR)` marker.
6. plexus-locus version bumped `0.1.0` → `0.1.1` in Cargo.toml. Annotated tag `plexus-locus-v0.1.1` created locally. Not pushed.
7. `.cargo/config.toml` rustflags resolution documented (option a/b/c per Step 6 above).

## Full error output (pre-fix, post-pin-bump)

After bumping pins to 0.5/0.5/0.2 with `RUSTFLAGS=""` (to bypass the `.cargo/config.toml` dead-code denial that trips on plexus-core itself):

```
error[E0119]: conflicting implementations of trait `plexus_core::ChildRouter` for type `InfoActivation`
  --> src/activations/info.rs:62:1
error[E0119]: conflicting implementations of trait `plexus_core::ChildRouter` for type `ObservationActivation`
  --> src/activations/observation.rs:260:1
error[E0119]: conflicting implementations of trait `plexus_core::ChildRouter` for type `PanesActivation`
  --> src/activations/panes.rs:1171:1
error[E0119]: conflicting implementations of trait `plexus_core::ChildRouter` for type `RecordingActivation`
  --> src/activations/recording.rs:404:1
error[E0119]: conflicting implementations of trait `plexus_core::ChildRouter` for type `RenderActivation`
  --> src/activations/render.rs:386:1
error[E0119]: conflicting implementations of trait `plexus_core::ChildRouter` for type `SessionsActivation`
  --> src/activations/sessions.rs:88:1
error[E0119]: conflicting implementations of trait `plexus_core::ChildRouter` for type `TabsActivation`
  --> src/activations/tabs.rs:152:1
error[E0119]: conflicting implementations of trait `plexus_core::ChildRouter` for type `WorkspaceActivation`
  --> src/activations/workspace.rs:519:1

error[E0061]: this function takes 5 arguments but 3 arguments were supplied (×8 sites)
   --> src/activations/sessions.rs:98:9     Activation::call(self, method, params).await
   --> src/activations/tabs.rs:162:9        Activation::call(self, method, params).await
   --> src/activations/workspace.rs:529:9   Activation::call(self, method, params).await
   --> (... 5 more)
   missing args: Option<&AuthContext>, Option<&RawRequestContext>

error: could not compile `plexus-locus` (lib) due to 16 previous errors
```

Pre-pin-bump (original 63-error state):

```
error[E0433]: failed to resolve: could not find `activation` in `plexus_macros`
  --> src/activations/info.rs:27:18 (×N activations)
error[E0433]: failed to resolve: could not find `method` in `plexus_macros`
  --> src/activations/info.rs:33:22 (×N methods)
error[E0277]: the trait bound `SessionsActivation: plexus_core::Activation` is not satisfied
  --> src/activations/sessions.rs:98:26
   (cascades — once macros resolve against 0.5, trait impl appears; downstream E0277 vanishes)

error: could not compile `plexus-locus` (lib) due to 63 previous errors
```

Both states reproduced. The second (16-error) state is the true migration scope.

## Proposed execution order

1. Bump manifest pins (trivial).
2. Run `cargo build` to surface 16 errors (same as captured above).
3. For each activation (pick one first, establish pattern, then replicate):
   - Resolve `ChildRouter` conflict (delete vs migrate).
   - Resolve `Activation::call` signature.
   - Verify build after that single activation's fixes.
4. Run `cargo test`. Enumerate deltas.
5. Version bump + commit + tag.
6. Resolve `.cargo/config.toml` rustflags issue (per Step 6 in Required behavior).

## Completion

Single commit in plexus-locus per step 3–5 (may split by activation if atomic-per-activation commits preferred). Tracker ticket is HF-AUDIT-2 in substrate's plans/; flip to Complete when plexus-locus passes the integration gate.
