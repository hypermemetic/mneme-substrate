---
id: IR-S03
title: "Spike: capability-intersection typing viability per target language"
status: Superseded
superseded_by: IR-9
type: spike
blocked_by: []
unlocks: [IR-9]
severity: High
target_repo: hub-codegen
---

**Superseded by recon.** Spike question ("can every target language express `DynamicChild<T> & Listable` with compile-time capability enforcement?") answered by language-level analysis against the backends hub-codegen currently supports:

- **TypeScript** — native `&` intersection operator. Passes trivially.
- **Rust** — no native intersection; maps to `T: TraitA + TraitB` trait bounds. Passes with different syntax.
- **Python** — not a hub-codegen backend today. When/if added, Protocol-based design is viable (PEP 544+). Deferred.

All currently-supported backends pass. IR-9 inherits the verdict in its Context. Python's approach is documented as a forward note, not a blocker.

## Question

Can every target language synapse-cc supports express `DynamicChild<T> & Listable` such that calling `.list()` compiles but calling `.search()` (not opted in) fails to compile at the same call site?

## Setup

This is a language-theory spike — no synapse-cc changes required. Per supported target language:

1. Hand-write a minimal module defining:
   - Generic `DynamicChild<T>` with `get(name) -> T | null` (or language-equivalent).
   - Marker trait/interface `Listable` with `list() -> AsyncIterable<string>`.
   - Marker trait/interface `Searchable` with `search(query) -> AsyncIterable<string>`.
   - A value with type `DynamicChild<Thing> & Listable` (or the language's equivalent form — trait impls in Rust, typed union in TypeScript, Protocol composition in Python, etc.).

2. Write TWO consumer snippets against that value:
   - Consumer A: calls `.get("x")` then `.list()`.
   - Consumer B: calls `.search("q")` on the same value.

3. Run the target language's type-checker / compiler over both consumers.

## Pass condition

Per target language:
- Consumer A compiles / type-checks cleanly.
- Consumer B fails type-checking with an error message that clearly indicates `.search` is not part of the value's type (not just a runtime-method-not-found).

Binary: both hold → PASS for that language. Either fails → FAIL for that language.

Overall spike pass: ALL supported target languages PASS.

## Fail → next (per language)

For each failing language, evaluate alternative shapes in order:
- **S03a — phantom-typed wrapper.** `DynamicChildWithList<T>` as a distinct named type that implements `list()` but not `search()`. More codegen boilerplate per combination but universal.
- **S03b — enum of capabilities.** Single `DynamicChild<T>` that returns an opt-set, forcing runtime capability checks via pattern match. Loses compile-time guarantee but works everywhere.
- **S03c — method-on-gate with runtime throw.** Gate exposes `.list()` / `.search()` that throw if capability absent. Worst option — purely runtime, no compile-time help.

Document in IR-9's Context which language falls back to which level, and re-scope IR-9's acceptance criteria accordingly (criterion 5 currently pins compile-time rejection; if any language can only do S03c, that criterion relaxes for that language specifically).

## Fail → fallback

If intersection typing fails in ALL supported target languages, IR-9 accepts runtime capability checks as its baseline with documentation-level typing hints. This would be a significant UX regression — escalate for replanning rather than silently accepting.

## Time budget

One hour per target language, in parallel. Three languages × 1h = 3h total wall time if done sequentially, but the work per language is independent.

## Out of scope

- Actual synapse-cc codegen generating these types (that's IR-9 proper).
- Hand-written runtime library design beyond what's needed to observe pass/fail.
- Error-message quality audits — only that the error exists and names the missing capability.

## Completion

Spike delivers one small module per supported target language (in scratch directories, not committed to any production tree), pass/fail per language, and the alternative shape landed at per language that failed. Report lands in IR-9's Context as a prerequisite before IR-9 is promoted to Ready.
