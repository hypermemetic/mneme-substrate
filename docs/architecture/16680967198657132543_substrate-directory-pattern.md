# .substrate Directory Pattern

The `.substrate` directory is a local workspace directory (similar to `.git`) that stores substrate-related data in the current working directory.

## Location

```
project/
├── .substrate/           # Created automatically
│   ├── arbor.db          # Tree storage (SQLite)
│   ├── cone.db           # LLM conversation state (SQLite)
│   └── templates/        # Custom output templates (planned)
└── ... project files
```

## Design Philosophy

The `.substrate` pattern follows these principles:

1. **Locality**: Data lives next to the project it serves
2. **Portability**: Moving the project moves its substrate state
3. **Discoverability**: Standard location, easy to find/backup/ignore
4. **Isolation**: Each project has independent state

## Implementation

### Directory Initialization (Rust)

```rust
// src/main.rs
fn substrate_data_dir() -> PathBuf {
    let cwd = std::env::current_dir()
        .expect("Failed to get current working directory");
    cwd.join(".substrate")
}

fn init_data_dir() -> std::io::Result<(PathBuf, PathBuf)> {
    let data_dir = substrate_data_dir();
    std::fs::create_dir_all(&data_dir)?;

    let arbor_db = data_dir.join("arbor.db");
    let cone_db = data_dir.join("cone.db");

    Ok((arbor_db, cone_db))
}
```

### Contents

| File | Purpose | Activation |
|------|---------|------------|
| `arbor.db` | Tree storage for conversation context | Arbor |
| `cone.db` | LLM conversation state and routing | Cone |
| `templates/` | Custom output templates (planned) | CLI |

## Client-Side Usage (Planned)

The `symbols` CLI will also use `.substrate` for:

### Template Storage

```
.substrate/
└── templates/
    ├── arbor/
    │   ├── tree-list.hbs      # List trees output
    │   └── tree-get.hbs       # Single tree output
    ├── cone/
    │   └── registry.hbs       # Model registry output
    └── default.hbs            # Fallback template
```

### Template Resolution Order

1. `.substrate/templates/{namespace}/{method}.hbs` (project-local)
2. `~/.config/symbols/templates/{namespace}/{method}.hbs` (user global)
3. Built-in default (JSON pretty-print)

## Gitignore

Add to `.gitignore`:

```gitignore
# Substrate local state
.substrate/
```

Or to track templates but ignore databases:

```gitignore
.substrate/*.db
```

## Future Extensions

The `.substrate` directory may grow to include:

- `config.toml` - Local substrate configuration
- `hooks/` - Event hooks (pre/post method calls)
- `cache/` - Response caching
- `logs/` - Local operation logs
