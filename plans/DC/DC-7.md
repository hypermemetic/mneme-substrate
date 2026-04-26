---
id: DC-7
title: "Lint / rustdoc hygiene check prevents boundary-violation regressions"
status: Pending
type: implementation
blocked_by: [DC-2, DC-3, DC-4, DC-5, DC-6]
unlocks: []
severity: Medium
target_repo: plexus-substrate
---

## Problem

Without a mechanical check, the library-API boundaries DC-2 pinned and DC-3..DC-6 migrated will drift over time. New code will reach into sibling internals because Rust — in the single-crate case — does not enforce per-activation boundaries via the type system. Even in the workspace-split case (if S01 chose that), new `pub` items added to a crate can become accidental library-API surfaces without review. A CI-enforced rule is the difference between "DC's decoupling held" and "DC's decoupling eroded in six months".

## Problem (workspace-split case, if S01 chose B)

If DC-S01 picks the workspace-split option, the compiler itself enforces most of the boundary — sibling activations can't import `pub(crate)` items across crates. DC-7 in this case is narrower: a rustdoc / CI check that every new `pub` item in an activation's crate root is either (a) explicitly added to the library API with a doc comment, or (b) flagged for review. Prevents accidental surface growth.

## Problem (convention-only case, if S01 chose A)

If DC-S01 picks convention-only, the compiler does not enforce boundaries (every `pub(crate)` item is visible to every module in substrate). DC-7 is the mechanical check that re-creates the enforcement: a CI script, custom clippy lint, or rustdoc rule that fails the build when a sibling imports a non-library-API item.

## Context

**Two viable enforcement mechanisms, picked based on DC-S01's outcome:**

**Mechanism 1 — Grep-based CI script (convention-only case).**

A small shell or Python script in `scripts/` (or `xtask/`) that:
1. Enumerates activation directories under `src/activations/`.
2. For each activation's `mod.rs`, parses the `pub use` re-export block to learn the library-API set.
3. For every other activation's code, greps for `use crate::activations::<other>::` imports.
4. Fails if any import references a symbol not in the target activation's library-API set.

Pros: simple, no external crates, easy to audit. Cons: grep-based, brittle to syntactic variation (`pub use` vs `pub use self::`, re-exports from sub-modules, etc.).

**Mechanism 2 — Custom clippy lint / `deny(exported_private_dependencies)`-style approach.**

Use Rust's module visibility combined with a custom lint attribute or a clippy extension that fails on out-of-activation reach-ins. Mark each activation's internals with a sentinel attribute (`#[doc(hidden)]`, or a documented module-level comment clippy can look for).

Pros: integrates with compile. Cons: custom clippy lints require a plugin (which is nightly-only historically) or a third-party tool like `cargo-deny` configured carefully.

**Mechanism 3 — rustdoc check (workspace-split case).**

`cargo doc` generates per-crate docs. A rustdoc post-processing script checks that every `pub` item in each activation crate appears in a doc-comment "Library API" section. Items missing from that section fail the build. This prevents accidental public-surface growth.

**Recommendation:** DC-S01's outcome decides. The implementor picks the mechanism most aligned with the chosen path.

**Pin the chosen mechanism in DC-7's commit** — do not build all three. DC-7 ships **one** check.

## Required behavior

| Event | Expected outcome |
|---|---|
| A PR adds `use crate::activations::bash::Bash` in `src/activations/cone/` where `Bash` is not in Bash's library-API set | CI fails with a message identifying the violating import and naming the expected library-API alternative |
| A PR adds `use crate::activations::arbor::NodeType` in `src/activations/claudecode/` | CI fails — `NodeType` is no longer in Arbor's library-API set post-DC-6 |
| A PR adds a legitimate import from `crate::activations::<other>::` where the imported symbol IS in the target's library-API set | CI passes |
| A PR adds a new `pub use` item to an activation's `mod.rs` without also updating the library-API doc comment / manifest | CI fails (workspace-split / rustdoc mechanism) OR CI passes with an advisory warning (convention-only mechanism) — implementor picks |
| A PR modifies code within one activation without touching cross-activation imports | CI passes |

**Activation library-API set declaration.** Each activation's library-API set is declared in one of two places (consistent across all activations):

- Option A (convention-only): a comment block in `mod.rs` delimiting the library API, e.g. a `// BEGIN LIBRARY API` / `// END LIBRARY API` sentinel around the `pub use` block. The lint parses this region.
- Option B (workspace-split): each crate's `lib.rs` uses `#[doc = "..."]` or a module-level attribute to declare library-API items.

## Risks

- **The check becomes noisy.** False positives make CI un-trusted. If the grep-based approach generates noise (e.g., imports inside test modules, imports inside `mod.rs` re-exports), tune until noise is zero against the HEAD codebase before turning on strict mode. Acceptance criteria 3 pins this: a clean HEAD must produce zero violations.
- **The check becomes decorative.** If the check is easy to bypass (e.g., `#[allow(...)]`, inline comments), people bypass it. Make the check hard to bypass without human review — e.g., any bypass annotation requires a comment explaining why, and a periodic audit catches bypasses.
- **DC-S01 reversed later.** If convention-only ships, drift is detected, and the project later decides to switch to workspace-split, DC-7's mechanism must be replaceable. **Mitigation:** document DC-7's mechanism clearly; note in README that swapping to workspace-split supersedes DC-7's implementation.

## What must NOT change

- Any activation's wire-level behavior.
- Any already-passing `cargo test`.
- The library-API surfaces defined in DC-2 — DC-7 encodes and enforces them, doesn't redefine them.
- `cargo build` must continue to succeed — DC-7 is a CI check that runs alongside or after build, not a change that breaks normal compilation.

## Acceptance criteria

1. A CI check is wired into the project's CI pipeline (GitHub Actions workflow, CircleCI config, or whichever runner the repo uses) that fails when a sibling activation imports a non-library-API item.
2. Running the check on `main` (post-DC-6) reports zero violations.
3. A synthetic regression PR that re-introduces `use crate::activations::claudecode_loopback::LoopbackStorage` in `src/activations/orcha/` causes the check to fail with a clear error message naming the violation.
4. The README or a dedicated `docs/architecture/` doc describes the enforcement mechanism, how to update the library-API declarations when adding a new re-export, and how to request a bypass if one is genuinely needed.
5. A second synthetic regression PR — `use crate::activations::arbor::NodeType` added to `src/activations/orcha/` — also causes the check to fail.
6. `cargo test --workspace` continues to pass.
7. The CI check's runtime is under 30 seconds against the full substrate tree — if the grep / parse approach is slower, optimize or switch mechanism.

## Completion

Implementor delivers:

- Commit introducing the CI check (script, clippy config, rustdoc post-processor, or equivalent — picked by DC-S01's outcome).
- Commit wiring the check into CI.
- Commit adding a README section or architecture doc describing the mechanism and how to maintain it.
- CI run output showing green against `main`.
- Two synthetic-regression-PR CI run outputs showing the expected failures (attached to the DC-7 commit or the closing epic commit).
- Status flip to `Complete` in the commit that lands the work. DC-1 epic can then be flipped to `Complete` by the epic owner.
