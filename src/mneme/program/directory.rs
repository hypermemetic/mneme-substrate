//! Program directory layout — filesystem structure for a single program run.
//!
//! Layout at `<root>/<program_id>/`:
//!
//! ```text
//! manifest.json              required
//! artifact.json              when status = completed
//! error.json                 when status = failed
//! trace.jsonl                always (append-only)
//! sessions/<session_id>.json one per claudecode session attributed to this program
//! skills/<child_program_id>/ one subdirectory per loopback child program
//! ```

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::id::ProgramId;
use super::manifest::Manifest;
use super::trace::TraceEntry;

/// Errors produced by program-directory operations.
#[derive(Debug, thiserror::Error)]
pub enum DirectoryError {
    #[error("io error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("serde error: {0}")]
    Serde(#[from] serde_json::Error),
}

/// Owned handle to a program directory on disk.
#[derive(Debug, Clone)]
pub struct ProgramDirectory {
    program_id: ProgramId,
    root: PathBuf,
}

impl ProgramDirectory {
    /// Create the directory layout (parent dirs + sessions/ + skills/).
    /// Idempotent: if the directory already exists, this returns Ok(_).
    pub fn create(programs_root: impl AsRef<Path>, program_id: ProgramId) -> Result<Self, DirectoryError> {
        let root = programs_root.as_ref().join(program_id.as_str());
        let sessions_dir = root.join("sessions");
        let skills_dir = root.join("skills");
        for d in [&root, &sessions_dir, &skills_dir] {
            fs::create_dir_all(d).map_err(|source| DirectoryError::Io {
                path: d.clone(),
                source,
            })?;
        }
        Ok(Self { program_id, root })
    }

    /// Open an existing program directory (does not create).
    pub fn open(programs_root: impl AsRef<Path>, program_id: ProgramId) -> Self {
        let root = programs_root.as_ref().join(program_id.as_str());
        Self { program_id, root }
    }

