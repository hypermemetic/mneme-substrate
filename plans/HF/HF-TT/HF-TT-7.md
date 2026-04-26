---
id: HF-TT-7
title: "Introduce Ecosystem enum + BuildSystemKind::ecosystem() accessor"
status: Pending
type: implementation
blocked_by: [HF-TT-2]
unlocks: [HF-TT-8]
target_repo: hyperforge
severity: Medium
---

## Problem

Hyperforge's `BuildSystemKind` enum encodes the build tool (Cargo, Cabal, Npm, Pnpm, Poetry, …) but not the overarching ecosystem (Rust, Haskell, JavaScript, Python, …). HF-CTX's fact taxonomy and HF-TT-4's `ArtifactId` both want a qualified reference of the form `<ecosystem>:<namespace>:<name>`, which requires a first-class `Ecosystem` type. Today, consumers who need to group build tools by ecosystem (e.g., "is this a JS ecosystem build?") write ad-hoc match arms inline and can silently miss a variant. This ticket introduces the `Ecosystem` enum as a first-class type and adds a `fn ecosystem() -> Ecosystem` accessor on `BuildSystemKind` so every build tool maps to its ecosystem once, in one place.

## Context

HF-TT-2 introduced `Ecosystem` in `crates/hyperforge-types/src/newtypes/ecosystem.rs`. This ticket wires it to `BuildSystemKind`.

`BuildSystemKind` lives in `crates/hyperforge-types/src/build_system/` (or equivalent post-HF-DC). Variants observed in HF-0: `Cargo, Cabal, Node, Npm, Pnpm, Poetry, ...`. Each variant maps to exactly one ecosystem:

| BuildSystemKind | Ecosystem |
|---|---|
| `Cargo` | `Rust` |
| `Cabal` | `Haskell` |
| `Node`, `Npm`, `Pnpm`, `Yarn` (if present) | `JavaScript` |
| `Poetry`, `Pip`, `Uv` (if present) | `Python` |
| `GoMod` (if present) | `Go` |
| `Gemspec`, `Bundler` (if present) | `Ruby` |
| `Mix`, `Rebar` (if present) | `Elixir` |

The exact `BuildSystemKind` variant list and the exhaustive mapping is pinned in HF-TT-S01's report.

File-boundary discipline: this ticket edits `crates/hyperforge-types/src/build_system/` files. It does NOT touch Repo / Package / Version / Path / Credential modules. The `Ecosystem` definition file (`ecosystem.rs`) was created in HF-TT-2; this ticket adds the accessor but does not modify `Ecosystem` itself beyond possibly adding missing variants that S01 identified.

## Required behavior

| Construct | Behavior |
|---|---|
| `impl BuildSystemKind { pub fn ecosystem(&self) -> Ecosystem }` | Returns the ecosystem for each variant per the mapping table. Exhaustive match — missing a variant is a compile error. |
| `Ecosystem::all()` (if added) | Returns a slice of all variants. Optional helper; include if any call site benefits. |
| Existing ad-hoc "group by build system" sites in `hyperforge-core` | Rewritten to call `build_system_kind.ecosystem()` when the grouping is ecosystem-shaped. |

No changes to `BuildSystemKind`'s existing variants or wire format. No changes to `Ecosystem`'s derives (pinned in HF-TT-2). No edits to any non-build-system consumer — those will adopt `Ecosystem` in HF-TT-8/9 or in HF-CTX.

Round-trip test `crates/hyperforge-types/tests/ecosystem_mapping.rs` asserts:
- Every `BuildSystemKind` variant has a defined `ecosystem()` return value (covered by exhaustive match; this test is a catch-net that instantiates each variant and calls `.ecosystem()`).
- `Ecosystem` round-trips through serde as its snake_case name.
- `Ecosystem` is `#[non_exhaustive]` (compile-test: attempting an exhaustive external match produces a warning).

## Risks

| Risk | Mitigation |
|---|---|
| S01 missed a `BuildSystemKind` variant. | Exhaustive `match` in `ecosystem()` is the compile-time gate. If S01's mapping table is incomplete, this ticket surfaces it. |
| A build system spans ecosystems (e.g., a meta-tool wrapping both Cargo and Npm). | Out of scope for this ticket. If encountered, `ecosystem()` is not the right abstraction — file a follow-up ticket and block. |
| `Ecosystem::all()` helper gets stale when `#[non_exhaustive]` variants grow. | Don't ship `all()` unless a call site demonstrably needs it. If shipped, document that external callers must not rely on its length being stable. |

## What must NOT change

- `BuildSystemKind`'s existing variants — wire format byte-identical.
- `Ecosystem`'s derives or attributes (pinned in HF-TT-2; this ticket only wires the accessor).
- Public method names on any activation.
- CLI behavior or output.

## Acceptance criteria

1. `BuildSystemKind::ecosystem(&self) -> Ecosystem` exists and returns the correct `Ecosystem` for every variant.
2. The `ecosystem()` match is exhaustive (no wildcard arm).
3. Every variant in S01's pinned table is covered.
4. `Ecosystem` serde round-trip test passes.
5. Mapping test passes (every `BuildSystemKind` variant, instantiated, produces a defined `Ecosystem`).
6. `cargo build --workspace` green in hyperforge.
7. `cargo test --workspace` green in hyperforge.
8. File-boundary check: edits confined to `crates/hyperforge-types/src/build_system/` files + possible additions in `ecosystem.rs`. No Repo / Package / Version / Path / Credential cluster edits.
9. Sibling-repo audit: consumer repos still build.
10. `hyperforge-types` version bumped; tag `hyperforge-types-v<version>` local, not pushed.

## Completion

Implementor commits the accessor, tests, version bump, confirms workspace + consumer audit green, tags local, flips status to Complete in the same commit.
