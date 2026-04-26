---
id: HF-CLEAN
title: "hyperforge: zero deprecations, zero warnings, then stricter lints"
status: Complete
type: implementation
blocked_by: [HF-0]
unlocks: []
severity: High
target_repo: hyperforge
---

## Problem

Post-HF-0 hyperforge builds, but the tree still emits:

1. **Two user-code `#[allow(deprecated)]` suppressions** added by HF-0 at `src/hub.rs:476` and `src/hubs/repo.rs:105`, each marked `// TODO(HF-IR): remove the deprecated 'hub' argument; hub mode is inferred from #[child] gates in plexus-macros 0.6.` These suppress the `hub = true` deprecation on `#[plexus_macros::activation]`, not fix it.
2. **Macro-emitted deprecation warnings** that `#[allow(deprecated)]` on the impl block can't reach (`ChildCapabilities`, `_PLEXUS_MACROS_DEPRECATED_HUB_FLAG_*`) — propagating out of macro expansion into hyperforge's build surface.
3. **Any other compile-time warnings** not related to deprecations (unused imports, dead code, non-snake-case, etc.) that accumulated through the pre-HF-0 partial migration and aren't being enforced today.
4. **A lax lint posture:** hyperforge has no workspace-level `[lints]` table; there's no `#![deny(warnings)]` anywhere; clippy isn't configured beyond rustc defaults. There's no CI gate that fails on new warnings.

HF-CLEAN establishes a zero-warnings baseline: remove deprecations by migrating off them (not suppressing), clear every residual warning, then tighten the lint posture and clean the additional noise that surfaces.

## Context

HF-CLEAN sits between HF-0 (already Complete — unbreak) and HF-DC (pending — librification). It does NOT do HF-IR's full dynamic-child-gate migration. It only does the narrow piece needed to drop `hub = true` usage on `HyperforgeHub` and `RepoHub`.

**How to drop `hub = true`** (per IR-17 Orcha migration precedent):

```rust
// BEFORE:
#[plexus_macros::activation(namespace = "hyperforge", version = "4.1", hub)]
impl HyperforgeHub { ... }

// AFTER (static children pattern, per IR-17 Orcha):
#[plexus_macros::activation(namespace = "hyperforge", version = "4.2")]
impl HyperforgeHub {
    #[plexus_macros::child]
    fn workspace(&self) -> WorkspaceHub { self.state.workspace.clone() }

    #[plexus_macros::child]
    fn repo(&self) -> RepoHub { self.state.repo.clone() }

    #[plexus_macros::child]
    fn build(&self) -> BuildHub { self.state.build.clone() }

    #[plexus_macros::child]
    fn images(&self) -> ImagesHub { self.state.images.clone() }

    #[plexus_macros::child]
    fn releases(&self) -> ReleasesHub { self.state.releases.clone() }

    // existing methods unchanged
}
```

Same pattern for `RepoHub` for its own static children (if any).

If a hub state field is wrapped in `Arc<T>`, the child accessor clones the `Arc`, which is cheap (`(*self.state.workspace).clone()` returns `WorkspaceHub` by cloning the inner). If `T` itself is `Clone` but not `Arc<T>`, plain `.clone()` works. Either way, the child accessor returns the child activation by value.

This migration is pre-HF-IR work in the sense that HF-IR does a broader reshape (dynamic gates for repos / packages / artifacts / credentials, child activation structs, deprecation wiring on flat methods). HF-CLEAN does only the static child accessors that let us drop `hub = true`. HF-IR's scope is unchanged.

Per HF-IR-1's version contract, HF-IR-2 bumps hyperforge 4.1.x → 4.2.0. HF-CLEAN landing first means hyperforge goes to 4.1.2 (patch: bug-fix / warning cleanup, no new public API). HF-IR-2 still bumps to 4.2.0 when it lands.

## Required behavior

1. **Drop `hub = true`** on `HyperforgeHub` and `RepoHub` (and any other activation currently using it — grep `hub = true` or `hub,` or `, hub)` across `src/`). Migrate each to `#[plexus_macros::child]` accessor fns for every static child currently stored in state.
2. **Remove** both `#[allow(deprecated)]` + `TODO(HF-IR)` markers added by HF-0. They are no longer needed after step 1.
3. **Eliminate every other compile warning.** Build with `cargo build --all-targets 2>&1 | grep warning:` returning zero lines. Target categories:
   - Unused imports → remove.
   - Dead code (`dead_code` warnings on fns / methods / fields) → remove if truly unused; add `#[allow(dead_code)]` with an explanatory comment ONLY if the item is part of a partially-built feature that must stay.
   - Non-snake-case or non-camel-case identifiers → rename.
   - Unused `mut`, unused variables (`let _ =`), unused parens → fix.
   - `unused_must_use` → use `.unwrap()`, `?`, `let _ =` with intent.
   - Deprecation warnings (other than the two from step 1) → resolve at the call-site per the deprecation's replacement, or narrowly `#[allow(deprecated)]` with a specific `// TODO(<ticket>):` marker pointing at a new follow-up ticket filed Pending per the `feedback_write_cleanup_tickets_immediately.md` memory.
