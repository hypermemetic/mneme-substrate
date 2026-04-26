---
id: HF-TESTS
title: "hyperforge: restore commented-out tests (MockAuthProvider + AuthHub stream tests)"
status: Pending
type: implementation
blocked_by: [HF-CLEAN]
unlocks: []
severity: Low
target_repo: hyperforge
---

## Problem

Several tests in hyperforge are currently disabled (block comments in the test module) because they depended on constructor signatures or stream semantics that changed during earlier refactors:

1. **`src/adapters/codeberg.rs` / `github.rs` / `gitlab.rs`** — three blocks labelled `/* Broken: XxxAdapter::new requires 2 arguments, not 1 */` — test coverage for `auth_headers_missing_token` and `auth_headers_with_token` against the mock auth provider.
2. **`src/auth_hub/mod.rs`** — three blocks labelled `/* Commented out: stream does not implement Unpin, cannot use .next().await directly */` — tests for `set_secret`, `list_secrets`, `delete_secret`.

HF-CLEAN added `#[allow(dead_code)]` to the `MockAuthProvider` structs and `create_test_hub` helper so the supporting scaffolding doesn't break the zero-warnings gate, with TODO references back to this ticket.

## Required behavior

1. For each adapter's commented-out test: update the constructor calls to the current 2-argument signature (`XxxAdapter::new(auth, org)`), uncomment the tests, confirm they pass.
2. For `auth_hub` tests: use `tokio::pin!` or `Box::pin` on the stream before calling `.next().await`, or restructure with `stream.next().await` inside a pinned helper. Uncomment the tests, confirm they pass.
3. Once restored, remove the `#[allow(dead_code)]` attributes and their justification comments from `MockAuthProvider` definitions and `create_test_hub`.
4. `cargo test` passes with all previously-commented tests running.

## Acceptance criteria

1. Zero `/* Commented out: */` or `/* Broken: */` blocks remain in `src/adapters/*.rs` or `src/auth_hub/**`.
2. No `#[allow(dead_code)]` on `MockAuthProvider` / `create_test_hub`.
3. `cargo test` exits 0 with strictly more passing tests than pre-HF-TESTS.
4. `cargo clippy --all-targets -- -D warnings` still exits 0.
5. Bump hyperforge patch version.

## Notes

Smallest-possible follow-up. Completely independent of HF-IR/HF-DC.
