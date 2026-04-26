---
id: PROT-5
title: "plexus-protocol 0.6.0.0 (Haskell): remove SchemaResult Method variant; PluginSchema is the sole .schema response"
status: Pending
type: implementation
blocked_by: [PROT-2]
unlocks: [PROT-6]
severity: Critical
target_repo: plexus-protocol
---

## Problem

plexus-protocol (Haskell) mirrors plexus-core's wire types. Its `SchemaResult` ADT has a `Method` constructor that PROT-2 removes from the Rust side. The Haskell decoder must match — otherwise synapse decodes phantom variants or chokes on unexpected wire data.

## Context

- Repo: `/Users/shmendez/dev/controlflow/hypermemetic/plexus-protocol/`
- Version: 0.5.0.0 → 0.6.0.0 (breaking ADT change).
- Files to edit:
  - `src/Plexus/Types.hs` (or wherever `SchemaResult` is defined — grep).
  - `src/Plexus/Transport.hs` — the `fetchSchemaAt` content_type filter already accepts suffix `.schema`; should need no change but verify.
  - `plexus-protocol.cabal` — version bump.

## Required behavior

1. **Remove** the `Method` constructor from `SchemaResult`. Either:
   - Flatten to `PluginSchema` directly (the ADT becomes a type alias or disappears).
   - Keep `SchemaResult` as a newtype wrapper over `PluginSchema` if downstream pattern-matching prefers it.
   - Pin which in the commit body. Match PROT-2's decision.

2. **Update the aeson `FromJSON`/`ToJSON`** instances for `SchemaResult` (or whatever replaces it) to handle the unified wire shape: always a `PluginSchema` JSON object, no tagged variant.

3. **Update `cookieHeader`** — wait, no, that's unrelated. `cookieHeader` was added for JWT work; keep it. Just checking — is there any other auth/token interaction that changes here? No.

4. **Version bump** plexus-protocol: 0.5.0.0 → 0.6.0.0 in `*.cabal`.

5. **Tag** `plexus-protocol-v0.6.0.0` locally.

## Risks

| Risk | Mitigation |
|---|---|
| Synapse depends on `SchemaResult` pattern matching. PROT-6 handles; flag here for coordination. | Pre-commit grep of synapse source for `SchemaResult.Method` or similar. Ensure PROT-6 covers every match. |
| Other Haskell consumers of plexus-protocol exist outside synapse. | Survey: `grep -rn 'plexus-protocol' ~/dev/controlflow/hypermemetic/ --include='*.cabal'`. Currently synapse is the only known consumer. Flag any discoveries. |
| `aeson` derivation generics may produce a different wire shape than expected when the ADT collapses. | Write a round-trip test in the cabal file's test-suite: serialize a known PluginSchema, deserialize, assert equality. |
| Orphan instance issues if `PluginSchema` aeson instances move between modules. | Keep instances where they are unless relocation is required. |

## What must NOT change

- `PluginSchema`, `MethodSchema`, `ParamSchema`, `ChildSummary` Haskell types — their field layouts are unchanged.
- Auth token threading via `SubstrateConfig.substrateHeaders` (the JWT work) — unchanged.
- `fetchSchemaAt` / `fetchSchema` function signatures — unchanged.

## Acceptance criteria

1. `cabal build` in plexus-protocol green.
2. `cabal test` green (any existing test suites).
3. `plexus-protocol.cabal` version is `0.6.0.0`. Tag `plexus-protocol-v0.6.0.0` exists locally.
4. Round-trip serialization test: encoding + decoding a PluginSchema through the new `SchemaResult` (or its replacement) produces byte-identical output to the encoding-only variant.
5. `grep -rn 'SchemaResult.*Method' plexus-protocol/src/` returns zero results.

## Completion

PR against plexus-protocol. Status flipped to Complete when PROT-6 (synapse) builds against this.
