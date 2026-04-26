---
id: HF-IR-7
title: "#[child(list = \"credential_keys\")] gate: credential (dynamic) under AuthHub; extract credential methods"
status: Pending
type: implementation
blocked_by: [HF-IR-2]
unlocks: [HF-IR-8]
severity: Medium
target_repo: hyperforge
---

## Problem

`AuthHub` (namespaces `auth` / `secrets`) exposes credentials as a flat keyed surface: `get_credential(key)`, `set_credential(key, value)`, `delete_credential(key)`, `list_credentials()`. Natural addressing is `auth credential <key>.get` or `.rotate`. The `CredentialActivation` shell introduced in HF-IR-2 receives these; `AuthHub` gets a `#[child(list = "credential_keys")]` gate — plus optionally `search_method = "find_credential"` per HF-IR-S01's decision.

AuthHub is handled in parallel with repo/package/artifact gates (not gated on HF-IR-4/5/6 — it touches a disjoint hub file).

## Context

Per HF-IR-S01:

- `list_method = "credential_keys"`.
- `search_method = "find_credential"` if S01 decides AuthHub benefits from search — expected when `CredentialKey` has a structure that supports partial-match lookup. Otherwise `search_method = None`.

`CredentialKey` newtype per HF-TT may have `#[serde(skip)]` or similar on Display to avoid leaking key material in logs. The `credential_keys` stream yields key names (not values). The per-credential methods on `CredentialActivation` return values through the existing secret-handling path.

Methods extracted (final list per HF-IR-S01; expected):

| Source (flat on AuthHub) | Target (on CredentialActivation) | Kept flat? |
|---|---|---|
| `get_credential(key)` | `get()` | Yes, deprecated in HF-IR-9 |
| `set_credential(key, value)` | `set(value)` | Yes, deprecated in HF-IR-9 |
| `delete_credential(key)` | `delete()` | Yes, deprecated in HF-IR-9 |
| `list_credentials()` | n/a — superseded by `credential_keys` stream | Yes, deprecated in HF-IR-9 |
| `rotate_credential(key)` (if exists) | `rotate()` | Yes, deprecated in HF-IR-9 |

AuthHub's remaining methods (login flows, OAuth exchange, session tokens, etc.) stay flat — they are aggregates / procedures, not child-addressable resources. HF-IR-S01 pins the exact non-gate set.

## Required behavior

| Invocation | Behavior |
|---|---|
| `synapse auth credential` or `synapse secrets credential` | Tree-lists all credential keys via `credential_keys` stream. |
| `synapse auth credential <key> get` | Returns same value as `synapse auth get_credential key=<key>` pre-ticket. |
| `ChildRouter::get_child(auth_hub, "<valid-key>")` | `Some(CredentialActivation)`. |
| `ChildRouter::get_child(auth_hub, "<invalid-key>")` | `None`. |
| `plugin_schema()` on `AuthHub` | Contains a method entry named `credential` with `role: MethodRole::DynamicChild { list_method: Some("credential_keys"), search_method: <per S01: Some("find_credential") or None> }`. |
| Flat credential methods | Unchanged wire behavior. Deprecation in HF-IR-9. |
| `ChildCapabilities::LIST` | Set on `ChildRouter` impl for `AuthHub`. If search_method is `Some`, also set `SEARCH`. |

## Risks

| Risk | Mitigation |
|---|---|
| Exposing credential keys in synapse tree rendering leaks secret names to users who shouldn't see them. | `credential_keys` honors the same access controls the existing `list_credentials` enforced. If `list_credentials` was admin-gated, `credential_keys` inherits the same gating. Don't change auth semantics in this ticket. |
| `find_credential` search semantics (partial match, case-sensitivity). | HF-IR-S01 pins the contract if search_method is kept. If kept, test coverage demonstrates the pinned semantics. |
| Secret values travel through `CredentialActivation::get()` return — must not log inadvertently. | Preserve the existing logging hygiene of `get_credential`. If `CredentialKey`/value types carry `#[serde(skip)]` attrs, extraction must preserve them. |
| Auth flows (OAuth, session) use per-session state that may look like a child-addressable surface but is actually per-request. | Explicitly flat per HF-IR-S01 classification. Not moved. |

## What must NOT change

- Wire format and semantics of every existing `AuthHub` method.
- Access control on credential listing/retrieval.
- `CredentialKey` newtype and its logging / display semantics.
- Login / auth flow methods on `AuthHub`.
- Other hubs.

## Acceptance criteria

1. `cargo build --workspace` in hyperforge passes.
2. `cargo test --workspace` in hyperforge passes.
3. `cargo build` green in every sibling repo that depends on hyperforge.
4. `plugin_schema()` on `AuthHub` contains a method entry named `credential` with correct `MethodRole::DynamicChild` (search_method per S01).
5. `ChildRouter::get_child(auth_hub, "<valid-key>")` returns `Some(CredentialActivation)`; `get_child("<invalid-key>")` returns `None`.
6. `ChildCapabilities::LIST` set on `AuthHub`'s `ChildRouter` impl (plus `SEARCH` if search_method is `Some`).
7. For every method extracted into `CredentialActivation`, a test asserts the nested path returns byte-identical response to the flat method.
8. A regression test asserts that non-credential `AuthHub` methods (login, OAuth exchange, etc.) remain unchanged.
9. Hyperforge version remains `4.2.0`.
10. File-boundary scope: this ticket modifies `hubs/auth.rs` and the library file holding `CredentialActivation`. No edits to `hubs/workspace.rs`, `hubs/repo.rs`, `hubs/build.rs`, `hubs/images.rs`, `hubs/releases.rs`, or `hubs/hyperforge.rs`.

## Completion

Commit lands `#[child(list = "credential_keys")]` (+ optional search_method) + stream + extracted methods on `CredentialActivation`. `cargo build --workspace` + `cargo test --workspace` green. Status flipped to Complete in the same commit.
