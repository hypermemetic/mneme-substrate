//! Program lifecycle types and on-disk layout.
//!
//! Every skill invocation that comes through the substrate's external dispatch
//! boundary is wrapped in a Program. The Program owns:
//!
//! - A unique [`ProgramId`]
//! - A [`Manifest`] describing entry skill, inputs, lifecycle state, versions
//! - A [`ProgramDirectory`] on disk capturing the manifest, artifact, trace,
//!   and any captured claudecode sessions
//! - A monotonic sequence number for [`TraceEntry`] writes
//!
//! Substrate-side code calls [`Program::open`] before dispatching the method,
//! [`Program::record_trace`] for each layer-1 orchestration call, and
//! [`Program::close_completed`] / [`Program::close_failed`] when the method
//! returns or errors.

use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};

use serde::Serialize;

use super::storage::SharedStorage;

pub mod directory;
pub mod id;
pub mod manifest;
pub mod trace;

pub use directory::{DirectoryError, ProgramDirectory};
pub use id::ProgramId;
pub use manifest::{Manifest, ProgramStatus, MANIFEST_SCHEMA_VERSION};
pub use trace::{TraceEntry, TraceOp, TraceOutcome};

/// Maximum loopback recursion depth before the substrate refuses to spawn a child.
/// Phase 1 hardcodes 8; tunable later via configuration.
pub const MAX_PROGRAM_DEPTH: u8 = 8;

/// Errors raised by Program operations.
#[derive(Debug, thiserror::Error)]
pub enum ProgramError {
    #[error(transparent)]
    Directory(#[from] DirectoryError),
    #[error("recursion depth {depth} exceeds MAX_PROGRAM_DEPTH ({max})")]
    DepthExceeded { depth: u8, max: u8 },
}

/// Live handle to an in-flight program. Owned for the program's lifetime.
#[derive(Debug)]
pub struct Program {
    directory: ProgramDirectory,
    manifest: Manifest,
    next_seq: AtomicU32,
    storage: Option<SharedStorage>,
}

impl Program {
    /// Open a fresh program. Creates the directory, writes the initial manifest.
    pub fn open(
        programs_root: impl AsRef<Path>,
        entry_skill: impl Into<String>,
        inputs: serde_json::Value,
        substrate_version: impl Into<String>,
        mneme_version: impl Into<String>,
    ) -> Result<Self, ProgramError> {
        let id = ProgramId::new();
        let directory = ProgramDirectory::create(programs_root, id.clone())?;
        let manifest = Manifest::open(id, entry_skill, inputs, substrate_version, mneme_version);
        directory.write_manifest(&manifest)?;
        Ok(Self {
            directory,
            manifest,
            next_seq: AtomicU32::new(1),
            storage: None,
        })
    }

    /// Open a child program under a parent's `skills/` directory.
    /// Bumps depth; refuses if it would exceed [`MAX_PROGRAM_DEPTH`].
    pub fn open_child(
        parent: &Program,
        entry_skill: impl Into<String>,
        inputs: serde_json::Value,
    ) -> Result<Self, ProgramError> {
        let parent_depth = parent.manifest.depth;
        let new_depth = parent_depth.saturating_add(1);
        if new_depth > MAX_PROGRAM_DEPTH {
            return Err(ProgramError::DepthExceeded {
                depth: new_depth,
                max: MAX_PROGRAM_DEPTH,
            });
        }
        let child_id = ProgramId::new();
        let directory = ProgramDirectory::create(parent.directory.skills_dir(), child_id.clone())?;
        let mut manifest = Manifest::open(
            child_id,
            entry_skill,
            inputs,
            parent.manifest.substrate_version.clone(),
            parent.manifest.mneme_version.clone(),
        );
        manifest = manifest.with_parent(parent.manifest.program_id.clone(), parent_depth);
        directory.write_manifest(&manifest)?;
        Ok(Self {
            directory,
            manifest,
            next_seq: AtomicU32::new(1),
            storage: None,
        })
    }

    pub fn id(&self) -> &ProgramId {
        &self.manifest.program_id
    }

    pub fn directory(&self) -> &ProgramDirectory {
        &self.directory
    }

    pub fn depth(&self) -> u8 {
        self.manifest.depth
    }

    /// Attach a storage handle so close_* methods mirror lifecycle into the
    /// SQLite index. Without this, the program is filesystem-only.
    pub fn attach_storage(&mut self, storage: SharedStorage) {
        self.storage = Some(storage);
    }

    /// Allocate the next monotonic sequence number for a trace entry.
    pub fn next_seq(&self) -> u32 {
        self.next_seq.fetch_add(1, Ordering::SeqCst)
    }

    /// Record a trace entry. Caller pre-built the entry with [`Program::next_seq`].
    pub fn record_trace(&self, entry: &TraceEntry) -> Result<(), ProgramError> {
        Ok(self.directory.append_trace(entry)?)
    }

