---
id: ST-2
title: "Foundation: newtype module with all cross-boundary IDs"
status: Pending
type: implementation
blocked_by: []
unlocks: [ST-3, ST-4, ST-5, ST-6, ST-7, ST-8, ST-9]
severity: High
target_repo: plexus-substrate
---

## Problem

Substrate has 29 distinct domain concepts passed around as bare `String`, `Uuid`, `i64`, or `PathBuf`. Two parameters of the same raw type are swap-compatible silently: the compiler cannot catch a `StreamId` passed where a `SessionId` was expected, or a `claude_session_id` where a `loopback_session_id` was expected, or a `ToolUseId` where a `tool_name` was expected. Every downstream migration in this epic (ST-3 through ST-9) needs the newtypes defined in one canonical location before activations can be migrated in parallel.

## Context

Follow `skills/strong-typing/SKILL.md` exactly. This ticket defines the types; later tickets apply them.

Pinned names live in `plans/README.md` under "Pinned cross-epic contracts". Use those names literally.

Target location: a new module `src/types.rs` in `plexus-substrate`, exported as `crate::types` (and re-exported through `crate::prelude` if a prelude exists — currently none does; create one at `src/prelude.rs` if convenient, otherwise document the `crate::types` import path in the module rustdoc).

The strong-typing skill lists the required trait set:

| Trait | Required for |
|---|---|
| `Debug` | Logging, tracing, error messages |
| `Clone` | Value semantics |
| `PartialEq`, `Eq` | Comparison, dedup, `HashMap` keys |
| `Hash` | `HashMap` / `HashSet` keys |
| `Serialize`, `Deserialize` | JSON wire format |
| `#[serde(transparent)]` | Wire-format byte-identity with the wrapped type |
| `Display` | tracing fields, string interpolation |

Every newtype must also expose:

- `pub fn new(value: impl Into<Inner>) -> Self` — construction
- `pub fn as_str(&self) -> &str` for `String`-backed newtypes
- `pub fn inner(&self) -> &Inner` or equivalent accessor for the wrapped value

`ApprovalId` already exists in `claudecode_loopback/types.rs` as `pub type ApprovalId = Uuid;`. This ticket canonicalizes it into the shared module as `pub struct ApprovalId(pub Uuid);`. The Loopback activation's own use (ST-7) then imports from `crate::types` instead of redefining.

`arbor::ArborId`, `arbor::NodeId` (alias to `ArborId`), and `arbor::TreeId` are already newtyped and stay in their existing location. They are NOT moved into `crate::types` by this ticket. `lattice::NodeId` is a different concept (a string identifier for a lattice DAG node) and IS defined in `crate::types`.

## Required behavior

Define the following public types in `src/types.rs`:

| Name | Shape | Serde form |
|---|---|---|
| `SessionId` | newtype over `String` | bare JSON string |
| `GraphId` | newtype over `String` | bare JSON string |
| `NodeId` | newtype over `String` (lattice node id; distinct from `arbor::NodeId`) | bare JSON string |
| `StreamId` | newtype over `Uuid` | JSON string in UUID format |
| `ApprovalId` | newtype over `Uuid` | JSON string in UUID format |
| `ToolUseId` | newtype over `String` | bare JSON string |
| `TicketId` | newtype over `String` | bare JSON string |
| `WorkingDir` | newtype over `PathBuf` | JSON string (path form) |
| `ModelId` | newtype over `String` | bare JSON string |
| `TemplateId` | newtype over `String` | bare JSON string |

Plus a structured backend URL:

| Name | Shape | Serde form |
|---|---|---|
| `BackendUrl` | struct `{ protocol: BackendProtocol, host: String, port: u16 }` | NOT `#[serde(transparent)]` — this is a structured type; serializes as an object with those three fields. Also provides `BackendUrl::parse(&str)` and `Display` that emits the `protocol://host:port` flat string form for backward-compat logging. |
| `BackendProtocol` | enum `{ Ws, Wss }` with `#[serde(rename_all = "lowercase")]` | JSON string `"ws"` or `"wss"`. |

Behavior contracts:

| Call | Expected |
|---|---|
| `SessionId::new("abc")` | constructs a `SessionId` wrapping `"abc".to_string()` |
| `serde_json::to_string(&SessionId::new("abc")).unwrap()` | `"\"abc\""` (a JSON string, not an object) |
| `serde_json::from_str::<SessionId>("\"abc\"").unwrap()` | `SessionId::new("abc")` |
| `StreamId::new(uuid)` | constructs a `StreamId` wrapping the provided `Uuid` |
| `serde_json::to_string(&StreamId::new(uuid)).unwrap()` | `"\"<uuid-canonical-string>\""` |
| `format!("{}", SessionId::new("abc"))` | `"abc"` |
| `format!("{}", StreamId::new(uuid))` | canonical UUID string |
| `WorkingDir::new("/tmp")` | constructs `WorkingDir(PathBuf::from("/tmp"))` |
| `serde_json::to_string(&WorkingDir::new("/tmp")).unwrap()` | `"\"/tmp\""` |
| `BackendUrl::parse("wss://example.com:8443")` | `Ok(BackendUrl { protocol: Wss, host: "example.com", port: 8443 })` |
| `format!("{}", BackendUrl { protocol: Ws, host: "x", port: 80 })` | `"ws://x:80"` |

