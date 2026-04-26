# mustache

Mustache template rendering for handle values.

## Overview

Mustache is substrate's shared template renderer. Activations register named
templates keyed by `(plugin_id, method, template_name)` — e.g. Bash registers
`execute.default`, `execute.compact`, `execute.verbose` — and any caller can
render a resolved handle value against one of those templates by calling
`mustache.render`.

Templates are compiled at registration time (invalid templates are rejected
before they reach storage) and persisted in SQLite. The `register_template`
method is the RPC entry point; activations that need to install their
defaults at startup instead call `Mustache::register_template_direct` or
`register_templates` directly on the Rust API (non-streaming), avoiding an
RPC round-trip during hub construction.

## Namespace

`mustache` — invoked via `synapse <backend> mustache.<method>`.

## Methods

| Method | Params | Returns | Description |
|---|---|---|---|
| `render` | `plugin_id: Uuid, method: String, template_name: Option<String>, value: Value` | `Stream<Item=MustacheEvent>` | Render `value` using the named template (defaults to `"default"`). Emits `Rendered { output }`, `NotFound`, or `Error`. |
| `register_template` | `plugin_id: Uuid, method: String, name: String, template: String` | `Stream<Item=MustacheEvent>` | Register or update a template for a `(plugin, method, name)` triple. Rejects templates that fail to compile. |
| `list_templates` | `plugin_id: Uuid` | `Stream<Item=MustacheEvent>` | List all templates for a plugin. |
| `get_template` | `plugin_id: Uuid, method: String, name: String` | `Stream<Item=MustacheEvent>` | Fetch a single template by identity. |
| `delete_template` | `plugin_id: Uuid, method: String, name: String` | `Stream<Item=MustacheEvent>` | Delete a single template. |

## Storage

- Backend: SQLite
- Config: `MustacheStorageConfig { db_path: PathBuf }`
- Schema: templates keyed by `(plugin_id, method, name)`; see
  `src/activations/mustache/storage.rs`.

## Composition

- `Bash::register_default_templates(&mustache)` — Bash registers its
  `execute.*` templates on startup.
- `Cone::register_default_templates(&mustache)` — Cone registers
  `chat.default`, `chat.markdown`, `chat.json`, `create.default`, and
  `list.default`.

## Example

```bash
synapse --port 44104 lforge substrate mustache.render \
  '{"plugin_id":"<uuid>","method":"execute","template_name":"compact","value":{"stdout":"hi"}}'
```

## Source

- `activation.rs` — RPC method surface + direct registration helpers
- `storage.rs` — SQLite persistence + `MustacheStorageConfig`
- `types.rs` — `MustacheEvent` / `TemplateInfo` / `MustacheError`
- `mod.rs` — module exports
