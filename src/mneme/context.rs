//! Shared context handed to every skill activation at construction.
//!
//! Bundles the substrate-side handles a skill needs to do real work:
//! - Where program directories live on disk
//! - The `SwarmRuntime` (real or mock) for orchestration calls
//! - Version strings for manifest provenance
//!
//! `builder.rs` constructs one `MnemeContext` and clones the `Arc` into each
//! skill activation. Tests construct contexts pointing at temp directories
//! and stub/mock runtimes.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::mneme::program::{Program, ProgramError};
use crate::mneme::runtime::swarm_runtime::{StubSwarmRuntime, SwarmRuntime};
use crate::mneme::storage::SharedStorage;

/// Shared per-substrate context for skill activations.
#[derive(Clone)]
pub struct MnemeContext {
    programs_root: PathBuf,
    swarm: Arc<dyn SwarmRuntime>,
    storage: Option<SharedStorage>,
    substrate_version: String,
    mneme_version: String,
}

impl std::fmt::Debug for MnemeContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MnemeContext")
            .field("programs_root", &self.programs_root)
            .field("substrate_version", &self.substrate_version)
            .field("mneme_version", &self.mneme_version)
            .field("swarm", &"<dyn SwarmRuntime>")
            .finish()
    }
}

impl MnemeContext {
    /// Build a context. `programs_root` is where `programs/<id>/` directories
    /// live. `swarm` is the runtime for orchestration calls.
    pub fn new(programs_root: impl Into<PathBuf>, swarm: Arc<dyn SwarmRuntime>) -> Self {
        Self {
            programs_root: programs_root.into(),
            swarm,
            storage: None,
            substrate_version: env!("CARGO_PKG_VERSION").to_string(),
            mneme_version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }

    /// Attach a storage handle for the queryable programs index.
    /// Builder pattern; returns self for chaining.
    pub fn with_storage(mut self, storage: SharedStorage) -> Self {
        self.storage = Some(storage);
        self
    }

    /// Convenience for tests: a context with a stubbed swarm runtime that
    /// always returns NotImplemented. Useful when a test needs a context
    /// to exist but doesn't actually exercise swarm calls.
    pub fn with_stub_swarm(programs_root: impl Into<PathBuf>) -> Self {
        Self::new(programs_root, Arc::new(StubSwarmRuntime))
    }

    pub fn programs_root(&self) -> &Path {
        &self.programs_root
    }

    pub fn swarm(&self) -> &Arc<dyn SwarmRuntime> {
        &self.swarm
    }

    pub fn storage(&self) -> Option<&SharedStorage> {
        self.storage.as_ref()
    }

    pub fn substrate_version(&self) -> &str {
        &self.substrate_version
    }

    pub fn mneme_version(&self) -> &str {
        &self.mneme_version
    }

    /// Open a program rooted at `programs_root` with the configured versions.
    /// If a storage handle is attached, the program also gets indexed in SQLite
    /// (insert on open, update on close).
    pub async fn open_program(
        &self,
        entry_skill: impl Into<String>,
        inputs: serde_json::Value,
    ) -> Result<Program, ProgramError> {
        let entry_skill = entry_skill.into();
        let mut program = Program::open(
            &self.programs_root,
            &entry_skill,
            inputs,
            self.substrate_version.clone(),
            self.mneme_version.clone(),
        )?;
        if let Some(storage) = &self.storage {
            // Best-effort: log but don't fail if the index can't be written.
            // The filesystem is the truth; SQLite is the index.
            if let Err(e) = storage
                .insert_program(
                    program.id(),
                    None,
                    &entry_skill,
                    self.substrate_version.as_str(),
                    self.mneme_version.as_str(),
                    program.depth(),
                )
                .await
            {
                tracing::warn!("storage.insert_program failed: {}", e);
            }
            program.attach_storage(storage.clone());
        }
        Ok(program)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    #[test]
    fn with_stub_swarm_constructs() {
        let root = TempDir::new().unwrap();
        let ctx = MnemeContext::with_stub_swarm(root.path());
        assert_eq!(ctx.programs_root(), root.path());
        assert!(!ctx.substrate_version().is_empty());
    }

    #[tokio::test]
    async fn open_program_creates_directory() {
        let root = TempDir::new().unwrap();
        let ctx = MnemeContext::with_stub_swarm(root.path());
        let prog = ctx.open_program("forecast.update", json!({"q": "demo"})).await.unwrap();
        assert!(prog.directory().root().exists());
    }

    #[tokio::test]
    async fn open_program_with_storage_indexes_in_sqlite() {
        use crate::mneme::storage::MnemeStorage;
        let root = TempDir::new().unwrap();
        let storage = Arc::new(MnemeStorage::open_in_memory().await.unwrap());
        let ctx = MnemeContext::with_stub_swarm(root.path()).with_storage(storage.clone());
        let prog = ctx.open_program("forecast.update", json!({})).await.unwrap();
        let row = storage.get(prog.id()).await.unwrap().expect("indexed");
        assert_eq!(row.entry_skill, "forecast.update");
    }
}
