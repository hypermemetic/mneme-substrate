---
id: HF-TT-9
title: "Call-site sweep: hyperforge bins adopt every newtype"
status: Pending
type: implementation
blocked_by: [HF-TT-8]
unlocks: [HF-TT-10]
target_repo: hyperforge
severity: Medium
---

## Problem

HF-TT-8 tightened `hyperforge-hubs`' public signatures, leaving the bin crates (`hyperforge`, `hyperforge-auth`, `hyperforge-ssh`) with transitional casts at their wire-layer boundaries. Bins construct newtypes from raw CLI arg strings or wire-deserialized values, then pass them to hub methods. This ticket removes every transitional cast inside bins, relies on `#[serde(transparent)]` + `clap` value-parsers to construct newtypes at the CLI/wire seam, and brings bin code to the same type-discipline bar as hubs and core.

## Context

Bin crates post-HF-DC: `hyperforge` (CLI adapter + server startup), `hyperforge-auth` (secrets sidecar), `hyperforge-ssh` (SSH handler). Each accepts CLI args and/or wire payloads, constructs values, and calls hub methods.

Newtype construction at the CLI seam uses `clap`'s `value_parser` or a `FromStr` impl. For every string-backed newtype, `FromStr` is trivially `Ok(Self::new(s))`. This ticket adds the `FromStr` derivation (via a `impl FromStr for RepoName { ... }` block in `hyperforge-types` if not already present from HF-TT-2; if added here, `hyperforge-types` gets a patch bump).

Wire-deserialization at server startup or sidecar IPC: `serde_json::from_str` handles newtype construction transparently because of `#[serde(transparent)]`. No additional glue.

File-boundary discipline: this ticket edits bin crates only (`crates/hyperforge/`, `crates/hyperforge-auth/`, `crates/hyperforge-ssh/`). It may also add `FromStr` impls in `hyperforge-types` if HF-TT-2 didn't ship them; if so, that is a disjoint file-level addition.

## Required behavior

| Before (bin) | After |
|---|---|
| `let name: String = args.name;` then pass to hub as `String` | `let name: RepoName = args.name;` (clap value_parser uses `FromStr`); hub accepts `RepoName` directly. |
| `RepoName::new(arg_string)` inline cast | Removed; `FromStr` on the `clap` arg declaration handles it. |
| `serde_json::from_str::<String>(raw)` followed by `PackageName::new(...)` | `serde_json::from_str::<PackageName>(raw)` — single step. |
| Bin logs that interpolated a `CredentialKey` raw | Now emit `"<redacted>"` via the type's `Display`. |

Wire-compat at the bin-to-user interface: CLI arg grammar is byte-identical — users still pass `--repo plexus-substrate`. The `FromStr` parse is transparent. CLI help text is byte-identical (clap doesn't include type name in help output unless explicitly configured).

Transitional-cast inventory: at ticket completion, `grep -rn '<Newtype>::new(' crates/hyperforge/ crates/hyperforge-auth/ crates/hyperforge-ssh/` returns zero (modulo FromStr trivial-body impls and any wire-glue flagged in commit message).

## Risks

| Risk | Mitigation |
|---|---|
| A `clap` arg's value-parser configuration doesn't pick up `FromStr` by default. | Explicit `value_parser = clap::value_parser!(RepoName)` where needed. |
| CLI help text shape changes if `clap` reflects type name. | Snapshot test on `hyperforge --help` output; byte-identical against pre-migration snapshot. |
| A `CredentialKey` in a log line regressed visibility that an operator relied on. | Audit log changes documented in commit message. Operators can opt into `CredentialKey::prefix(n)` for audit breadcrumbs. |

## What must NOT change

- CLI arg grammar (byte-identical usage strings).
- `hyperforge --help` output (byte-identical).
- Exit codes and output format.
- Wire format of any sidecar or server endpoint.
- Files outside the three bin crates + any `FromStr` additions in `hyperforge-types`.

## Acceptance criteria

1. Every bin call to a hub method passes newtypes directly, not constructed inline.
2. `FromStr` impls exist for every string-backed newtype in `hyperforge-types` (added in HF-TT-2 or here).
3. Transitional-cast sweep: `grep` for `<Newtype>::new(` across bin crates returns zero modulo flagged glue.
4. CLI snapshot test on `hyperforge --help` matches pre-migration snapshot byte-identically.
5. CLI arg grammar test: every command that takes a repo/org/package/etc. arg still parses its expected input unchanged.
6. `cargo build --workspace` green in hyperforge.
7. `cargo test --workspace` green in hyperforge.
8. File-boundary check: edits confined to bin crates + possible `FromStr` additions in `hyperforge-types`.
9. Sibling-repo audit: consumer repos still build (bins have no library surface, so this is normally trivial).
10. Version bumps: bin crates bumped (patch, since CLI surface is unchanged); `hyperforge-types` patch-bumped if `FromStr` added here. Tags local, not pushed.

## Completion

Implementor commits bin migration, CLI snapshot, version bumps, confirms workspace + consumer audit green, tags local, flips status to Complete in the same commit.
