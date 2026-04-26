---
id: RUSTGEN-S01
title: "Spike: Rust runtime library shape — inline vs sibling crate vs vendored"
status: Pending
type: spike
blocked_by: []
unlocks: [RUSTGEN-2, RUSTGEN-3, RUSTGEN-4, RUSTGEN-5]
severity: High
target_repo: hub-codegen
---

## Question

Where does the Rust runtime library live — the code that hub-codegen's generated output depends on (transport, `DynamicChild` trait, `Listable`/`Searchable` traits, `PlexusStreamItem`, `call_stream` / `call_single` helpers)?

Three candidates:

| Option | Shape | Trade-off |
|---|---|---|
| **A. Inline** | All runtime code is emitted into the generated crate verbatim, every time. | Zero external deps; self-contained output. Duplication across consumer crates; upgrades require regen. |
| **B. Sibling crate** | Generated crate depends on a published / pathed `plexus-client-runtime = "0.1"` crate in the hypermemetic workspace. | Clean separation; DRY; upgradeable. Requires publishing/maintenance cadence. Every consumer pulls the runtime as a dep. |
| **C. Vendored from plexus-transport** | Reuse the existing `plexus-transport` crate — hub-codegen-generated output imports it directly. | Reuses battle-tested transport. Couples generated output to substrate's internal library. `plexus-transport` must stay stable. |

TypeScript's answer: hybrid. Core transport types (`PlexusStreamItem`, etc.) are emitted into the generated package (inline). The `DynamicChild` / `Listable` / `Searchable` interface shapes are also emitted inline. The JSON-RPC logic is generated into `rpc.ts` (inline). The WebSocket client is `transport.ts` (inline). There is no sibling TS runtime package — everything is vendored into generated output.

Rust may or may not want the same shape. A sibling crate (option B) may be more idiomatic for Rust given trait orphan rules and `cargo` ergonomics; a small runtime crate avoids duplicating trait definitions across every consumer crate (which would prevent them from being the same trait — consumers couldn't pass a `Box<dyn DynamicChild<...>>` across crate boundaries because each crate defines its own `DynamicChild` trait).

## Setup

1. Pick a minimal fixture IR: one activation with one `#[method]` and one dynamic-child gate (existing `solar` fixture or a slimmer one). Ensure a child activation's schema is in the IR batch.
2. For each option A / B / C, generate a throwaway sample output manually (no hub-codegen changes required — write the files by hand).
3. For each sample, stand up a second consumer crate (`spike_consumer`) that imports the generated client and:
   - Constructs the client: `let c = PlexusClient::new("ws://localhost:8080");`
   - Calls the dynamic-child gate: `c.<hub>.<gate>.get("name").await?.<method>().await?` (compile-check is enough — no live server required).
   - Passes a `Box<dyn DynamicChild<Child = ChildClient>>` from the generated crate INTO a function defined in `spike_consumer`. This tests trait-orphan-rule safety: if each crate redefines the trait, this pattern breaks.
4. `cargo check` the `spike_consumer` crate against each of the three runtime-library shapes.

## Pass condition

Binary: the first option that satisfies ALL of these passes —

- [ ] `cargo check` on `spike_consumer` succeeds.
- [ ] `Box<dyn DynamicChild<Child = ChildClient>>` round-trips across the generated crate and `spike_consumer` without orphan-rule errors.
- [ ] The generated crate has no more than one runtime-shape dependency (inline = zero; sibling crate = one; vendored = one pathed).
- [ ] No duplication of trait DEFINITIONS (as opposed to USES) across generated crates — if two consumer crates regen from the same IR, the `DynamicChild` trait is the same trait, not two different traits with the same name.

If more than one option passes: pick the one with the cleanest dependency graph (fewest transitive deps). Ties broken by "matches TS backend's inline pattern" (option A).

## Fail → next

If option A (inline) fails the orphan-rule check, try option B (sibling crate). Create a `plexus-client-runtime` crate at `~/dev/controlflow/hypermemetic/plexus-client-runtime/` (path dep; not published for the spike) with just the trait definitions. Regenerate the sample output without inline traits; the generated crate depends on the runtime crate. Re-run the consumer check.

If option B fails, try option C (vendor from `plexus-transport`). Requires verifying `plexus-transport` exposes the necessary traits publicly; if not, fall-back is out of scope for this spike and escalates to a replanning trigger on RUSTGEN-1.

## Fail → fallback

If all three options fail, the Rust backend must use type erasure (`Box<dyn std::any::Any>` or a trait object with `dyn-compatible` restriction) at the gate boundary, sacrificing the typed-handle guarantee that IR-9 established for TypeScript. This is a serious degradation and triggers a replanning conversation before RUSTGEN-2 is promoted.

## Time budget

Four focused hours. If the spike exceeds this, stop and report regardless of pass/fail state.

## Out of scope

- Benchmarking the runtime. Correctness only; perf is a downstream concern.
- Publishing a runtime crate to crates.io. A pathed dep is sufficient for the spike.
- Alternative transport (HTTP long-poll, gRPC, etc.). WebSocket is the current protocol; changing that is not RUSTGEN's concern.
- Deciding the runtime crate's versioning scheme (if option B wins). That's a follow-up ticket.

## Completion

Spike delivers:

- A spike directory under `hub-codegen/spike/rustgen-s01/` containing the three sample outputs (A, B, C) and the `spike_consumer` crate.
- A decision doc in the same directory named `DECISION.md` containing: which option passed, the consumer-check output for the passing option (`cargo check` success transcript), and the rationale for rejecting the others.
- Pass/fail result embedded in the decision doc's frontmatter or first paragraph.
- Time spent.

Report lands in RUSTGEN-2's Context section as a reference before RUSTGEN-2 is promoted to `Ready`. The decision pins the runtime-library shape for RUSTGEN-2 through RUSTGEN-9.
