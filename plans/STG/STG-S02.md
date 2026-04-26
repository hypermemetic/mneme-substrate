---
id: STG-S02
title: "Spike: hotswap proof — in-memory MustacheStore passes the same tests"
status: Pending
type: spike
blocked_by: [STG-S01]
unlocks: [STG-2]
severity: High
target_repo: plexus-substrate
---

## Question

Given STG-S01's `MustacheStore` trait, can a second, fundamentally different backend (an in-memory `HashMap`-backed `InMemoryMustacheStore`) implement the trait and pass the exact same test suite that `SqliteMustacheStore` passed?

This spike is the hotswap proof. STG-S01 showed that the trait can wrap the existing backend. STG-S02 shows that the trait is real — that a second implementation with zero shared code passes the same contract. Without this spike, "trait abstraction" is a rename, not a seam.

## Setup

1. Build on STG-S01's branch (or a branch off STG-S01's branch tip).
2. Implement `InMemoryMustacheStore`:
   - Storage: `Arc<Mutex<HashMap<(Uuid, String, String), TemplateRow>>>` or equivalent, where `TemplateRow` carries `id`, `template`, `created_at`, `updated_at`.
   - Implement every method of `MustacheStore` using the in-memory structure. No SQLite, no async I/O except the `async fn` signature itself.
   - Generate IDs via `Uuid::new_v4().to_string()` (same as the SQLite backend).
   - `current_timestamp()` via `SystemTime::now()` (same as the SQLite backend).
3. Parameterize the existing Mustache storage tests over the backend. Two approaches — pick one:
   - **(a)** Duplicate the four tests, one duplicate per backend (`test_set_and_get_template_sqlite`, `test_set_and_get_template_memory`, etc.).
   - **(b)** Rewrite the tests to take a `Box<dyn MustacheStore>` factory fn and run the same assertion body against each. Requires a small test harness.

   STG-2 will canonicalize the approach. For the spike, either is acceptable — prefer (b) if it compiles cleanly within the time budget.
4. Run the whole Mustache test suite. Every assertion passes against both backends.

## Pass condition

All four Mustache test assertions (`test_set_and_get_template`, `test_update_template`, `test_list_templates`, `test_delete_template`) pass against **both** `SqliteMustacheStore` **and** `InMemoryMustacheStore`, with identical assertion bodies for each backend.

Binary: eight test runs (four tests × two backends) pass → PASS. Any failure, semantic divergence, or required assertion divergence → FAIL.

## Fail → next

If the in-memory backend cannot reproduce the SQLite backend's behavior for a specific test (e.g., ordering in `list_templates`, `created_at` preservation across `set_template` updates), investigate whether the test encodes a SQLite-specific behavior or a genuine contract. If it's a SQLite-specific behavior, tighten the `MustacheStore` contract in the trait's docstring and amend the in-memory impl to match. Re-run.

If a genuine semantic gap emerges that cannot be reconciled (e.g., two backends fundamentally order results differently and the test asserts a specific order), the contract is under-specified. Document the gap as a finding for STG-2.

## Fail → fallback

If the backends cannot be made to agree on the current Mustache test contract without loosening assertions, STG-2 must land a tightened `MustacheStore` contract specification (with explicit ordering / concurrency semantics) before any other migration proceeds. Flag the epic as needing a contract-definition phase.

## Time budget

Two focused hours after STG-S01 lands. If exceeded, stop and report.

## Out of scope

- Any other activation.
- Property-based or fuzz tests — the contract is validated by the existing assertion set.
- Persistence across process restarts (by definition, the in-memory backend is ephemeral — tests that encode restart semantics would need a different fixture strategy, but Mustache's current tests don't).
- Concurrency stress — single-threaded tests are sufficient for the spike.

## Completion

Spike delivers:

- A single commit (or branch tip) with `InMemoryMustacheStore`, updated/duplicated tests, and all tests green against both backends.
- Pass/fail result per test × backend (eight cells).
- Time spent.
- A one-paragraph report on the test-parameterization approach chosen (duplicate vs harness), any contract gaps uncovered, and recommendations for STG-2.

Report lands in STG-2's Context section. Spike branch is NOT merged — STG-2 consumes the finding.
