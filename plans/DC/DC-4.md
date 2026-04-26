---
id: DC-4
title: "Decouple Orcha from concrete ClaudeCode and Model via ClaudeCodeClient"
status: Pending
type: implementation
blocked_by: [DC-2]
unlocks: [DC-7]
severity: High
target_repo: plexus-substrate
---

## Problem

Orcha imports the concrete `ClaudeCode` activation struct and its internal `Model` enum directly. This makes Orcha un-compilable without ClaudeCode compiled in, and it makes Orcha brittle to ClaudeCode's internal shape — any change to `ClaudeCode`'s constructor, its `ChatEvent` event type, its `CreateResult` return type, or the `Model` enum variants ripples into three Orcha files.

Orcha should call ClaudeCode through a curated library surface — a client handle that exposes just the operations Orcha uses (start a chat stream, consume chat events, create a resource) — not by reaching for the activation's concrete struct and its internal types.

## Context

**The specific coupling (re-verify against HEAD; audit drift caveat applies):**

- `src/activations/orcha/activation.rs:8` — `use crate::activations::claudecode::{ClaudeCode, Model};`
- `src/activations/orcha/orchestrator.rs:5` — `use crate::activations::claudecode::{ChatEvent, ClaudeCode, Model};`
- `src/activations/orcha/graph_runner.rs:2` — `use crate::activations::claudecode::{ChatEvent, ClaudeCode, CreateResult, Model};`

Orcha reaches for four ClaudeCode items: the activation struct (`ClaudeCode`), the chat-event enum (`ChatEvent`), the create-result type (`CreateResult`), and the model enum (`Model`).

- `ClaudeCode` — concrete activation struct. Used to dispatch library-level calls. Library API alternative: a client handle, `ClaudeCodeClient`, obtained from the activation struct itself.
- `ChatEvent` — domain type (chat stream event). This is a legitimate library-API member — callers need it to pattern-match on stream output. Remains re-exported, just from a narrower surface.
- `CreateResult` — domain type. Same deal — library-API member.
- `Model` — enum over concrete model IDs. Tension: is `Model` a ClaudeCode-internal concept (a ClaudeCode API enum) or a substrate-wide concept? Per README's pinned cross-epic contracts, ST introduces `ModelId: String` as a cross-activation newtype. **Decision for DC-4:** `Model` stays a ClaudeCode-internal enum; ClaudeCodeClient methods that take a model take a `ModelId` (string-newtyped) parameter and ClaudeCode converts internally. If ST hasn't shipped `ModelId` yet, DC-4 uses bare `String` and notes it as a ST-epic migration target.

**What Orcha actually does with ClaudeCode.** Orcha's runner constructs ClaudeCode chat sessions, consumes `ChatEvent` streams, submits `Create*` operations and inspects `CreateResult`. All library-level operations. `ClaudeCodeClient` exposes these as methods returning domain types.

**Library-API shape pinned by DC-2.** ClaudeCode's entry point re-exports: activation struct (for constructor-use by `builder.rs`), constructor, `ClaudeCodeError`, `ChatEvent`, `CreateResult`, `Model` (pending the ST-related decision above), and `ClaudeCodeClient` (new). It does NOT re-export `sessions.rs` internals, `render.rs` internals, or `ClaudeCodeStorage`.

## Required behavior

**ClaudeCode side:**

| Operation | Current shape | New shape (via ClaudeCodeClient) |
|---|---|---|
| Create a chat stream | Direct method on `ClaudeCode` struct | `client.create_chat(session_id, model, ...)` returning a stream of `ChatEvent` |
| Consume chat events | Direct enum pattern-match | Same — `ChatEvent` remains a library-API domain type |
| Submit create operation | Direct method on `ClaudeCode` | `client.create(...)` returning `CreateResult` |
| Model selection | `Model` enum param | `ModelId` (String newtype) or `Model` — see Risks |

Orcha code migrates from `claude_code.<method>(...)` to `claude_code_client.<method>(...)`. The client is a cheap Clone handle (wraps `Arc` internally) obtained once when Orcha is constructed and stored on the runner structs.

**Orcha side:**

