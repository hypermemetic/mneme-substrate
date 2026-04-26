---
id: OB-2
title: "Config file loader (TOML) with per-activation sections and env overrides"
status: Pending
type: implementation
blocked_by: []
unlocks: []
severity: High
target_repo: plexus-substrate
---

## Problem

Substrate today has no config file. Every activation's construction path is `ActivationType::default()` plus an ad-hoc handful of `std::env::var(...)` reads scattered through the tree. DB paths live in `activation_db_path_from_module!`, ports live in `main.rs` as literals, timeouts live in `Default` impls, API base URLs live inline. An operator cannot answer "what are the current substrate settings?" without reading source.

This ticket introduces a TOML config loader at `~/.plexus/substrate/config.toml` with per-activation sections and env-var overrides. Each activation pulls its config from this loader at startup; ad-hoc `env::var` reads inside activation code move into the loader. The default config (no file, no env) produces exactly the same substrate behavior as today — the ticket is purely additive with respect to defaults; it adds **observability and overridability**, not new knobs.

## Context

Affected components:

| Component | Role |
|---|---|
| `src/builder.rs` | Activation startup; currently calls `::default()` on every activation's constructor. Wires config after this ticket. |
| `src/main.rs` | Server entry point; reads port from a literal / env var today. Becomes config-driven. |
| New `src/config/mod.rs` (or `src/config.rs`) | Loader, schema, merge logic. The ticket's primary surface. |
| Every activation's `mod.rs` / `activation.rs` | Adds a `from_config(&ActivationConfig) -> Self` (or equivalent) constructor alongside the existing `::default()` / `::new()` path. Existing constructors remain to preserve test ergonomics. |

**Config schema (pinned for this ticket):**

```toml
# ~/.plexus/substrate/config.toml

[server]
port = 4444                          # main RPC port
metrics_port = 9090                  # owned by OB-3; reserved here

[storage]                            # reserved for STG epic; OB-2 pins the section name
kind = "sqlite"                      # "sqlite" | "postgres" | "memory"
dir = "~/.plexus/substrate/activations"

# Per-activation sections. Section name is the activation's namespace string.
[orcha]
# activation-specific keys populated per-activation by this ticket

[orcha.storage]                      # reserved for STG epic
# overrides at per-activation granularity

[claudecode]
# ...

[cone]
# ...

# ... one section per stateful activation
```

**Env-var override convention (pinned):**

Any config key is overridable by env var `PLEXUS_SUBSTRATE__<SECTION>__<KEY>` (double underscore as separator — standard in the `config` crate ecosystem). Env vars take precedence over file values. File values take precedence over built-in defaults.

Examples:
- `PLEXUS_SUBSTRATE__SERVER__PORT=8080` overrides `[server] port`.
- `PLEXUS_SUBSTRATE__ORCHA__STORAGE__DIR=/tmp/orcha` overrides `[orcha.storage] dir`.

**Path conventions:**

| Config source | Precedence (high → low) |
|---|---|
| CLI flag `--config <path>` | highest |
| Env var `PLEXUS_SUBSTRATE_CONFIG` | second |
| `~/.plexus/substrate/config.toml` | third |
| (file missing) built-in defaults | lowest |

Individual-key env overrides (`PLEXUS_SUBSTRATE__SECTION__KEY`) apply on top of whichever of the above was selected.

**Existing ad-hoc env vars to absorb** (re-verify against HEAD during implementation; audit pointers drift):

