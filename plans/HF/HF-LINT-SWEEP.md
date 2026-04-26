---
id: HF-LINT-SWEEP
title: "hyperforge: re-enable opted-out clippy pedantic/nursery lints one at a time"
status: Pending
type: implementation
blocked_by: [HF-CLEAN]
unlocks: []
severity: Low
target_repo: hyperforge
---

## Problem

HF-CLEAN adopted `clippy::pedantic` + `clippy::nursery` at `warn` level, but needed to opt out several lints that produced noise-to-signal too low for a single-pass sweep. Each opt-out has a one-line justification in `Cargo.toml`. This ticket re-enables them one at a time as churn capacity allows.

Opted-out clippy lints (as of hyperforge 4.1.2):

| Lint | Rationale for opt-out | Notes |
|------|-----------------------|-------|
| `module_name_repetitions` | Common in domain modules; renaming adds churn without clarity | Probably leave allowed indefinitely |
| `missing_errors_doc` | Opt-in documentation, not correctness | Leave allowed |
| `missing_panics_doc` | Same | Leave allowed |
| `must_use_candidate` | Signal-to-noise too low | Probably leave allowed |
| `too_many_lines` | Subjective | Leave allowed |
| `unused_async` | Fires on macro-generated hub-method signatures | Fix upstream in plexus-macros |
| `too_many_arguments` | Hub methods pass Plexus RPC params 1:1 | HF-IR reshape changes this |
| `struct_excessive_bools` | HyperforgeEvent variants | HF-IR reshape changes this |
| `match_same_arms` + `ignored_unit_patterns` | Idiomatic `Ok(())` patterns | Leave allowed |
| `significant_drop_tightening` | Nursery, brittle | Revisit |
| `assigning_clones` | Nursery, churn | Revisit |
| `option_if_let_else` | Nursery, style | Revisit |
| `or_fun_call` | Stylistic | Revisit |
| `redundant_pub_crate` | Stylistic | Revisit |
| `use_self` | Stylistic | Revisit |
| `needless_pass_by_value` | API stability churn | Revisit post-HF-IR |
| `ref_option` | `&Option<T>` refactor churn | Revisit |
| `case_sensitive_file_extension_comparisons` | Valid concern but narrow impact | Worth a targeted sweep |
| `similar_names` | Aesthetic | Revisit |
| `cast_possible_truncation` | Real concerns in narrow places | Worth per-site narrow allows with justification |
| `cast_precision_loss` | Same | Worth per-site narrow allows |
| `format_push_string` | Pedantic | Probably leave allowed |
| `return_self_not_must_use` | Pedantic; noisy on builders | Probably leave allowed |
| `manual_let_else` | Pedantic, `yield Error; return;` pattern handles poorly | Probably leave allowed |
| `unused_crate_dependencies` (rust) | Multi-target structural FP | Leave allowed until Cargo supports target-specific deps |
| `let_underscore_drop` (rust) | Fire-and-forget I/O is idiomatic | Probably leave allowed |

## Required behavior

Pick ONE opted-out lint per PR. For each:

1. Remove the `"allow"` entry from `Cargo.toml`.
2. Run `cargo clippy --all-targets -- -D warnings`.
3. Fix every site the lint flags. Prefer a real fix; narrow per-site `#[allow]` with justification only when correctness/readability would suffer.
4. `cargo test` still passes.
5. Commit with body listing every changed site.
6. Bump hyperforge patch version.

Never remove more than one lint in a single PR — the point of a sweep is to keep the diff reviewable.

## Acceptance criteria (per PR)

1. One lint transitioned from `"allow"` to `"warn"`.
2. `cargo clippy --all-targets -- -D warnings` exits 0.
3. `cargo test` exits 0.
4. Patch version bumped.
5. Local annotated tag, not pushed (same discipline as HF-CLEAN).

## Notes

Completely parallel with HF-DC/HF-IR/HF-TT/HF-CTX — this ticket only touches lint posture and fixes to meet it, not API surface.