Error handling for `BackendUrl::parse`: returns `Result<BackendUrl, BackendUrlParseError>`. `BackendUrlParseError` is an enum with variants `InvalidProtocol(String)`, `MissingHost`, `InvalidPort(String)`. It derives `Debug`, `Clone`, `Error`, `PartialEq`.

Re-exports:

- `crate::types::*` is the canonical import path.
- If a prelude module is convenient, add `src/prelude.rs` re-exporting these names and update `src/lib.rs` to expose both `pub mod types;` and (optionally) `pub mod prelude;`. Otherwise document the import path in rustdoc.

## Risks

- **`#[serde(transparent)]` interaction with `#[derive(JsonSchema)]`.** Some existing wire types derive `JsonSchema` via `schemars`. Newtypes over primitive types should forward the schema to the wrapped type. Default: derive `JsonSchema` on every newtype and add a test that the generated schema reports `type: "string"` for `String`/`Uuid`-backed newtypes and `type: "string"` for `WorkingDir`. If a collision surfaces, document it and add an explicit `impl JsonSchema` that forwards.
- **`BackendUrl` shape change.** Today Registry stores `host`, `port`, `protocol` as separate `String` fields inside `BackendInfo`. This ticket defines `BackendUrl` but does NOT yet mutate `BackendInfo` — ST-9 owns that migration and is the ticket that must prove the wire format holds. This ticket only provides the type.
- **`WorkingDir` path encoding on non-UTF8 filesystems.** `PathBuf` can contain non-UTF8 bytes on Unix. For serialization, use `String` form via `to_string_lossy()` on the `Display` path and `Path::to_str()` fallible on serialize. If `to_str()` returns `None`, serialize fails with a descriptive error. Substrate only runs on macOS/Linux with UTF-8 paths in practice; this is a known constraint, not a blocker.
- **`uuid` crate feature flags.** `Uuid` must be serializable as a string via `serde`. Ensure `uuid = { version = "...", features = ["serde", "v4"] }` is already present in `Cargo.toml` — it is for existing `ApprovalId = Uuid` usage. No change needed.

## What must NOT change

- No existing source file is modified by this ticket EXCEPT `src/lib.rs` (to add `pub mod types;`) and possibly `src/prelude.rs` if created.
- No activation `types.rs` file is touched. ST-3 through ST-9 migrate activations individually.
- No SQLite schema changes.
- No wire-format changes yet observable — this ticket only defines types; nothing consumes them.
- `arbor::ArborId`, `arbor::NodeId`, `arbor::TreeId` are not moved or modified.
- The existing `claudecode_loopback::ApprovalId = Uuid` alias keeps compiling — ST-7 replaces it.

## Acceptance criteria

1. `cargo build -p plexus-substrate` succeeds.
2. `cargo test -p plexus-substrate` succeeds — no pre-existing tests regress.
3. A new unit test module `src/types.rs` (or a dedicated `#[cfg(test)] mod tests`) exercises each newtype:

   | Test | Expected |
   |---|---|
   | Construct `SessionId::new("s1")` | Display yields `"s1"`; `Clone` is a distinct value equal by `PartialEq` |
   | Roundtrip `SessionId` through `serde_json` | Input `"\"s1\""` → output `"\"s1\""` |
   | Construct `StreamId::new(Uuid::nil())` | Display yields the nil UUID canonical string |
   | Roundtrip `StreamId` through `serde_json` | UUID serializes as a bare JSON string, not an object |
   | Construct and roundtrip `ApprovalId`, `ToolUseId`, `TicketId`, `GraphId`, `NodeId`, `ModelId`, `TemplateId` | Same bare-string/UUID pattern |
   | Construct `WorkingDir::new("/tmp/x")` | Display yields `"/tmp/x"`; serialize yields `"\"/tmp/x\""` |
   | `HashMap::<SessionId, u32>::new().insert(SessionId::new("s"), 1)` | Compiles and works |
   | Construct `BackendUrl { protocol: Wss, host: "h".into(), port: 443 }` | Display yields `"wss://h:443"` |
   | `BackendUrl::parse("ws://a:80")` | `Ok(_)` with matching fields |
   | `BackendUrl::parse("http://a:80")` | `Err(BackendUrlParseError::InvalidProtocol(_))` |
   | `BackendUrl::parse("wss://a")` | `Err(BackendUrlParseError::InvalidPort(_))` or equivalent port-missing error |
   | Serialize/deserialize `BackendUrl` through serde | Object with fields `protocol`, `host`, `port`; `protocol` is a lowercase string |

4. A swap-compile negative test (documented in a doc comment with a `compile_fail` example in rustdoc, OR asserted via `trybuild` if that infra exists; if `trybuild` is absent, the rustdoc compile_fail example is sufficient):

   ```rust
   fn wants_session(_: SessionId) {}
   let sid = StreamId::new(Uuid::nil());
   wants_session(sid); // does NOT compile
   ```

5. `pub mod types;` is exported from `src/lib.rs`. Running `cargo doc -p plexus-substrate --no-deps` lists each newtype in the generated API docs.

## Completion

Implementor delivers:

- A commit adding `src/types.rs` with all ten identifier newtypes, `BackendUrl`, `BackendProtocol`, and `BackendUrlParseError`.
- Unit tests covering every acceptance-criterion row.
- `cargo build -p plexus-substrate`, `cargo test -p plexus-substrate`, and `cargo doc -p plexus-substrate --no-deps` all green.
- Ticket status flipped from `Ready` → `Complete` in the same commit.
- ST-3 through ST-9 unblocked; the implementor notes this in the PR/commit description.
