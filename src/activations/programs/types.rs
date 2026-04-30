//! Event and result types for the programs activation.

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::mneme::program::ProgramStatus;

/// One row in a programs.list result.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ProgramSummary {
    pub program_id: String,
    pub parent_program_id: Option<String>,
    pub entry_skill: String,
    pub status: String,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub depth: u8,
}

/// Full program detail returned by programs.inspect.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ProgramDetail {
    pub program_id: String,
    pub manifest: Value,
    pub artifact: Option<Value>,
    pub error: Option<Value>,
    pub trace_count: u32,
}

/// Events from programs.status.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StatusEvent {
    Status { summary: ProgramSummary },
    NotFound { program_id: String },
    Error { message: String },
}

/// Events from programs.list.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ListEvent {
    /// One program. The activation streams these one per item.
    Program { summary: ProgramSummary },
    /// Emitted last; closes the listing.
    Completed { count: u32 },
    Error { message: String },
}

/// Events from programs.inspect.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InspectEvent {
    Detail { detail: ProgramDetail },
    NotFound { program_id: String },
    Error { message: String },
}

/// Events from programs.wait — block-and-stream until a fire-and-return
/// program reaches a terminal state.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WaitEvent {
    /// Emitted on status transitions (running → completed/failed).
    /// The first poll always emits one Progress with the initial status
    /// so consumers know what state the program started in.
    Progress {
        program_id: String,
        status: String,
        age_ms: u64,
    },
    /// Terminal: program completed successfully. Carries the artifact.
    Completed {
        program_id: String,
        artifact: serde_json::Value,
        waited_ms: u64,
    },
    /// Terminal: program failed. Carries the error.
    Failed {
        program_id: String,
        error: serde_json::Value,
        waited_ms: u64,
    },
    /// Terminal: hit timeout cap before reaching a final state.
    TimedOut {
        program_id: String,
        last_status: String,
        waited_ms: u64,
    },
    /// Terminal: the program directory doesn't exist (id wrong, or program
    /// hasn't been opened yet by the substrate).
    NotFound { program_id: String },
}

pub fn status_string(s: ProgramStatus) -> String {
    match s {
        ProgramStatus::Running => "running",
        ProgramStatus::Completed => "completed",
        ProgramStatus::Failed => "failed",
    }
    .to_string()
}

pub fn parse_status(s: &str) -> Option<ProgramStatus> {
    match s {
        "running" => Some(ProgramStatus::Running),
        "completed" => Some(ProgramStatus::Completed),
        "failed" => Some(ProgramStatus::Failed),
        _ => None,
    }
}