| Before | After |
|---|---|
| `use crate::activations::claudecode::{ClaudeCode, Model};` | `use crate::activations::claudecode::{ClaudeCodeClient, ChatEvent, CreateResult};` (Model either still imported, or gone — see Risks) |
| `claude_code: Arc<ClaudeCode>` field | `claude_code: ClaudeCodeClient` (or `Arc<ClaudeCodeClient>`) |
| `self.claude_code.create_chat(...)` | `self.claude_code.create_chat(...)` — identical call shape, different underlying type |

**ClaudeCode's `mod.rs` after DC-4:**
- Re-exports `ClaudeCodeClient` (new).
- Keeps re-exports of `ChatEvent`, `CreateResult`, `ClaudeCodeError`.
- Model handling depends on ST's status — see Risks.
- Concrete `ClaudeCode` struct remains re-exported (needed by `builder.rs`), but Orcha imports `ClaudeCodeClient` exclusively.

## Risks

- **`Model` enum tension.** If DC-4 lands before ST's `ModelId` newtype lands, Orcha either (a) keeps importing `Model` directly during a short transition, or (b) switches to bare `String`. Option (a) is a smaller diff but preserves the import. Option (b) removes the import now but risks string typos. **Decision:** DC-4 picks (a) — keep `Model` as a ClaudeCode library-API re-export for the transition. ST epic's per-activation migration ticket replaces it with `ModelId` later. The `use` line becomes `use crate::activations::claudecode::{ClaudeCodeClient, ChatEvent, CreateResult, Model};` — still in Orcha but narrower. If this choice doesn't feel right to the implementor after seeing the code, escalate to a pinning conversation.
- **Stream lifetime.** `ChatEvent` streams are borrowed from ClaudeCode's session state. If `ClaudeCodeClient` is cheap-to-Clone (`Arc` inside), the stream lifetime is tied to the client handle not to a specific struct. Verify at implementation time that stream types don't leak lifetime parameters tying them to ClaudeCode's internals.
- **File collision with DC-3.** Both tickets touch `orcha/graph_runner.rs`, `orcha/activation.rs`. Cannot land in parallel. Pin order: DC-3 first, DC-4 second. DC-4's implementor re-verifies HEAD after DC-3 lands and adjusts file-line anchors.
- **File collision with DC-6.** DC-6 touches `orcha/graph_runner.rs`, `orcha/graph_runtime.rs`, `orcha/context.rs`. If DC-4 and DC-6 execute concurrently, they collide at commit time. Pin order: DC-4 before DC-6.

## What must NOT change

- ClaudeCode's wire-level RPC methods — request/response shapes identical.
- Orcha's graph-execution semantics — chat streams start, yield events, complete the same way.
- Orcha's wire API — `#[plexus_macros::method]` surface unchanged.
- `ChatEvent`, `CreateResult` shapes — these are library-API domain types; DC-4 does not repackage them.
- ClaudeCode's SQLite schema, storage layout.

## Acceptance criteria

1. `grep -rn "use crate::activations::claudecode::{.*ClaudeCode" src/activations/orcha/` returns zero results. Orcha no longer imports the concrete `ClaudeCode` activation struct.
2. `grep -rn "Arc<ClaudeCode>" src/activations/orcha/` returns zero results.
3. Orcha's runner/activation/orchestrator structs hold `ClaudeCodeClient` (or `Arc<ClaudeCodeClient>`) in place of `Arc<ClaudeCode>`.
4. ClaudeCode's `mod.rs` exposes `ClaudeCodeClient` as a public re-export with a library-API doc comment.
5. `cargo test --workspace` passes with zero test failures.
6. Orcha's chat-stream integration behavior (stream begins, yields events in order, completes with `CreateResult`) is unchanged — verified by whichever Orcha test currently covers this, re-run and green.
7. If `Model` transitions to `ModelId`: `grep -rn "use crate::activations::claudecode::Model" src/activations/orcha/` returns zero. If not: documented in the commit message as a deliberate transitional choice, per Risks.

## Completion

Implementor delivers:

- Commit introducing `ClaudeCodeClient` in `claudecode/`, with the method set matched to Orcha's actual call sites.
- Commit migrating Orcha's runner/activation/orchestrator to use `ClaudeCodeClient`.
- `cargo test` output showing green.
- Before/after `grep` output for the import-leak criteria.
- Commit message notes the `Model` enum treatment chosen (transitional import vs `ModelId` swap).
- Status flip to `Complete` in the commit that lands the work.
