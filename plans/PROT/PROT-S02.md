---
id: PROT-S02
title: "Spike: does the strip-suffix bug affect `.hash` and `_info` too, or only `.schema`?"
status: Pending
type: spike
blocked_by: []
unlocks: [PROT-3]
severity: High
target_repo: plexus-macros
---

## Problem

HF-AUDIT-3's manifestation is `synapse lforge hyperforge build` → "No schema in response". The root cause is the macro's `strip_suffix(".schema")` branch in `Activation::call` matching `#[child]` accessor names.

The macro also emits similar patterns for `.hash` and potentially `_info`. If these have the same shape — strip-suffix + find in `plugin_schema.methods` — they're also buggy. Possibly nobody noticed because `.hash` is rarely invoked directly; but a generic codegen consumer could hit it.

PROT-S02 audits the macro's dispatch for analogous bugs.

## Context

Target file: `/Users/shmendez/dev/controlflow/hypermemetic/plexus-macros/src/codegen/activation.rs`

The current `Activation::call` generated body looks roughly like:

```rust
match method {
    // user method arms
    "schema" => { /* return plugin_schema */ }
    _ => {
        if let Some(method_name) = method.strip_suffix(".schema") {
            // find in plugin_schema.methods, return SchemaResult::Method
        }
        // call_fallback — route_to_child or MethodNotFound
    }
}
```

Is there an analogous `strip_suffix(".hash")` or `strip_suffix("_info")` branch? If yes, the same bug exists:

- `hyperforge.build.hash` → strip_suffix(".hash") → "build" → find in methods → match child accessor → wrong response shape.
- `hyperforge.build._info` → similar if supported.

## Required behavior

1. **Read** `src/codegen/activation.rs` end-to-end. Find every `strip_suffix` or analogous pattern in the generated dispatch.

2. **Determine** for each introspection method (`.schema`, `.hash`, `_info`):
   - Is there a strip-suffix branch in Activation::call codegen?
   - Does it match against `plugin_schema.methods` (→ same bug) or does it route differently (→ possibly fine)?

3. **Test on the wire** against the running hyperforge binary:
   ```
   echo '{"jsonrpc":"2.0","id":1,"method":"lforge.call","params":{"method":"hyperforge.build.hash","params":{}}}' | websocat -t ws://127.0.0.1:44104/
   ```
   If it returns a sensible hash value, `.hash` works. If it returns an error or wrong content_type, same bug.

4. **Decide** PROT-3's scope: fix only `.schema` (if only `.schema` is buggy), or widen to `.hash` and `_info` (if they share the issue).

5. **Document** in PROT-3's "Required behavior" section. Update the code sketch to cover all affected branches.

## Risks

| Risk | Mitigation |
|---|---|
| `.hash` is buggy but silently — no consumer has reported because nobody fetches child-level hashes. | Confirmed by wire test. Fixing in PROT-3 is cheap if it shares the pattern. |
| The introspection methods have wildly different dispatch paths, none of them strip-suffix. | Good — PROT-3 scope stays minimal. |
| Fix for one breaks another subtly. | One cohesive rewrite of the dispatch `_` arm, tested against all three introspection methods. |

## Acceptance criteria

1. Enumerated list of introspection methods that use strip-suffix dispatch: `.schema`, `.hash`, `_info` — for each, yes/no.
2. Wire-test results for each against running hyperforge (post-HF-CLEAN, PID 18804 or later).
3. PROT-3's "Required behavior" updated to cover each buggy dispatch path in one cohesive rewrite.
4. Spike commit references the file:line of the buggy branches.

## Completion

Spike concludes with PROT-3's scope ratified. If additional introspection paths are affected, PROT-3's code sketch is updated in the same commit.