4. **Add a workspace-level `[lints]` table** to `Cargo.toml`:
   ```toml
   [lints.rust]
   warnings = "deny"
   unsafe_code = "forbid"
   unused_crate_dependencies = "warn"
   let_underscore_drop = "warn"
   unreachable_pub = "warn"
   missing_debug_implementations = "warn"

   [lints.clippy]
   pedantic = { level = "warn", priority = -1 }
   nursery = { level = "warn", priority = -1 }
   # Opt-outs for clippy lints that produce too much noise for too little value:
   module_name_repetitions = "allow"
   missing_errors_doc = "allow"
   missing_panics_doc = "allow"
   must_use_candidate = "allow"
   too_many_lines = "allow"
   # Add more opt-outs as discovered; each deserves a one-line justification in a comment.
   ```
5. **Clear the new lint cascade.** After adding the lints above, the first `cargo build --all-targets` + `cargo clippy --all-targets` run will likely produce hundreds of new warnings. Clean systematically:
   - `unreachable_pub` → narrow `pub` to `pub(crate)` where external visibility isn't needed.
   - `missing_debug_implementations` → add `#[derive(Debug)]` where feasible; add targeted `#[allow(missing_debug_implementations)]` where the type wraps a non-Debug foreign type.
   - `clippy::pedantic` / `clippy::nursery` noise → fix where cheap, narrowly suppress with justification where expensive.
   - Iterate until zero warnings remain.
6. **Wire the gate into CI** (if hyperforge has CI config in-repo): add a step that runs `cargo build --all-targets -- -D warnings` and `cargo clippy --all-targets -- -D warnings`. If CI lives outside the repo, note this in the commit body as a TODO for the user.
7. **Version bump + tag.** Hyperforge bumps `4.1.1 → 4.1.2` (patch: bug/warning cleanup, no API change). Local annotated tag `hyperforge-v4.1.2`, not pushed.

## Risks

| Risk | Mitigation |
|---|---|
| Dropping `hub = true` changes the activation's wire surface enough to break synapse tree rendering. | Per IR-17 Orcha migration, the `#[plexus_macros::child]` pattern produces the same wire-level child listing as `hub = true` did — synapse sees the same children at the same paths. Verify by running `synapse hyperforge` pre/post and diffing the tree. |
| `clippy::pedantic` or `clippy::nursery` produces genuinely useful signal but also signal-to-noise issues that each need a per-case judgement call. | Expected. The commit body lists every `#[allow]` added with justification. If the noise is overwhelming on a specific lint, opt it out at the workspace level with a comment explaining why — don't scatter per-site allows. |
| A dead-code warning turns out to be for genuinely-used-but-obscure code (e.g., a `#[test]` helper only compiled under a feature flag). | Don't delete blindly. If the item has a semantic use that the compiler can't see (e.g., feature-gated), `#[cfg_attr(not(feature = "X"), allow(dead_code))]` or similar. |
| A new lint surfaces a real bug. | Fix the bug in this commit OR file a Pending ticket immediately per `feedback_write_cleanup_tickets_immediately.md`; do NOT suppress the lint without acknowledging the underlying concern. |
| Dropping `hub = true` reveals that the macro's post-0.4 behavior for multi-child impls has a latent issue (generic parameter inference, trait bounds, etc.). | Per IR-17 / IR-20 / IR-21 — the macro has been through multiple fix passes for generic activation support. If a new snag surfaces, file a plexus-macros ticket and include `#[allow(deprecated)]` with the TODO pointing at it as a temporary measure. The ticket body documents the regression. |

## What must NOT change

- Hyperforge's public CLI behavior (arg grammar, exit codes, output format).
- Activation namespaces (`hyperforge`, `auth`/`secrets` — preserve).
- The 74 existing method signatures beyond what `hub = true` removal implies.
- Any test's behavior — if a test passes pre-HF-CLEAN it passes post.
- `Cargo.lock` beyond natural resolution (no hand edits).

## Acceptance criteria

1. `cargo build --all-targets` at hyperforge root exits 0 with **zero warnings** after the full cleanup (including post-lint-table cascade).
2. `cargo clippy --all-targets` exits 0 with **zero warnings**.
3. `cargo test` exits 0.
4. `cargo build -p plexus-substrate` at substrate root still green (regression check).
5. `grep -rn 'hub = true\|hub,\|, hub)' hyperforge/src/` returns zero results.
6. `grep -rn '#\[allow(deprecated)\]' hyperforge/src/` returns zero results — OR each remaining site has a filed Pending follow-up ticket referenced in its `// TODO(<ticket-id>):` marker, enumerated in the commit body.
7. `Cargo.toml` contains the `[lints.rust]` and `[lints.clippy]` tables per step 4. Every `"allow"` entry has an adjacent comment justifying why.
8. synapse tree render for hyperforge produces the same child structure pre/post HF-CLEAN (manual diff noted in commit body).
9. hyperforge version `4.1.1 → 4.1.2` in `Cargo.toml`; annotated local tag `hyperforge-v4.1.2` created, not pushed.
10. All new Pending follow-up tickets filed (if any) are listed in the commit body with their IDs.

## Completion

PR against hyperforge. Commit body includes:
- The pre/post synapse tree diff for hyperforge (should be empty).
- The full list of `[lints.rust]` and `[lints.clippy]` settings adopted.
- Every `"allow"` justification.
- Every remaining `#[allow(*)]` in user code with its follow-up ticket reference.
- The full list of follow-up Pending tickets filed.
- Integration gate output summary (build + clippy + test green).

HF-CLEAN flipped to `Complete` in the same commit.
