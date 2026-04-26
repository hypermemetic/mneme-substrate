//! Program manifest — the metadata file that describes a single program run.
//!
//! Lives at `programs/<program_id>/manifest.json`. Written when the program
//! opens, updated when it closes. Schema versioned so future readers can
//! detect format drift.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::id::ProgramId;

/// Current manifest schema version. Bump on breaking changes.
pub const MANIFEST_SCHEMA_VERSION: &str = "0.1.0";

/// Lifecycle states of a program.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProgramStatus {
    /// Program is in flight; manifest.finished_at is null.
    Running,
    /// Program returned successfully; artifact.json was written.
    Completed,
    /// Program errored; error.json was written.
    Failed,
}

/// Manifest contents — serialized as `programs/<id>/manifest.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    /// This program's id.
    pub program_id: ProgramId,
    /// If this program was spawned by a parent via loopback, the parent's id.
    pub parent_program_id: Option<ProgramId>,
    /// The skill method that was invoked, e.g., "forecast.update".
    pub entry_skill: String,
    /// Arguments passed to the entry method.
    pub inputs: Value,
    /// Schema version of `inputs`. Each skill versions its own input schema.
    pub inputs_schema_version: String,
    /// When the program opened.
    pub started_at: DateTime<Utc>,
    /// When the program closed; null if still running.
    pub finished_at: Option<DateTime<Utc>>,
    /// Lifecycle state.
    pub status: ProgramStatus,
    /// Schema version of `artifact.json` once written; null while running or on failure.
    pub artifact_schema_version: Option<String>,
    /// The mneme-substrate version that ran the program.
    pub substrate_version: String,
    /// The mneme harness/binary version (may equal substrate_version in single-binary mode).
    pub mneme_version: String,
    /// Schema version of THIS manifest format. Always set on write.
    pub manifest_schema_version: String,
    /// Recursion depth (parent count). Used to refuse runaway loopback chains.
    #[serde(default)]
    pub depth: u8,
}

impl Manifest {
    /// Open a fresh manifest at the given program id with the given inputs.
    /// Sets `started_at = now()`, `status = Running`, defaults version fields.
    pub fn open(
        program_id: ProgramId,
        entry_skill: impl Into<String>,
        inputs: Value,
        substrate_version: impl Into<String>,
        mneme_version: impl Into<String>,
    ) -> Self {
        Self {
            program_id,
            parent_program_id: None,
            entry_skill: entry_skill.into(),
            inputs,
            inputs_schema_version: "0.1.0".to_string(),
            started_at: Utc::now(),
            finished_at: None,
            status: ProgramStatus::Running,
            artifact_schema_version: None,
            substrate_version: substrate_version.into(),
            mneme_version: mneme_version.into(),
            manifest_schema_version: MANIFEST_SCHEMA_VERSION.to_string(),
            depth: 0,
        }
    }

    /// Mark a child relationship; sets parent_program_id and bumps depth.
    pub fn with_parent(mut self, parent: ProgramId, parent_depth: u8) -> Self {
        self.parent_program_id = Some(parent);
        self.depth = parent_depth.saturating_add(1);
        self
    }

    /// Mark the program as completed; sets finished_at and the artifact schema version.
    pub fn complete(&mut self, artifact_schema_version: impl Into<String>) {
        self.finished_at = Some(Utc::now());
        self.status = ProgramStatus::Completed;
        self.artifact_schema_version = Some(artifact_schema_version.into());
    }

    /// Mark the program as failed; sets finished_at and status only.
    pub fn fail(&mut self) {
        self.finished_at = Some(Utc::now());
        self.status = ProgramStatus::Failed;
    }

    /// Total wall-clock duration. None while running.
    pub fn duration(&self) -> Option<chrono::Duration> {
        self.finished_at.map(|end| end - self.started_at)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture() -> Manifest {
        Manifest::open(
            ProgramId::new(),
            "forecast.update",
            json!({"question_id": "Q-001"}),
            "0.6.3",
            "0.1.0",
        )
    }

    #[test]
    fn open_initializes_running() {
        let m = fixture();
        assert_eq!(m.status, ProgramStatus::Running);
        assert!(m.finished_at.is_none());
        assert_eq!(m.depth, 0);
        assert!(m.parent_program_id.is_none());
        assert_eq!(m.manifest_schema_version, MANIFEST_SCHEMA_VERSION);
    }

    #[test]
    fn complete_sets_status_and_artifact_version() {
        let mut m = fixture();
        m.complete("0.1.0");
        assert_eq!(m.status, ProgramStatus::Completed);
        assert_eq!(m.artifact_schema_version.as_deref(), Some("0.1.0"));
        assert!(m.finished_at.is_some());
    }

    #[test]
    fn fail_sets_status_no_artifact_version() {
        let mut m = fixture();
        m.fail();
        assert_eq!(m.status, ProgramStatus::Failed);
        assert!(m.artifact_schema_version.is_none());
        assert!(m.finished_at.is_some());
    }

    #[test]
    fn parent_bumps_depth() {
        let parent = ProgramId::new();
        let m = fixture().with_parent(parent.clone(), 2);
        assert_eq!(m.parent_program_id.as_ref(), Some(&parent));
        assert_eq!(m.depth, 3);
    }

    #[test]
    fn depth_saturates_at_u8_max() {
        let m = fixture().with_parent(ProgramId::new(), u8::MAX);
        assert_eq!(m.depth, u8::MAX);
    }

    #[test]
    fn round_trips_through_serde() {
        let mut m = fixture();
        m.complete("0.1.0");
        let json = serde_json::to_string(&m).unwrap();
        let back: Manifest = serde_json::from_str(&json).unwrap();
        assert_eq!(m.program_id, back.program_id);
        assert_eq!(m.status, back.status);
        assert_eq!(m.artifact_schema_version, back.artifact_schema_version);
    }

    #[test]
    fn duration_is_some_after_complete() {
        let mut m = fixture();
        std::thread::sleep(std::time::Duration::from_millis(2));
        m.complete("0.1.0");
        assert!(m.duration().unwrap().num_milliseconds() >= 1);
    }

    #[test]
    fn duration_is_none_while_running() {
        let m = fixture();
        assert!(m.duration().is_none());
    }
}
