---
id: IR-S02
title: "Spike: synapse-cc codegen extensibility"
status: Superseded
superseded_by: IR-7
type: spike
blocked_by: []
unlocks: [IR-7, IR-9]
severity: High
target_repo: hub-codegen
---

**Superseded by recon.** Spike question answered by exploration pass:

- **Big correction to the original framing:** synapse-cc (Haskell) is the orchestration layer, but actual code emission lives in `hub-codegen` (Rust) at `~/dev/controlflow/hypermemetic/hub-codegen/`. The IR-7 and IR-9 implementations live in hub-codegen, not synapse-cc.
- hub-codegen backends: TypeScript (fully), Rust (skeleton stub), Python (not implemented).
- Method emission: `hub-codegen/src/generator/typescript/namespaces.rs` lines 155–158. Hand-written string building; adding a comment above a method is a two-line addition (field on `MethodDef`, emit in `generate_namespace`).
- TypeScript's runtime (transport.ts, rpc.ts) is inlined into generated output, not a separate package — simplifies IR-9's framing.

Recon satisfies the spike's pass condition. IR-7 and IR-9 inherit the findings in their Context sections; their `target_repo` corrected to hub-codegen.

## Question

Can synapse-cc emit a target-language comment above a generated method based on a schema field, for at least one supported target language, without restructuring its codegen core?

## Setup

1. In a fork/branch of synapse-cc (`/Users/shmendez/dev/controlflow/hypermemetic/synapse-cc/`), identify the codegen entry point and the per-target-language emission paths.
2. Pick the target language with the simplest codegen path (whichever that turns out to be).
3. Add a test-only `deprecated: bool` flag on one method's schema in a fixture input.
4. Extend codegen for the chosen language to emit a single-line comment `// SPIKE-DEPRECATED` immediately above the generated method when `deprecated = true`.
5. Run synapse-cc against the fixture; inspect the generated output file.

## Pass condition

`grep -c "// SPIKE-DEPRECATED" <generated-file>` returns exactly 1, located immediately above the method's declaration.

Binary: marker present in correct position → PASS. Missing or in wrong position → FAIL.

## Fail → next

synapse-cc's codegen is not structurally extensible via simple per-field hooks. Open a design ticket (IR-7 would depend on it) for a codegen-hook architecture; IR-9's typed-handle work would share the same prerequisite.

## Fail → fallback

If extensibility requires structural work but a simpler "generate then post-process" approach is viable (run a second pass over generated files to inject comments based on a sidecar schema index), pin that as the implementation path for IR-7. IR-9 still likely needs structural work because typed-handle emission isn't post-processable.

## Time budget

Two focused hours. Over budget → stop and report.

## Out of scope

- Multi-backend validation (that's IR-7's acceptance scope).
- Native language deprecation markers (`@deprecated` JSDoc / `#[deprecated]` Rust). The spike only needs a raw comment.
- Any CLI flag / stderr behavior (IR-7 owns).

## Completion

Spike delivers: one commit to a throwaway branch of synapse-cc with the fixture + patch, pass/fail result, identification of which target language was chosen and why, and one-paragraph description of synapse-cc's codegen architecture as discovered during the spike. Report lands in IR-7 and IR-9's Context sections before either is promoted to Ready.
