---
id: HF-IR-9
title: "Deprecate flat list/get method pairs across hyperforge with DeprecationInfo"
status: Pending
type: implementation
blocked_by: [HF-IR-8]
unlocks: [HF-IR-10]
severity: Medium
target_repo: hyperforge
---

## Problem

HF-IR-4..8 added dynamic child gates on every hub and extracted per-id method bodies into child activations. The flat `list_X` / `get_X` / `<verb>_X(id)` pairs still exist for wire-compat — they route to the same underlying logic and are semantically identical to the nested path. Consumers have no in-CLI signal that the flat methods are being phased out and no migration pointer to the nested path.

This ticket attaches `DeprecationInfo` to every flat method superseded by a child gate, per IR-4/IR-5's `DeprecationInfo` primitive. Synapse's rendering (IR-6) and invocation-warning (IR-15) pipelines pick up the metadata and surface it to users.

## Context

IR-4 and IR-5 added `deprecation: Option<DeprecationInfo>` to `MethodSchema` and `PluginSchema`. `DeprecationInfo` shape:

```rust
pub struct DeprecationInfo {
    pub since: String,          // e.g., "4.2.0"
    pub removed_in: String,     // e.g., "5.0.0"
    pub message: String,        // migration pointer
}
```

Authoring deprecation in hyperforge: per plexus-substrate's IR-2 convention, the activation macro's `#[plexus_macros::method]` accepts a `deprecated(since = "...", removed_in = "...", message = "...")` arg, and the generated schema carries the `DeprecationInfo`. If the macro doesn't yet support the arg in hyperforge's pinned `plexus-macros` version, this ticket bumps the pin (hyperforge's `plexus-macros` dep was already updated for HF-IR-4's `#[child]` use) or attaches deprecation via a schema post-hook.

