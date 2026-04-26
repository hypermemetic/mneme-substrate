---
id: HF-CTX-3
title: "Ticket CRUD activation methods + scope-frontmatter parsing"
status: Pending
type: implementation
blocked_by: [HF-CTX-2]
unlocks: [HF-CTX-7]
severity: High
target_repo: hyperforge
---

## Problem

With `Ticket`, `Epic`, `TicketScope`, and `TicketStore` pinned by HF-CTX-2, hyperforge still exposes no callable ticket surface. This ticket introduces the ticket CRUD methods on the `hyperforge.ctx` activation — `create_ticket`, `get_ticket`, `update_body`, `update_status`, `delete_ticket`, `create_epic`, `get_epic` — plus the frontmatter parser that turns the ticket's YAML scope block into a `TicketScope`. `update_status` enforces the valid state-machine transitions but does not enforce the human promotion gate; HF-CTX-8 adds that.

## Context

Target repo: `hyperforge` at `/Users/shmendez/dev/controlflow/hypermemetic/hyperforge/`.

New file introduced by this ticket: `src/ctx/activation.rs` (or whatever sibling-directory HF-CTX-S01's layout pinned — match the existing hub layout under `src/hubs/`).

Namespace: `hyperforge.ctx` (pinned in HF-CTX-S01; HF-CTX-1 assumes this). The activation wraps a `Box<dyn TicketStore>` from HF-CTX-2.

`update_status` enforces the state-machine table; it does **not** enforce the human gate. HF-CTX-8 wraps the `Pending → Ready` transition behind auth. Agent-driven transitions (e.g., Orcha's `Ready → Complete`) flow through `update_status` unchanged.

Scope frontmatter: ticket YAML headers under the `scope:` key map directly to the `TicketScope` struct fields pinned in HF-CTX-2. Parsing is done via `serde_yaml` against the frontmatter block. Tickets without a `scope:` block default to `TicketScope::default()` (empty vectors / empty hashmaps).

File-boundary discipline: this ticket owns exactly one file (`src/ctx/activation.rs`). HF-CTX-4 (fact emission) touches the existing hub files, not this one. HF-CTX-5 (query methods) lives in its own file. HF-CTX-6 (watch) lives in its own file. The four tickets run in parallel.

## Required behavior

### RPC methods on `hyperforge.ctx`

| Method | Args | Return | Behavior |
|---|---|---|---|
| `create_ticket` | `ticket: Ticket` | `CtxCreateResult` | Inserts via `TicketStore::create_ticket`. Returns `ok { id }`, `already_exists { id }`, or `err { message }`. Rejects `Ticket { status: Ready, .. }` — new tickets always land as `Pending` regardless of submitted value. Parses the `scope` field if present in the `Ticket`'s body frontmatter. |
| `get_ticket` | `id: TicketId` | `CtxGetTicketResult` | Returns `ok { ticket }`, `not_found { id }`, or `err { message }`. |
| `update_body` | `id: TicketId, body: String` | `CtxUpdateResult` | Replaces body, re-parses frontmatter for `scope`, calls `update_ticket_scope` if changed. Last-write-wins. Returns `ok { id }` or an error. |
| `update_status` | `id: TicketId, status: Status` | `CtxUpdateResult` | Transitions status using the table below. Does **not** enforce the human gate (HF-CTX-8 wraps the `Pending → Ready` path). Returns `ok { id }`, `invalid_transition { from, to }`, `requires_promote { id }` (for the `Pending → Ready` case), or an error. |
| `delete_ticket` | `id: TicketId` | `CtxDeleteResult` | Removes via store. Returns `ok { id }`, `referenced { id, by }`, `not_found { id }`, or `err`. |
| `create_epic` | `epic: Epic` | `CtxCreateResult` | Inserts via `TicketStore::create_epic`. Idempotent on re-create of an identical epic. |
| `get_epic` | `prefix: String` | `CtxGetEpicResult` | Returns `ok { epic }` or `not_found { prefix }`. |

### Valid status transitions

HF-CTX-3 enforces the state machine at the RPC layer. `update_status` accepts:

| From | Allowed transitions to |
|---|---|
| `Pending` | `Ready` *(returns `requires_promote`)*, `Blocked`, `Idea`, `Superseded` |
| `Ready` | `Blocked`, `Complete`, `Superseded` |
| `Blocked` | `Ready`, `Superseded` |
| `Idea` | `Pending`, `Superseded` |
| `Complete` | (terminal; no outgoing) |
| `Superseded` | (terminal; no outgoing) |
| `Epic` | (no transitions) |

Any other transition returns `invalid_transition { from, to }`.

The `Pending → Ready` transition is state-machine-valid but routing returns `requires_promote { id }` without mutating state, directing the caller to HF-CTX-8's gated method. All other transitions are state-machine-enforced only.

### Result types

Tagged enums (`#[serde(tag = "type", rename_all = "snake_case")]`): `CtxCreateResult`, `CtxGetTicketResult`, `CtxUpdateResult`, `CtxDeleteResult`, `CtxGetEpicResult`. Variants are the `ok { ... }` success case, the specific failure modes above, and a catch-all `err { message: String }`.

### Scope frontmatter parsing

When `create_ticket` or `update_body` is called, the implementation:

1. Reads the `body` field.
2. Detects an opening `---\n` + closing `\n---\n` block at the top (standard YAML frontmatter).
3. Parses the block with `serde_yaml`.
4. Extracts the `scope:` key (if present) and deserializes to `TicketScope`.
5. Stores the parsed `TicketScope` on the ticket record via `update_ticket_scope`.
6. If the `scope:` key is missing or empty, stores `TicketScope::default()`.
7. If the YAML fails to parse, logs a warning and stores `TicketScope::default()` — the ticket is still writable; scope defaults are not fatal.

Example scope block (from HF-CTX-1):

```yaml
scope:
  repos: [hyperforge, plexus-substrate]
  packages:
    - { ecosystem: cargo, package: hub-core }
  introduces: [hyperforge::ctx::TicketStore]
  versions_before: { hub-core: "0.4.0" }
  versions_after: { hub-core: "0.5.0" }
```

### Activation registration

The `hyperforge.ctx` activation is registered with the hyperforge hub registry the same way every existing hub is (pattern from `src/hubs/build/`, `src/hubs/repo.rs`, etc.). The activation is constructed with `SqliteTicketStore` as its default backend.

### Module wiring

- `src/ctx/activation.rs` — this ticket's one file.
- `src/ctx/mod.rs` — re-exports.
- `src/hubs/mod.rs` or the hub registry entry point — adds `pub mod ctx;` (or equivalent) and an instantiation.

## Risks

| Risk | Mitigation |
|---|---|
| Status transition table conflicts with the HF-CTX epic's real workflow. | Transitions are pinned in this ticket; changes require a new ticket. |
| `create_ticket` with `status: Ready` would bypass HF-CTX-8's gate. | This ticket rejects `status: Ready` on create — new tickets always land `Pending`. HF-CTX-8 adds the auth gate on `Pending → Ready`. |
| Malformed scope YAML in a ticket body halts the insert. | Parse failures are warnings, not errors; scope defaults to empty. The ticket still writes. |
| Frontmatter has a `scope:` key with types that disagree with `TicketScope` shape. | Parse fails; warning logged; scope defaults to empty. Ticket writes successfully. |

## What must NOT change

- HF-CTX-2's `TicketStore` trait surface. This ticket consumes it, does not modify it.
- Every other hyperforge hub's compile and test behavior.
- The hub registration pattern — follows the existing convention exactly.
- Synapse's ability to introspect other hyperforge activations.

## Acceptance criteria

1. `cargo build --workspace` succeeds in hyperforge.
2. `cargo test --workspace` succeeds. New tests cover:

   | Scenario | Expected |
   |---|---|
   | `create_ticket` with a valid `Pending` ticket | `ok { id }`; `get_ticket` returns identical ticket. |
   | `create_ticket` with `status: Ready` | Persisted as `Pending`. |
   | `create_ticket` twice with the same id | Second call returns `already_exists { id }`. |
   | `get_ticket` on absent id | `not_found { id }`. |
   | `update_body` round-trip | `ok { id }`; `get_ticket` returns new body; `updated_at` increased. |
   | `update_body` with a ticket body containing a valid `scope:` block | `get_ticket` returns a populated `TicketScope`. |
   | `update_body` with a ticket body containing malformed YAML in the scope | `ok { id }`; `scope == TicketScope::default()`; warning logged. |
   | `update_status` `Pending → Ready` | Returns `requires_promote { id }`; ticket still `Pending`. |
   | `update_status` `Ready → Complete` | `ok { id }`. |
   | `update_status` `Complete → Ready` | `invalid_transition { from: Complete, to: Ready }`. |
   | `update_status` `Pending → Complete` | `invalid_transition { from: Pending, to: Complete }`. |
   | `delete_ticket` on a leaf | `ok { id }`. |
   | `delete_ticket` on a referenced ticket | `referenced { id, by }` listing referrers. |
   | `create_epic` + `get_epic` | Round-trip matches. |

3. `synapse hyperforge ctx` surfaces all seven methods with parameter names and types.
4. The activation appears in hyperforge's hub registry listing.
5. `cargo build --workspace` + `cargo test --workspace` pass green end-to-end (rule 12 integration gate).

## Completion

- Commit adds `src/ctx/activation.rs` and updates the hub registry.
- Commit message includes `cargo build --workspace` + `cargo test --workspace` output — both green, plus `synapse hyperforge ctx` output showing the seven methods.
- If this ticket is the first HF-CTX surface to land, it rides on HF-CTX-2's version bump (`hyperforge-v4.3.0`). If HF-CTX-2 already landed, this is a patch bump within 4.3.x or no version change if the surface was pinned by HF-CTX-2 (decide based on public-vs-activation-internal split at commit time).
- Ticket status flipped from `Ready` → `Complete` in the same commit as the code.
