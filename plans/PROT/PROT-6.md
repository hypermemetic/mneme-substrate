---
id: PROT-6
title: "synapse 4.0.0: remove MethodSchema content-type branch; PluginSchema is sole schema response"
status: Pending
type: implementation
blocked_by: [PROT-5]
unlocks: [PROT-10]
severity: Critical
target_repo: synapse
---

## Problem

synapse's response parser filters by `".schema" `T.isSuffixOf` ct` (in `plexus-protocol/Transport.hs` and synapse's own navigation code). When the old wire protocol emitted `method_schema` content_type for leaf methods, the filter rejected it and reported "No schema in response".

PROT-5 removes the `SchemaResult.Method` variant. Every `.schema` response is now a `PluginSchema` with content_type ending in `.schema`. Synapse's filter accepts it uniformly.

This ticket removes any synapse code that special-cases `method_schema` content_type, simplifies the parser, and bumps synapse to 4.0.0 (major — wire-compat-breaking change in how .schema responses are interpreted).

## Context

- Repo: `/Users/shmendez/dev/controlflow/hypermemetic/synapse/`
- Version: 3.13.0 → 4.0.0 (breaking wire compat on `.schema` responses).
- Files to edit (grep-determined before editing):
  - `src/Synapse/Schema/Types.hs` — any `SchemaResult` pattern-match.
  - `src/Synapse/Schema/` subdir — parser code.
  - `src/Synapse/Transport.hs` — navigator may need tweaks.
  - `plexus-synapse.cabal` — version bump.

## Required behavior

1. **Remove** any parser branch that decodes `method_schema` content_type. Post-PROT, only `PluginSchema` flows.

2. **Simplify** the schema-navigation logic: every `fetchSchemaAt path` returns a `PluginSchema`. No ADT case discrimination.

3. **Update tree rendering** (`Synapse.Algebra.Render` etc.): nodes reached via drill-down return their own PluginSchema, which already contains `methods` + `children` + `is_hub`. No special rendering for "this was a method-level schema" — leaf methods surface as single-method PluginSchema and render consistently.

4. **Rebuild deprecation-test** suite: no fact changes, but the schema ADT shift may ripple into test fixtures. Verify all 29 deprecation-test specs pass. Other suites (cli-test, ir-test, typeref-json, parse-test, bidir-test, path-normalization-test, stream-tracker-test, ir12-method-role-test, synapse-types-test) all pass.

5. **Pre-existing failures** (websocket-raw-test, bidir-integration-test) — stashed-change comparison documents these are unrelated; they remain failing identically pre- and post-PROT.

6. **Version bump** synapse: 3.13.0 → 4.0.0 in `plexus-synapse.cabal`.

7. **Tag** `plexus-synapse-v4.0.0` locally.

8. **Reinstall CLI binary**: `cabal install exe:synapse --overwrite-policy=always`. Verify `synapse --version` reports `4.0.0`.

## Risks

| Risk | Mitigation |
|---|---|
| Synapse's parser code is spread across multiple modules; some may have implicit dependency on the old ADT shape. | Grep wide before editing. Migrate in one cohesive commit. |
| The reinstall step requires cabal / GHC to be in PATH and functional. | Verify before starting the ticket. If cabal build fails for environmental reasons, the ticket is blocked, not broken. |
| IR-15's invocation-time deprecation warnings use the same RPC path. Test that `--no-deprecation-warnings` still works. | deprecation-test suite covers this; verify all 29 pass post-migration. |
| Existing synapse binary at `~/.local/bin/synapse` (v3.10.1) is invoked by other tools during this transition. | Reinstall to 4.0.0 updates the symlink. Any running session using the old binary keeps working (they've already loaded schemas) but new invocations get the new parser. |
| Tree rendering shifts visually (e.g., leaf method as single-method PluginSchema looks different). | Pre/post render diff on a representative activation (hyperforge.build, substrate.solar.mercury). Commit body documents any rendering change. |

## What must NOT change

- CLI grammar: `synapse <backend> <path...>` args — unchanged.
- JWT auth (`--token`, `--token-file`, `~/.plexus/tokens/<backend>`) — unchanged.
- Template generation, IR emission, cache behavior — unchanged.
- Invocation-time deprecation warnings (IR-15) — unchanged.
- Exit codes for any command — unchanged.

## Acceptance criteria

1. `cabal build` green.
2. `cabal test` all suites green except pre-existing failures (websocket-raw-test, bidir-integration-test — which are confirmed pre-existing via stash comparison).
3. `cabal install exe:synapse --overwrite-policy=always` succeeds. `synapse --version` reports `4.0.0`.
4. `grep -rn 'SchemaResult.*Method\|method_schema' synapse/src/` returns zero results.
5. `plexus-synapse.cabal` version is `4.0.0`. Tag `plexus-synapse-v4.0.0` exists locally.
6. End-to-end check (after PROT-7, PROT-8 rebuild too): `synapse lforge hyperforge build` renders BuildHub's schema tree successfully. Validated in PROT-10.

## Completion

PR against synapse. Status flipped to Complete once PROT-10's e2e verification passes.