Methods to deprecate (final set derived from HF-IR-4..8; authoritative list per HF-IR-S01's ratified mapping):

| Hub | Flat method | Replacement | Since | Removed in |
|---|---|---|---|---|
| `WorkspaceHub` | `list_repos` | `workspace.repo` (tree) / `repo_names` (stream) | `4.2.0` | `5.0.0` |
| `WorkspaceHub` | `get_repo` | `workspace.repo <name>.info` | `4.2.0` | `5.0.0` |
| `WorkspaceHub` | `status(name)` | `workspace.repo <name>.status` | `4.2.0` | `5.0.0` |
| `WorkspaceHub` | `pull(name)` | `workspace.repo <name>.pull` | `4.2.0` | `5.0.0` |
| `RepoActivation` (or `RepoHub` if S01 pinned) | `list_packages` | `repo <r>.package` / `package_names` | `4.2.0` | `5.0.0` |
| `RepoActivation` | `get_package` | `repo <r>.package <p>.info` | `4.2.0` | `5.0.0` |
| `RepoActivation` | `build(pkg)` | `repo <r>.package <p>.build` | `4.2.0` | `5.0.0` |
| `RepoActivation` | `test(pkg)` | `repo <r>.package <p>.test` | `4.2.0` | `5.0.0` |
| `RepoActivation` | `publish(pkg)` | `repo <r>.package <p>.publish` | `4.2.0` | `5.0.0` |
| `RepoActivation` | `list_artifacts` | `repo <r>.artifact` / `artifact_ids` | `4.2.0` | `5.0.0` |
| `RepoActivation` | `get_artifact` | `repo <r>.artifact <id>.info` | `4.2.0` | `5.0.0` |
| `RepoActivation` | `download_artifact` | `repo <r>.artifact <id>.download` | `4.2.0` | `5.0.0` |
| `RepoActivation` | `inspect_artifact` | `repo <r>.artifact <id>.inspect` | `4.2.0` | `5.0.0` |
| `ReleasesHub` | `list_releases` | `releases.release` / `release_versions` | `4.2.0` | `5.0.0` |
| `ReleasesHub` | `get_release` | `releases.release <v>.info` | `4.2.0` | `5.0.0` |
| `ReleasesHub` | `release_artifacts` | `releases.release <v>.artifacts` | `4.2.0` | `5.0.0` |
| `ReleasesHub` | `publish_notes` | `releases.release <v>.publish_notes` | `4.2.0` | `5.0.0` |
| `ImagesHub` | `list_images` | `images.image` / `image_ids` | `4.2.0` | `5.0.0` |
| `ImagesHub` | `get_image` | `images.image <id>.inspect` | `4.2.0` | `5.0.0` |
| `ImagesHub` | `tag_image` | `images.image <id>.tag` | `4.2.0` | `5.0.0` |
| `ImagesHub` | `pull_image` | `images.image <id>.pull` | `4.2.0` | `5.0.0` |
| `ImagesHub` | `push_image` | `images.image <id>.push` | `4.2.0` | `5.0.0` |
| `AuthHub` | `list_credentials` | `auth.credential` / `credential_keys` | `4.2.0` | `5.0.0` |
| `AuthHub` | `get_credential` | `auth.credential <key>.get` | `4.2.0` | `5.0.0` |
| `AuthHub` | `set_credential` | `auth.credential <key>.set` | `4.2.0` | `5.0.0` |
| `AuthHub` | `delete_credential` | `auth.credential <key>.delete` | `4.2.0` | `5.0.0` |

Every entry's `message` follows the template:

> "Use <replacement nested path>. This method is deprecated since 4.2.0 and will be removed in 5.0.0."

Any method in HF-IR-S01's "stays flat — not a gate" classification is NOT deprecated.

## Required behavior

| Surface | Behavior |
|---|---|
| `plugin_schema()` on each hub | Every deprecated method's `MethodSchema.deprecation` is `Some(DeprecationInfo { since: "4.2.0", removed_in: "5.0.0", message: "..." })`. |
| Wire invocation of a deprecated method | Returns same result as pre-ticket. No functional change. The deprecation is metadata only. |
| Synapse rendering | Per IR-6: deprecated methods render with a `⚠` prefix or `[DEPRECATED]` marker in the tree; `info` view shows the three-line Deprecation block. |
| Synapse invocation | Per IR-15: deprecated method invocation writes a notice to stderr before the response is written to stdout. Response payload is byte-identical. |
| Rust-side `#[deprecated]` | Each deprecated method also carries a standard Rust `#[deprecated(since = "4.2.0", note = "...")]` attribute so in-process callers get a compiler warning. (This is additional to the schema-level `DeprecationInfo`.) |

## Risks

| Risk | Mitigation |
|---|---|
| The `plexus-macros` version hyperforge pins may not support the `deprecated` arg on `#[method]`. | Hyperforge bumped its plexus-macros dep for HF-IR-4's `#[child]` work. If `deprecated` is still missing, either (a) bump further, or (b) attach `DeprecationInfo` via a schema post-processing hook on each hub's `plugin_schema()`. Pin the choice in implementation. |
| Rust `#[deprecated]` attribute emits warnings inside hyperforge's own codebase (flat methods still call into their helpers from HF-IR-4..8). | Allow the warning at the helper-call call site with `#[allow(deprecated)]` where the flat method's body calls into the extracted helpers. Do not silence at the crate level. |
| Downstream sibling consumers call deprecated methods — their builds now warn. | Deprecation warnings, not errors. Consumers see the warning and migrate on their own timeline. No consumer-side change is required by this ticket. |
| A method gets accidentally deprecated that HF-IR-S01 classified as "stays flat". | Cross-reference HF-IR-S01's mapping table during implementation. A test asserts the set of deprecated methods matches the mapping exactly. |

## What must NOT change

- Wire invocation behavior — response payloads are byte-identical.
- Non-deprecated methods (including those classified "stays flat") — no `DeprecationInfo` on them.
- Child-gate methods added in HF-IR-3..8 — no deprecation on the new surface.
- Wire / schema field ordering — only the `deprecation: Option<...>` field values change.
- `PluginSchema` / `MethodSchema` types themselves — those come from upstream plexus-core, not hyperforge.

## Acceptance criteria

1. `cargo build --workspace` in hyperforge passes.
2. `cargo test --workspace` in hyperforge passes.
3. `cargo build` green in every sibling repo that depends on hyperforge.
4. A test asserts that every method in the "Methods to deprecate" table (dereferenced through HF-IR-S01's ratified mapping) has `MethodSchema.deprecation == Some(DeprecationInfo { since: "4.2.0", removed_in: "5.0.0", message: <non-empty> })`.
5. A test asserts that every method NOT in the deprecation set has `MethodSchema.deprecation == None`.
6. A test asserts that every deprecated method's Rust-level `#[deprecated]` attribute is present (compile-time check via a doc test or `#[deny(deprecated)]` on a known-clean fixture).
7. A manual check: invoking a deprecated method end-to-end returns a byte-identical response to pre-ticket.
8. Hyperforge version remains `4.2.0` — this ticket is the last substantial HF-IR ticket before integration verification (HF-IR-10). Since 4.2.0 already reflects the public-surface change, the deprecation metadata additions fall under 4.2.0 as well.
9. Local tag `hyperforge-v4.2.0` (or whatever hyperforge's canonical tag naming is) created after this commit lands and the integration gate passes — not pushed. (Tag is documented here; actual tag creation happens when HF-IR-9 lands since it's the final surface-bearing ticket; HF-IR-10 is verification only.)

## Completion

Commit lands `DeprecationInfo` on every flat method superseded by a child gate, matching the ratified mapping. `cargo build --workspace` + `cargo test --workspace` green. Local tag `hyperforge-v4.2.0` created, not pushed. Status flipped to Complete in the same commit.
