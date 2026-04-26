---
id: PROT-S03
title: "Spike: verify PROT-3's child-first dispatch works correctly with dynamic child gates (#[child(list = \"...\")])"
status: Pending
type: spike
blocked_by: []
unlocks: [PROT-3]
severity: High
target_repo: plexus-macros
---

## Problem

PROT-3's proposed fix rewrites the `.schema` dispatch to `self.get_child(child_name).await` first. Static children are matched by exact name (e.g., `build`, `workspace`). But **dynamic children** (like IR-18's `ClaudeCode.session(<id>)` or IR-19's `Cone.of(<uuid>)`) resolve at runtime via a user-provided `fn(name: &str) -> Option<Child>`.

If synapse sends `lforge.call {method: "claudecode.session.<uuid>.schema"}`, the dispatch path is:
1. plexus.route("claudecode.session.<uuid>.schema") → ClaudeCode.call("session.<uuid>.schema")
2. ClaudeCode.call: no local match → strip_suffix(".schema") → "session.<uuid>"
3. New PROT-3 logic: self.get_child("session.<uuid>").await

But `get_child` for ClaudeCode is generated to look for exact static-child matches first, THEN falls through to dynamic — passing `"session.<uuid>"` as-is. It wouldn't split on `.` to find `"session"` as a dynamic-child function. So it'd fail to route.

PROT-S03 verifies whether the dispatch correctly handles `<dynamic_child>.<id>.schema` patterns.

## Context

The current macro `get_child_body` (from `src/codegen/activation.rs:312+`):

```rust
match name {
    "static_child_1" => Some(Box::new(self.static_child_1())),
    "static_child_2" => Some(Box::new(self.static_child_2())),
    _ => self.dynamic_child(_name).await.map(...)
}
```

So `get_child("session.abc123")` calls `self.session("session.abc123").await` — which is the dynamic function expecting just `"abc123"`, not `"session.abc123"`. Wrong.

The proper routing for `claudecode.session.<id>.schema` should:
1. Split `"session.<id>.schema"` into `("session", "<id>.schema")`.
2. Call `self.session(<id>).await` → `Option<SessionActivation>`.
3. On Some: route `"schema"` into the SessionActivation → returns its PluginSchema.

But PROT-3's proposed simple `self.get_child(child_name)` doesn't do the split. We need either:
(a) `route_to_child` handling multi-level paths before returning the "leaf" to get_child.
(b) The macro's `get_child` splitting the name itself.
(c) A different dispatch strategy for paths containing dots.

## Required behavior

1. **Trace** the current (broken) dispatch for a dynamic-child path like `claudecode.session.<uuid>.schema`:
   - What does route_to_child do with "session.<uuid>.schema"?
   - Where does it fail?

2. **Prototype** the PROT-3 fix on a substrate activation with dynamic children (ClaudeCode, Cone). Run `cargo test -p plexus-substrate` + wire-test via raw RPC.

3. **Decide**: does PROT-3's fix as written cover dynamic children, or does it need augmentation? Options:
   - Extend PROT-3 to split the path in `get_child` dispatch: `get_child_body` checks `name.split_once('.')` before dispatching.
   - Extend `route_to_child` in plexus-core to do deeper path walking.
   - Keep PROT-3 narrow (only static child schema fetches work) and file a follow-up ticket for dynamic.

4. **Update PROT-3** with the ratified decision and corresponding code sketch.

## Risks

| Risk | Mitigation |
|---|---|
| The dispatch is deeply tangled; the fix might require changes to plexus-core's `route_to_child` AND the macro's `get_child` AND the dispatch body. | Prototype first; understand the surface before promoting PROT-3. |
| Dynamic-child schema fetches have latent bugs beyond the strip-suffix issue. | Spike would surface them; either widen PROT-3's scope or file follow-ups. |
| Static-only works, dynamic-only requires more. | Acceptable if PROT-3 ships the static fix and dynamic gets a follow-up ticket within the same epic. |

## Acceptance criteria

1. Wire-trace of the buggy dispatch for a representative dynamic-child path.
2. Prototype on substrate (ClaudeCode or Cone) demonstrating the fix works — or documenting additional work needed.
3. PROT-3's ticket text updated with the final dispatch code sketch covering both static and dynamic cases (or a clear "static-only; dynamic is PROT-3b").

## Completion

Spike concludes with PROT-3's scope confirmed or widened. If dynamic requires a separate ticket, file PROT-3b alongside this spike's completion.
