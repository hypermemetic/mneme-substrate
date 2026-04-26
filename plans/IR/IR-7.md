---
id: IR-7
title: "synapse-cc: track IR version and annotate codegen output consuming deprecated fields"
status: Complete
type: implementation
blocked_by: [IR-2, IR-5]
unlocks: []
severity: Medium
target_repo: hub-codegen
---

## Problem

synapse-cc (codegen CLI at `~/dev/controlflow/hypermemetic/synapse-cc/`) reads a Plexus RPC `PluginSchema` and emits target-language client code. After IR-2 through IR-5 land, schemas carry `DeprecationInfo` on methods, activations, parameter fields, and on the deprecated `children` / `is_hub` / `ChildCapabilities` surfaces themselves. Today synapse-cc treats all schema surfaces uniformly — it emits client code that reads deprecated fields (e.g., treating `children: Vec<ChildSummary>` as authoritative) with no signal to the consumer that the source surface is slated for removal. Consumers regenerating their clients cannot see, from the generated code alone, which parts of their integration will break in a future plexus-core release.

## Context

Target crate: synapse-cc at `/Users/shmendez/dev/controlflow/hypermemetic/synapse-cc/` (outside the substrate workspace; separate repo).

**IR-version declaration on the schema:**

IR-2 did not explicitly pin an "IR version" field on `PluginSchema`. For this ticket, synapse-cc relies on the following inference:

| Schema observation | Assumed IR version |
|---|---|
| `PluginSchema.methods[].role` is present on deserialization | IR-2 or later (call it "0.5+") |
| `PluginSchema.methods[].role` is absent (older serialized schemas) | Pre-IR ("0.4") |
| The schema itself carries an explicit `ir_version: String` field | Use the declared value verbatim |

If synapse-cc later needs a stronger version pin, adding `ir_version: String` to `PluginSchema` is a future ticket. This ticket's scope: infer from structural presence.

**Deprecated surfaces synapse-cc's codegen must annotate when consuming them:**

| Surface | Deprecation reason to emit in the annotation |
|---|---|
| `PluginSchema.children` | "Derive from MethodRole on MethodSchema." |
| `PluginSchema.is_hub` | "Use the `is_hub()` query helper on PluginSchema." |
| `ChildCapabilities` (as a type referenced in generated router code) | "Use MethodRole::DynamicChild { list_method, search_method } on the gate method." |
| Any `MethodSchema.deprecation: Some(_)` | Use the schema's own message verbatim. |
| Any `PluginSchema.deprecation: Some(_)` | Use the schema's own message verbatim. |
| Any `ParamSchema.field_deprecations[*]` entry | Use the schema's own message verbatim, prefixed with the field name. |

**Annotation shape in generated code:**

A single-line comment in the target language's comment syntax, placed immediately above the declaration/reference, of the form:

```
// DEPRECATED since <since_version>, removed in <removed_in_version>: <message>
```

For target languages that use `#`, `--`, or `/* */` comment syntax, the comment character is adjusted accordingly. The **format of the body** is pinned (byte-identical except for the comment leader).

## Required behavior

**IR-version detection:**

synapse-cc detects whether the input schema is pre-IR or post-IR (per the table in Context). Post-IR schemas enable deprecation scanning; pre-IR schemas skip it entirely.

**Codegen annotation:**

For each generated code artifact (function, struct, field, type reference) that is **sourced from** a deprecated schema surface:

| Codegen action | Annotation emitted |
|---|---|
| Generated struct field corresponds to a deprecated `ParamSchema` field entry | Single-line comment above the field declaration (format above). |
| Generated client method corresponds to a `MethodSchema` with `deprecation: Some(_)` | Single-line comment above the method. In target languages with native deprecation markers (e.g., Python's `@deprecated`, TypeScript's `@deprecated` JSDoc), the native marker is **also** emitted alongside the comment. |
| Generated code references `PluginSchema.children` (e.g., to enumerate children in a generated helper) | Single-line comment above the reference site. |
| Generated code references `PluginSchema.is_hub` | Single-line comment above the reference site. |
| Generated code references `ChildCapabilities` | Single-line comment above the type reference. |

**Severity and CLI flags:**

