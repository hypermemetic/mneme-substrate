---
id: IR-6
title: "synapse: surface deprecation warnings inline in the CLI"
status: Complete
type: implementation
blocked_by: [IR-4, IR-5]
unlocks: []
severity: Medium
target_repo: synapse
---

## Problem

After IR-4 and IR-5, `PluginSchema`, `MethodSchema`, and `ParamSchema` carry structured `DeprecationInfo` on any surface that's been marked deprecated. Synapse currently renders activation trees and invokes methods with no awareness of these markers — a user listing an activation's methods cannot see which ones are deprecated, and a user invoking a deprecated method gets no notice of the deprecation or the planned removal version. Consumers cannot plan migrations from inside the CLI.

## Context

Target crate: `synapse` (the Plexus RPC CLI). Source of schema metadata: whatever Plexus RPC introspection call synapse already uses to fetch `PluginSchema` from a connected server.

Deprecation metadata shape (from IR-2, IR-4, IR-5):

| Location | Field | Type |
|---|---|---|
| `PluginSchema.deprecation` | Activation-level deprecation | `Option<DeprecationInfo>` |
| `MethodSchema.deprecation` | Method-level deprecation | `Option<DeprecationInfo>` |
| `ParamSchema.field_deprecations` | Per-field deprecation on parameter types | `Vec<(String, DeprecationInfo)>` |
| `PluginSchema.children` (the field itself) | Field-level deprecation on the schema type | Declared via `#[deprecated]` — not data synapse reads, but synapse authors should avoid reading the field in rendering code |
| `PluginSchema.is_hub` (the field itself) | Field-level deprecation on the schema type | Same — synapse should prefer `PluginSchema::is_hub()` helper |

**User-visible affordances pinned for this ticket:**

- A visible marker (textual — e.g., `⚠`, `[DEPRECATED]`, or similar; exact character/string pinned in Acceptance 3) appears adjacent to the name of any deprecated method, deprecated activation, or deprecated parameter field when synapse renders a tree or a detail view.
- `help` / `info` output on a deprecated surface includes three lines: `since: <version>`, `removed_in: <version>`, `message: <string>`.
- Invoking a deprecated method prints the deprecation notice (prefixed with the visible marker) to **stderr** before the invocation's response is written to stdout. Pin: notice on stderr, response on stdout, so scripts piping stdout are unaffected.

## Required behavior

**Tree / list rendering:**

| Context | Marker placement |
|---|---|
| Listing methods under an activation (e.g., `synapse list <path>`) | Deprecated method name is prefixed with the marker. Non-deprecated methods render unchanged. |
| Listing activations (e.g., `synapse list` at the root) | Deprecated activation names are prefixed with the marker. |
| Detail view of a method (e.g., `synapse info <path>/method`) | After the existing description block, a `Deprecation:` section renders `since`, `removed_in`, and `message` fields — each on its own line, labeled. |
| Detail view of an activation | Same `Deprecation:` section if `PluginSchema.deprecation` is `Some`. |
| Detail view showing parameter fields | Each deprecated field renders with the marker, and `since` / `removed_in` / `message` are shown inline under that field's description. |

**Invocation:**

| Context | Behavior |
|---|---|
| User invokes a non-deprecated method | Unchanged — no notice. |
| User invokes a method whose `deprecation` is `Some` | Before invocation proceeds, a notice is printed to stderr: `<marker> DEPRECATED: <method path> — since <version>, removed in <version>. <message>`. Invocation proceeds normally; response is written to stdout. |
| User invokes a method on a deprecated activation (activation-level `deprecation` is `Some`) | Same stderr notice, but scoped to the activation. Notice printed once per invocation. |
| User invokes a method with parameters that include deprecated fields | After the method's own notice (if any), one additional stderr line per deprecated-field parameter the user actually supplied: `<marker> DEPRECATED parameter '<field>': since <version>, removed in <version>. <message>`. No notice for deprecated fields the user did not supply. |

**Color / TTY:**

- If stderr is a TTY and the user hasn't set `NO_COLOR`, the marker is rendered in the same color used by synapse's existing warning output (yellow or equivalent).
- If stderr is not a TTY, or `NO_COLOR` is set, the marker is plain text.