    pub fn program_id(&self) -> &ProgramId {
        &self.program_id
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn manifest_path(&self) -> PathBuf {
        self.root.join("manifest.json")
    }

    pub fn artifact_path(&self) -> PathBuf {
        self.root.join("artifact.json")
    }

    pub fn error_path(&self) -> PathBuf {
        self.root.join("error.json")
    }

    pub fn trace_path(&self) -> PathBuf {
        self.root.join("trace.jsonl")
    }

    pub fn sessions_dir(&self) -> PathBuf {
        self.root.join("sessions")
    }

    pub fn skills_dir(&self) -> PathBuf {
        self.root.join("skills")
    }

    pub fn session_path(&self, session_id: &str) -> PathBuf {
        self.sessions_dir().join(format!("{}.json", session_id))
    }

    pub fn child_program_root(&self, child_id: &ProgramId) -> PathBuf {
        self.skills_dir().join(child_id.as_str())
    }

    /// Write `manifest.json` (overwriting if present). Pretty-printed for human inspection.
    pub fn write_manifest(&self, manifest: &Manifest) -> Result<(), DirectoryError> {
        let path = self.manifest_path();
        let json = serde_json::to_vec_pretty(manifest)?;
        fs::write(&path, json).map_err(|source| DirectoryError::Io { path, source })
    }

    /// Read `manifest.json`. Errors if missing or malformed.
    pub fn read_manifest(&self) -> Result<Manifest, DirectoryError> {
        let path = self.manifest_path();
        let bytes = fs::read(&path).map_err(|source| DirectoryError::Io {
            path: path.clone(),
            source,
        })?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    /// Write `artifact.json` (overwriting if present). The caller's typed value.
    pub fn write_artifact<T: Serialize>(&self, value: &T) -> Result<(), DirectoryError> {
        let path = self.artifact_path();
        let json = serde_json::to_vec_pretty(value)?;
        fs::write(&path, json).map_err(|source| DirectoryError::Io { path, source })
    }

    /// Write `error.json`. Use this on the failure path.
    pub fn write_error(&self, kind: &str, message: &str, stage: &str) -> Result<(), DirectoryError> {
        let value = serde_json::json!({
            "kind": kind,
            "message": message,
            "stage": stage,
        });
        let path = self.error_path();
        let json = serde_json::to_vec_pretty(&value)?;
        fs::write(&path, json).map_err(|source| DirectoryError::Io { path, source })
    }

    /// Append one trace entry as a JSONL line. Creates the file on first call.
    pub fn append_trace(&self, entry: &TraceEntry) -> Result<(), DirectoryError> {
        let path = self.trace_path();
        let mut line = entry.to_jsonl()?;
        line.push('\n');
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|source| DirectoryError::Io {
                path: path.clone(),
                source,
            })?;
        file.write_all(line.as_bytes())
            .map_err(|source| DirectoryError::Io {
                path: path.clone(),
                source,
            })?;
        Ok(())
    }

    /// Write a captured claudecode session JSON into `sessions/`.
    pub fn write_session(&self, session_id: &str, session_json: &[u8]) -> Result<(), DirectoryError> {
        let path = self.session_path(session_id);
        fs::write(&path, session_json).map_err(|source| DirectoryError::Io { path, source })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mneme::program::manifest::ProgramStatus;
    use crate::mneme::program::trace::{TraceOp, TraceOutcome};
    use std::time::Duration;
    use tempfile::TempDir;

    fn programs_root() -> TempDir {
        TempDir::new().expect("tempdir")
    }

    #[test]
    fn create_makes_full_layout() {
        let root = programs_root();
        let id = ProgramId::new();
        let dir = ProgramDirectory::create(root.path(), id.clone()).unwrap();
        assert!(dir.root().is_dir());
        assert!(dir.sessions_dir().is_dir());
        assert!(dir.skills_dir().is_dir());
        assert_eq!(dir.program_id(), &id);
    }

    #[test]
    fn create_is_idempotent() {
        let root = programs_root();
        let id = ProgramId::new();
        ProgramDirectory::create(root.path(), id.clone()).unwrap();
        // Second create should not error.
        ProgramDirectory::create(root.path(), id).unwrap();
    }

    #[test]
    fn manifest_round_trips() {
        let root = programs_root();
        let id = ProgramId::new();
        let dir = ProgramDirectory::create(root.path(), id.clone()).unwrap();
        let manifest = Manifest::open(
            id,
            "forecast.update",
            serde_json::json!({"q": "test"}),
            "0.6.3",
            "0.1.0",
        );
        dir.write_manifest(&manifest).unwrap();
        let back = dir.read_manifest().unwrap();
        assert_eq!(back.entry_skill, "forecast.update");
        assert_eq!(back.status, ProgramStatus::Running);
    }

    #[test]
    fn artifact_writes_pretty_json() {
        let root = programs_root();
        let id = ProgramId::new();
        let dir = ProgramDirectory::create(root.path(), id).unwrap();
        let value = serde_json::json!({"probability": 0.34, "summary": "..."});
        dir.write_artifact(&value).unwrap();
        let bytes = std::fs::read(dir.artifact_path()).unwrap();
        let text = String::from_utf8(bytes).unwrap();
        // Pretty-print introduces newlines.
        assert!(text.contains('\n'));
        assert!(text.contains("\"probability\""));
    }

    #[test]
    fn append_trace_creates_file_then_appends() {
        let root = programs_root();
        let id = ProgramId::new();
        let dir = ProgramDirectory::create(root.path(), id).unwrap();
        let entry = TraceEntry::new(
            1,
            TraceOp::SwarmTrial,
            serde_json::json!({}),
            vec![],
            TraceOutcome::Ok,
            Duration::from_millis(10),
        );
        dir.append_trace(&entry).unwrap();
        let entry2 = TraceEntry::new(
            2,
            TraceOp::SwarmAggregate,
            serde_json::json!({}),
            vec![],
            TraceOutcome::Ok,
            Duration::from_millis(1),
        );
        dir.append_trace(&entry2).unwrap();

        let bytes = std::fs::read(dir.trace_path()).unwrap();
        let text = String::from_utf8(bytes).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        let first: TraceEntry = serde_json::from_str(lines[0]).unwrap();
        let second: TraceEntry = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(first.seq, 1);
        assert_eq!(second.seq, 2);
    }

    #[test]
    fn error_json_has_expected_fields() {
        let root = programs_root();
        let id = ProgramId::new();
        let dir = ProgramDirectory::create(root.path(), id).unwrap();
        dir.write_error("Validation", "schema mismatch", "respond")
            .unwrap();
        let bytes = std::fs::read(dir.error_path()).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["kind"], "Validation");
        assert_eq!(value["message"], "schema mismatch");
        assert_eq!(value["stage"], "respond");
    }

    #[test]
    fn session_path_includes_id() {
        let root = programs_root();
        let id = ProgramId::new();
        let dir = ProgramDirectory::create(root.path(), id).unwrap();
        let session_path = dir.session_path("sess-abc");
        assert!(session_path.to_string_lossy().ends_with("sessions/sess-abc.json"));
    }

    #[test]
    fn child_program_root_is_under_skills() {
        let root = programs_root();
        let parent_id = ProgramId::new();
        let child_id = ProgramId::new();
        let dir = ProgramDirectory::create(root.path(), parent_id).unwrap();
        let child = dir.child_program_root(&child_id);
        let s = child.to_string_lossy();
        assert!(s.contains("/skills/"));
        assert!(s.contains(&child_id.as_str()));
    }
}