| Flag | Behavior |
|---|---|
| (default — no flag) | For each deprecated-surface consumption in the regen output, print one line to stderr: `WARNING: generated code consumes deprecated <surface> at <target-language-file>:<line> — <message>`. Exit code 0 on success. |
| `--fail-on-deprecated` | Same stderr output, but on any deprecated consumption, exit with non-zero status **after** writing the generated files. Partial output is left on disk (consistent with current synapse-cc behavior on non-fatal issues; if synapse-cc's current error model requires full rollback, follow that convention). Acceptance 5 pins this. |
| `--no-deprecation-annotations` | Suppress both the generated-file annotations and the stderr warnings. Codegen proceeds as if the schema were pre-IR. Useful for consumers who have opted out. |

**Pre-IR regression:**

When the input schema is pre-IR (no `role` field on any method), synapse-cc's output is **byte-identical** to pre-ticket output. No annotations, no warnings, no stderr output related to deprecation. Acceptance 6 pins this.

## Risks

| Risk | Mitigation |
|---|---|
| synapse-cc supports multiple target languages; each has different comment syntax and native deprecation markers. | Annotation format is parameterized per target-language backend. The body format (`since <X>, removed in <Y>: <message>`) is identical across backends; only the comment leader changes. Acceptance 4 covers at least two backends. |
| `--fail-on-deprecated` races with in-progress codegen writes. | synapse-cc writes output files first, then checks for deprecated consumption, then exits. If any deprecated consumption was recorded, exit non-zero. Generated files stay on disk. This matches synapse-cc's current "best-effort-output" model. |
| IR-version detection via structural presence is fragile — a pre-IR schema that happens to be augmented by a user with a synthetic `role` field would be misclassified. | Accept the risk; the fallback to "0.4 / pre-IR" is safe (annotations are suppressed, regen output unchanged). When IR adds an explicit `ir_version` field in a future ticket, synapse-cc reads it preferentially. Documented in synapse-cc's user-facing docs. |
| Consumers regenerating against a rapidly-changing schema see churn in generated diffs because of annotation additions. | Annotations are deterministic in ordering and content given a fixed input schema. Two regenerations against the same schema produce byte-identical output. Acceptance 7 pins. |

## What must NOT change

- Regenerating against a pre-IR substrate schema produces output indistinguishable from today's synapse-cc output. No new comments, no new stderr output.
- synapse-cc's existing CLI surface (subcommands, positional args, existing flags) is unchanged except for the additions `--fail-on-deprecated` and `--no-deprecation-annotations`.
- Default severity is WARNING (stderr, non-fatal, exit 0). No behavior change for users who don't pass the new flags and whose target schemas are pre-IR.
- Generated code compiles / runs in its target language exactly as before — annotations are comments only (plus native deprecation markers where applicable); they don't change semantics.

## Acceptance criteria

1. `cargo build` and `cargo test` in the synapse-cc repo succeed.
2. An integration test regenerates client code against a fixture post-IR schema exposing one deprecated method (`since: "0.5"`, `removed_in: "0.6"`, `message: "use foo2"`). The generated client source for that method's declaration contains (in the target-language's comment syntax) the exact substring: `DEPRECATED since 0.5, removed in 0.6: use foo2`.
3. The same regen run writes to stderr at least one line containing: `WARNING`, the method name, `0.5`, `0.6`, and `use foo2`. Exit code is 0.
4. The integration test covers at least two target-language backends (whichever are currently supported by synapse-cc — e.g., Rust + Python, or Rust + TypeScript). Both backends emit the annotation in the correct comment syntax for their language, with identical body content.
5. Running the same regen with `--fail-on-deprecated` produces the same stderr output and the same generated files on disk, but exits with non-zero status.
6. Regenerating against a pre-IR substrate schema (all methods serialized without a `role` field) produces output files byte-identical to the pre-ticket regen output. `diff` against the captured pre-ticket snapshot yields zero lines of difference. Stderr contains zero deprecation-related warnings.
7. Two consecutive regens against the same post-IR schema produce byte-identical generated files. (Determinism pin.)
8. Passing `--no-deprecation-annotations` against a post-IR schema produces generated files containing zero deprecation annotations and stderr containing zero deprecation warnings.
9. The generated client code, when compiled or executed in its target language, produces no new compilation errors or runtime errors attributable to this ticket's changes — only comments and native deprecation markers are added, never semantics.

## Completion

- PR against the synapse-cc repo adding IR-version detection, per-target-language annotation emission, the two new CLI flags, and the integration tests.
- PR description includes `cargo test` output and a diff demonstrating the annotation on at least two target-language backends.
- Ticket status flipped from `Ready` → `Complete` in the same commit.