    /// Close the program as completed; writes the artifact then updates the
    /// manifest. If a storage handle is attached, the SQLite index is updated
    /// as well (best-effort; logged on failure but not propagated since the
    /// filesystem is the truth).
    pub async fn close_completed<T: Serialize>(
        mut self,
        artifact: &T,
        artifact_schema_version: impl Into<String>,
    ) -> Result<(), ProgramError> {
        self.directory.write_artifact(artifact)?;
        self.manifest.complete(artifact_schema_version);
        self.directory.write_manifest(&self.manifest)?;
        if let Some(storage) = &self.storage {
            if let Err(e) = storage
                .update_status(&self.manifest.program_id, ProgramStatus::Completed)
                .await
            {
                tracing::warn!("storage.update_status(completed) failed: {}", e);
            }
        }
        Ok(())
    }

    /// Close the program as failed; writes error.json then updates the manifest.
    pub async fn close_failed(
        mut self,
        kind: &str,
        message: &str,
        stage: &str,
    ) -> Result<(), ProgramError> {
        self.directory.write_error(kind, message, stage)?;
        self.manifest.fail();
        self.directory.write_manifest(&self.manifest)?;
        if let Some(storage) = &self.storage {
            if let Err(e) = storage
                .update_status(&self.manifest.program_id, ProgramStatus::Failed)
                .await
            {
                tracing::warn!("storage.update_status(failed) failed: {}", e);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    #[test]
    fn open_creates_directory_and_manifest() {
        let root = TempDir::new().unwrap();
        let prog = Program::open(
            root.path(),
            "forecast.update",
            json!({}),
            "0.6.3",
            "0.1.0",
        )
        .unwrap();
        assert!(prog.directory().root().exists());
        assert!(prog.directory().manifest_path().exists());
        assert_eq!(prog.depth(), 0);
    }

    #[test]
    fn next_seq_is_monotonic() {
        let root = TempDir::new().unwrap();
        let prog = Program::open(root.path(), "x", json!({}), "v", "v").unwrap();
        assert_eq!(prog.next_seq(), 1);
        assert_eq!(prog.next_seq(), 2);
        assert_eq!(prog.next_seq(), 3);
    }

    #[tokio::test]
    async fn close_completed_writes_artifact_and_status() {
        let root = TempDir::new().unwrap();
        let prog = Program::open(root.path(), "x", json!({}), "v", "v").unwrap();
        let dir = prog.directory().clone();
        prog.close_completed(&json!({"ok": true}), "0.1.0").await.unwrap();
        assert!(dir.artifact_path().exists());
        let manifest = dir.read_manifest().unwrap();
        assert_eq!(manifest.status, ProgramStatus::Completed);
    }

    #[tokio::test]
    async fn close_failed_writes_error_and_status() {
        let root = TempDir::new().unwrap();
        let prog = Program::open(root.path(), "x", json!({}), "v", "v").unwrap();
        let dir = prog.directory().clone();
        prog.close_failed("Boom", "everything", "trial").await.unwrap();
        assert!(dir.error_path().exists());
        let manifest = dir.read_manifest().unwrap();
        assert_eq!(manifest.status, ProgramStatus::Failed);
    }

    #[test]
    fn child_inherits_substrate_version_and_bumps_depth() {
        let root = TempDir::new().unwrap();
        let parent = Program::open(root.path(), "outer", json!({}), "0.6.3", "0.1.0").unwrap();
        let child = Program::open_child(&parent, "inner", json!({})).unwrap();
        assert_eq!(child.depth(), 1);
        let cm = child.directory().read_manifest().unwrap();
        assert_eq!(cm.substrate_version, "0.6.3");
        assert_eq!(cm.parent_program_id.as_ref(), Some(parent.id()));
    }

    #[test]
    fn child_refuses_at_max_depth() {
        // Build a chain right up to the cap, then try one too many.
        let root = TempDir::new().unwrap();
        let mut prog = Program::open(root.path(), "lvl0", json!({}), "v", "v").unwrap();
        for i in 1..=MAX_PROGRAM_DEPTH {
            prog = Program::open_child(&prog, format!("lvl{}", i), json!({})).unwrap();
            assert_eq!(prog.depth(), i);
        }
        // Next child should fail.
        let err = Program::open_child(&prog, "too-deep", json!({})).unwrap_err();
        match err {
            ProgramError::DepthExceeded { depth, max } => {
                assert_eq!(depth, MAX_PROGRAM_DEPTH + 1);
                assert_eq!(max, MAX_PROGRAM_DEPTH);
            }
            _ => panic!("expected DepthExceeded"),
        }
    }

    #[test]
    fn record_trace_appends_to_jsonl() {
        let root = TempDir::new().unwrap();
        let prog = Program::open(root.path(), "x", json!({}), "v", "v").unwrap();
        let entry = TraceEntry::new(
            prog.next_seq(),
            TraceOp::SwarmTrial,
            json!({}),
            vec![],
            TraceOutcome::Ok,
            std::time::Duration::from_millis(5),
        );
        prog.record_trace(&entry).unwrap();
        let text = std::fs::read_to_string(prog.directory().trace_path()).unwrap();
        assert_eq!(text.lines().count(), 1);
    }
}
