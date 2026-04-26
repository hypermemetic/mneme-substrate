# bash

Execute bash commands and stream output.

## Overview

Bash runs a command through a POSIX shell and streams each stdout line,
stderr line, and the final exit code as discrete `BashEvent` items. Executor
errors (failed spawn, failed wait, missing stdio capture) surface as an
`Error` variant rather than a transport failure, so callers can handle them
uniformly.

Bash also registers a handful of mustache templates on startup via
`register_default_templates` (for the `execute` method: `default`, `compact`,
and `verbose` variants) so handle-rendering flows can format command output
without reimplementing it per call-site.

## Namespace

`bash` — invoked via `synapse <backend> bash.<method>`.

## Methods

| Method | Params | Returns | Description |
|---|---|---|---|
| `execute` | `command: String` | `Stream<Item=BashEvent>` | Execute a bash command and stream `Stdout` / `Stderr` lines followed by an `Exit { code }` (or `Error`). |

`BashEvent` variants: `Stdout { line }`, `Stderr { line }`, `Exit { code }`,
`Error { message }`.

## Composition

- `BashExecutor` (sibling module) — owns the `tokio::process::Command`
  spawn and the stdio-pump loop.
- `Mustache` — Bash calls `register_default_templates(mustache)` during
  startup to install `execute.default`, `execute.compact`, and `execute.verbose`
  templates.

## Example

```bash
synapse --port 44104 lforge substrate bash.execute '{"command":"ls -la /tmp"}'
```

## Source

- `activation.rs` — RPC method surface + template registration
- `executor/` — process-spawn + stdio-pump implementation
- `types.rs` — `BashEvent` / `BashOutput` alias / `ExecutorError`
- `mod.rs` — module exports