The implementation begins with a sweep — `rg 'std::env::var|env!|\.env\(' src/` — and every matching site either moves into the config loader or is explicitly documented as out of scope (e.g., log-level env vars like `RUST_LOG` stay ad-hoc because they're `tracing` conventions, not substrate config).

## Required behavior

### Loading

| Startup condition | Result |
|---|---|
| Config file exists, valid TOML | File loaded; env vars layered on top; activations constructed from the merged config. |
| Config file exists, invalid TOML | Substrate prints a clear error to stderr (file path + parse location) and exits non-zero. |
| Config file absent | Built-in defaults used; env vars applied on top. Substrate boots identically to today's behavior. |
| `--config <path>` provided, file absent | Substrate errors (explicit opt-in to a missing file is a mistake; exit non-zero). |
| Config section for an activation is absent | Activation uses built-in defaults for that section. No error. |
| Config has unknown top-level section | Logged at `warn!` level; not an error. (Future activations may land with pre-populated config.) |
| Config has unknown key within a known section | Logged at `warn!` level; not an error. |

### Per-activation consumption

Each stateful activation gains a public `from_config` constructor. Example shape (activation-specific fields vary):

```rust
impl OrchaActivation {
    pub fn from_config(cfg: &OrchaConfig) -> Self { ... }
}
```

`OrchaConfig`, `ClaudeCodeConfig`, etc., are structs in the config loader; each activation owns its config struct's field list (this ticket pins the initial fields by absorbing current `env::var` reads and `Default` values).

`builder.rs` calls `from_config` instead of `Default::default()` for activations that grow a config struct. Activations without current config needs (Echo, Health, Chaos, Bash, Interactive) keep their existing constructors; they can grow config in future tickets.

### Default equivalence

With no config file and no env overrides, substrate behaves **identically** to pre-ticket substrate:
- Same ports.
- Same DB paths.
- Same timeouts.
- Same API base URLs.

The "default equivalence" property is verifiable by running substrate with no config file and observing that all existing tests pass.

### Observability of effective config

Substrate logs the **effective config** (post-merge) at `info!` level at startup. Secrets (if any exist in config — API keys, passwords) are redacted in the log output. This ticket does not introduce secret-handling infrastructure; it pins the pattern that any field named `*_key`, `*_secret`, `*_password`, or `*_token` is redacted before logging. Per-activation tickets that later add secret fields must name them consistently with that pattern.

## Risks

| Risk | Mitigation |
|---|---|
| The audit's env-var sweep discovers more scattered reads than expected; absorbing all of them blows the ticket scope. | Ticket scope is "absorb env vars currently read at activation-construction time". Reads inside method handlers (e.g., `env::var` at tool-call time) stay ad-hoc and are flagged for follow-up tickets, not absorbed here. |
| `~/.plexus/substrate/config.toml` conflicts with an existing user file (unlikely given the path's novelty but possible). | The loader refuses to overwrite existing unexpected content; if the file exists and parses, proceed; if invalid, exit with a clear error (don't silently recreate). |
| Env-var override convention collides with `plexus-core` or `plexus-transport` conventions. | Re-verify at implementation time. If collision exists, pick a distinct prefix (`SUBSTRATE__` instead of `PLEXUS_SUBSTRATE__`) and document. |
| Existing tests rely on ad-hoc env vars to override defaults. | Env vars continue to work through the new loader (higher precedence than file). Tests unaffected. |
| STG's storage-config keys (`storage.kind`, `storage.dir`) are pinned here before STG's spike resolves, and STG later picks different names. | OB-2 reserves the `[storage]` section and keys `kind` / `dir` as placeholders; STG renames at its landing if needed. Cross-epic coordination responsibility is on STG, not OB-2. |
| Loading becomes async-dependent (e.g., reading config from remote). | Out of scope. Config is synchronous, file-based. |

## What must NOT change

- Default substrate behavior with no config file and no env overrides. Every existing test must pass unchanged.
- Activation namespace strings, method names, wire shapes.
- DB file paths at default (`~/.plexus/substrate/activations/{name}/`). The paths become overridable, but defaults are identical.
- Default main RPC port.
- The `activation_db_path_from_module!` macro. It remains the path-derivation primitive; the config loader's `storage.dir` feeds into it.
- `builder.rs` startup order (cyclic-parent injection via `OnceLock<Weak<DynamicHub>>`).
- Existing `cargo test` pass rate.

## Acceptance criteria

1. A file `~/.plexus/substrate/config.toml` with a minimal valid schema (e.g., `[server] port = 4444`) is parsed at startup and applied — verifiable by running substrate with a config that sets `port = 4445` and observing the server bound to 4445.
2. `PLEXUS_SUBSTRATE__SERVER__PORT=4446` (with no config file) binds substrate to port 4446.
3. Config file plus env override: file sets `port = 4445`, env sets `PLEXUS_SUBSTRATE__SERVER__PORT=4447`; substrate binds 4447 (env wins).
4. Missing config file: substrate boots with built-in defaults; `cargo test --workspace` passes exactly as before the ticket.
5. Invalid TOML in the config file: substrate prints a stderr error naming the file path and the parse location (line/column), then exits non-zero.
6. Unknown top-level section: logged at `warn!` level; substrate boots.
7. `--config <path>` flag overrides the default path; pointing it at a non-existent file exits non-zero with a clear error.
8. Substrate logs the effective config at startup (`info!` level). Running with a config that includes a key matching `*_secret` shows the value redacted as `<REDACTED>`.
9. Every activation that previously read an env var at construction time now pulls that value from its config section. Verifiable by grep: `rg 'std::env::var' src/activations/` returns no matches at activation-constructor scope (reads inside method handlers are out of scope and do not count).
10. Each activation with config has a `from_config` (or equivalent) constructor; `builder.rs` calls it instead of `Default::default()` for that activation.
11. A committed example config file at `examples/config.toml` (or `docs/config.toml.example`) showing every section with explanatory comments.

## Completion

PR against `plexus-substrate`. CI green. Status flipped from `Ready` to `Complete` in the same commit that lands the config loader. With OB-2 Complete, operators can express substrate configuration in one place — the prerequisite for OB-3's metrics port config, OB-5's log-level tuning, and STG's storage backend selection.
