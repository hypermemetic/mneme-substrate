---
id: HF-TT-2
title: "Introduce all newtypes in hyperforge-types (types-only, no consumers)"
status: Pending
type: implementation
blocked_by: [HF-TT-S01]
unlocks: [HF-TT-3, HF-TT-4, HF-TT-5, HF-TT-6, HF-TT-7]
target_repo: hyperforge
severity: High
---

## Problem

Every HF-TT migration ticket (3–7) needs the newtype definitions to exist before it can replace a call site. Introducing newtypes alongside call-site edits forces the same file to be touched twice — once to add the type, once to adopt it — and defeats the file-boundary parallelism that 3–7 depend on. The foundation ticket introduces every newtype at once, inside a freshly-scoped module tree in `hyperforge-types`, with zero consumers. The downstream cluster tickets then import from a stable surface.

## Context

After HF-DC, `hyperforge-types` exists as a workspace crate. HF-TT-S01's report pins the final inventory and the per-cluster file layout. Typical module shape (subject to S01's final pins):

```
crates/hyperforge-types/src/newtypes/
  mod.rs            # re-exports
  repo.rs           # RepoName, OrgName, WorkspaceName
  package.rs        # PackageName, ArtifactId
  version.rs        # Version, CommitRef, BranchRef, TagRef
  path.rs           # RepoPath, WorkspaceRoot
  credential.rs     # CredentialKey
  ecosystem.rs      # Ecosystem enum
```

Every newtype derives `Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema`. String newtypes use `#[serde(transparent)]`. `PathBuf` newtypes also use `#[serde(transparent)]` (the inner `PathBuf` already serializes as a string). `CredentialKey`'s `Display` redacts the inner value; the `Serialize` impl participates in the wire format as normal (this ticket does not introduce `#[serde(skip)]` on the whole type — that would break the wire).

## Required behavior

| Construct | Derives / attributes | Inner type | Notes |
|---|---|---|---|
| `RepoName` | `Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema`; `#[serde(transparent)]` | `String` | `new(impl Into<String>)`, `as_str(&self) -> &str`, `Display` via inner. |
| `OrgName` | same | `String` | same shape. |
| `WorkspaceName` | same | `String` | same shape. |
| `PackageName` | same | `String` | same shape. |
| `ArtifactId` | same | per S01 decision: `String` or `{ ecosystem, namespace, name }` | If parsed, ships `FromStr` and `Display` that round-trip `<ecosystem>:<namespace>:<name>`. |
| `Version` | same | `String` | No validation at construction; per-ecosystem parsing deferred. |
| `CommitRef` | same | `String` | Exposes `from_sha`, `from_tag`, `from_branch` constructors in addition to the generic `new`. |
| `BranchRef` | same | `String` | same shape. |
| `TagRef` | same | `String` | same shape. |
| `RepoPath` | same | `PathBuf` | `new(impl Into<PathBuf>)`, `as_path(&self) -> &Path`, `Display` via `Path::display`. |
| `WorkspaceRoot` | same | `PathBuf` | same shape. |
| `CredentialKey` | same | `String` | `Display` renders `"<redacted>"` (or equivalent fixed token). Serialization still transparent. |
| `Ecosystem` | `Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema`; `#[non_exhaustive]` | enum | Variants per S01 pin; at minimum `Rust, Haskell, JavaScript, Python, Go, Ruby, Elixir`. `serde(rename_all = "snake_case")`. |

Every newtype is `pub`. Module `crates/hyperforge-types/src/newtypes/mod.rs` re-exports every type so downstream code writes `use hyperforge_types::RepoName;` (not `hyperforge_types::newtypes::repo::RepoName`).

Round-trip serde tests in `crates/hyperforge-types/tests/newtype_serde.rs` confirm: for each string-backed newtype, `serde_json::to_string(&RepoName::new("foo"))` returns `"\"foo\""`; `serde_json::from_str::<RepoName>("\"foo\"")` returns `RepoName("foo")`. For `Ecosystem`, each variant round-trips as its snake_case name. For `ArtifactId` (if parsed), the string form round-trips through `Display` and `FromStr`.

No existing hyperforge code imports any of the new types in this ticket. The only diffs outside `crates/hyperforge-types/` are the dependency declaration in the workspace root `Cargo.toml` if the crate was not yet a workspace member (it is, post-HF-DC — so typically no root-level change).

## Risks

| Risk | Mitigation |
|---|---|
| Missing derive macro (e.g., `JsonSchema`) creates a feature mismatch with downstream consumers. | Ticket's acceptance requires all derives listed, checked by `cargo build -p hyperforge-types --all-features`. |
| Inner-type serialization differs from the provisional "byte-identical" contract. | Round-trip tests cover every newtype. |
| `CredentialKey`'s redacted Display causes a log line somewhere to read as `"<redacted>"` where a human expects the inner string. | No consumers yet in this ticket; flagged for HF-TT-6 to audit. |

## What must NOT change

- No public API outside `hyperforge-types`.
- No existing newtype gets removed or renamed (there are none today).
- Zero call-site edits in `hyperforge-core`, `hyperforge-hubs`, bins, or sibling repos. If a grep after this ticket shows any hyperforge-core file importing one of the new newtypes, the ticket has drifted scope.
- Wire format of any pre-existing type that happens to be in `hyperforge-types` is byte-identical pre/post (since this ticket only adds types, not alters existing ones).

## Acceptance criteria

1. Every newtype listed in the Required-behavior table exists in `crates/hyperforge-types/src/newtypes/` in the file cluster pinned by HF-TT-S01.
2. Every newtype's derives match the Required-behavior table.
3. String-backed newtypes carry `#[serde(transparent)]`. `PathBuf`-backed newtypes carry `#[serde(transparent)]`. `Ecosystem` carries `#[non_exhaustive]` and `#[serde(rename_all = "snake_case")]`.
4. Round-trip serde test suite (`crates/hyperforge-types/tests/newtype_serde.rs`) passes: each newtype serializes and deserializes to/from its byte-identical string (or snake_case enum name) form.
5. `CredentialKey::Display` returns the redaction token; `CredentialKey::Serialize` emits the inner string (verified in test).
6. `cargo build --workspace` green in hyperforge.
7. `cargo test --workspace` green in hyperforge.
8. No file outside `crates/hyperforge-types/` is modified in this ticket (verified by `git diff --stat`).
9. `hyperforge-types` version bumped (minor: `0.1.x` → `0.2.0`, or equivalent per the crate's current version); tag `hyperforge-types-v<new-version>` created locally, not pushed.

## Completion

Implementor commits the new module tree + tests + Cargo.toml version bump, confirms `cargo build --workspace && cargo test --workspace` green, tags `hyperforge-types-v<version>` locally, flips status to Complete in the same commit.
