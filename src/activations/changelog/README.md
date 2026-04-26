# changelog

Track and document plexus configuration changes.

## Overview

Changelog watches the substrate's plexus-schema hash and enforces that every
wire-visible change is documented. On startup the builder calls
`Changelog::startup_check(current_hash)`, which compares the current hash
to the last-seen hash persisted in storage. A change without a matching
changelog entry produces an "UNDOCUMENTED PLEXUS CHANGE" message — a signal
to add an entry before publishing.

Changelog also exposes a lightweight planned-change queue: work that should
be implemented against some future hash. Queue entries are tagged (e.g.
`frontend`, `api`, `breaking`) so downstream consumers can filter to the set
they need to act on; when an implementation lands, `queue_complete` links
the queue entry to the hash in which the change was realized.

## Namespace

`changelog` — invoked via `synapse <backend> changelog.<method>`.

## Methods

### Entry operations

| Method | Params | Returns | Description |
|---|---|---|---|
| `add` | `hash: String, summary: String, previous_hash: Option<String>, details: Option<Vec<String>>, author: Option<String>, queue_id: Option<String>` | `Stream<Item=ChangelogEvent>` | Add a changelog entry documenting a plexus-hash change. Completes a queue entry if `queue_id` is supplied. |
| `list` | — | `Stream<Item=ChangelogEvent>` | List all changelog entries, newest first. |
| `get` | `hash: String` | `Stream<Item=ChangelogEvent>` | Get the changelog status for a specific hash. |
| `check` | `current_hash: String` | `Stream<Item=ChangelogEvent>` | Check whether the current plexus configuration is documented. |

### Queue operations

| Method | Params | Returns | Description |
|---|---|---|---|
| `queue_add` | `description: String, tags: Option<Vec<String>>` | `Stream<Item=ChangelogEvent>` | Queue a planned change that systems should implement. |
| `queue_list` | `tag: Option<String>` | `Stream<Item=ChangelogEvent>` | List all queued changes, optionally filtered by tag. |
| `queue_pending` | `tag: Option<String>` | `Stream<Item=ChangelogEvent>` | List pending (incomplete) queue entries. |
| `queue_get` | `id: String` | `Stream<Item=ChangelogEvent>` | Get a specific queued change by ID. |
| `queue_complete` | `id: String, hash: String` | `Stream<Item=ChangelogEvent>` | Mark a queued change complete, linking it to the hash where it was implemented. |

## Storage

- Backend: SQLite
- Config: `ChangelogStorageConfig` with `db_path`.
- Schema: two tables — changelog entries keyed by hash, queue entries keyed
  by UUID with `tags` and optional `completed_at`/`completed_hash`. See
  `src/activations/changelog/storage.rs`.

## Composition

Changelog is self-contained at the method layer. The builder calls
`startup_check(&hash)` during hub assembly so hash regressions surface in
the startup logs before any client connects.

## Example

```bash
synapse --port 44104 lforge substrate changelog.check '{"current_hash":"abc123"}'
synapse --port 44104 lforge substrate changelog.add \
  '{"hash":"abc123","summary":"added foo.bar method","author":"me"}'
```

## Source

- `activation.rs` — RPC method surface + `startup_check`
- `storage.rs` — SQLite persistence + `ChangelogStorageConfig`
- `types.rs` — `ChangelogEntry`, `QueueEntry`, `QueueStatus`, `ChangelogEvent`
- `mod.rs` — module exports