**Cache behavior:**

Deprecation metadata is read from whatever schema cache synapse already maintains. A refreshed schema (e.g., after a `--refresh-schema` flag or cache invalidation) re-reads the metadata. No new caching infrastructure is introduced.

## Risks

| Risk | Mitigation |
|---|---|
| The schema cache used by synapse was serialized before IR-2 landed (older cache entries) — those cache entries lack `deprecation` fields. | Serde defaults (from IR-2) yield `None` on deserialization. Existing caches produce no false-positive warnings. Acceptance 6 pins this. |
| User invoking a deprecated method in a scripted pipeline sees unexpected stderr output that interferes with log parsing. | Notices go to stderr, not stdout — scripts reading stdout are unaffected. Provide `--quiet-deprecations` flag to suppress (opt-out), matching synapse's existing flag conventions. |
| The marker character renders as mojibake on Windows consoles without UTF-8 support. | Provide plain-ASCII fallback `[DEPRECATED]` when `SYNAPSE_PLAIN_MARKERS=1` or similar; the exact env-var name is an implementation detail. Acceptance 3 accepts either marker shape. |
| Multiple deprecation notices on a single invocation (activation-level + method-level + parameter-field-level) produce noisy output. | Print each on its own stderr line, in stable order: activation → method → parameter fields. Acceptance 5 exercises this. |

## What must NOT change

- Synapse against a schema carrying no deprecation markers (pre-IR server, or a post-IR server with no deprecated surfaces) renders and invokes identically to pre-ticket behavior. No new stderr output, no new markers.
- Exit codes on successful invocation of a deprecated method are unchanged — the deprecation notice does not influence exit status.
- Response payload written to stdout is byte-identical to pre-ticket behavior. Only stderr gains the notice.
- Existing `synapse --help`, `synapse list`, `synapse info`, and invocation syntax are unchanged.
- Non-deprecated methods, activations, and parameters render exactly as before.

## Acceptance criteria

1. `cargo build -p synapse` (or whatever the synapse crate's cargo name is) succeeds.
2. `cargo test -p synapse` succeeds, including new tests covering the behaviors below.
3. An integration test — or equivalent end-to-end fixture — runs synapse against a fixture server exposing an activation with one deprecated method (`since: "0.5"`, `removed_in: "0.6"`, `message: "use foo2 instead"`) and one non-deprecated method. `synapse list <activation>` output contains:

   | Check | Expected |
   |---|---|
   | A line for the non-deprecated method | No marker, name renders plain. |
   | A line for the deprecated method | Contains the marker (either `⚠` or `[DEPRECATED]` — test accepts either). |

4. `synapse info <activation>/<deprecated-method>` output contains the literal substrings `"since: 0.5"`, `"removed_in: 0.6"`, and `"use foo2 instead"` — each on its own line under a `Deprecation:` header.
5. Invoking the deprecated method end-to-end writes its response payload to stdout (byte-identical to invoking the same method on a non-deprecated server) and writes to stderr a line containing: the marker, the substring `"DEPRECATED"`, the method name, `"since 0.5"`, `"removed in 0.6"`, and the message `"use foo2 instead"`.
6. Running synapse against a fixture server whose `PluginSchema` predates IR-2 (no `deprecation` fields on any method) produces zero deprecation notices on stderr — all list/info output is byte-identical to pre-ticket synapse.
7. Invoking a non-deprecated method on a fixture server writes zero bytes to stderr related to deprecation (other stderr output from the transport layer is unaffected).
8. An integration test covering an invocation that supplies a deprecated parameter field asserts that stderr contains one line per deprecated field actually supplied, each naming the field and its `since` / `removed_in` / `message`.

## Completion

- PR against synapse adding schema-reading logic that honors `DeprecationInfo`, rendering logic for tree/list/info views, and invocation-time stderr notices.
- Fixture Plexus RPC server definitions live alongside the test harness. The test may spin up a lightweight substrate with a test-only activation carrying the deprecation metadata configured in the Acceptance criteria.
- PR description includes the output of `cargo test -p synapse` — all green — and a transcript demonstrating a deprecated invocation with the stderr notice.
- Ticket status flipped from `Ready` → `Complete` in the same commit.
