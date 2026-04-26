//! Event and state types for the planning activation.
//!
//! `EpicEvent` variants represent the lifecycle of epic decomposition: started,
//! completed with DAG structure and child program IDs, or error at a specific stage.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Events emitted by `planning.epic`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EpicEvent {
    /// Epic decomposition started.
    Started,
    /// Epic successfully decomposed into tickets and written to disk.
    Completed {
        epic_id: String,
        overview: Value,
        ticket_count: u32,
        dag_check: Value,
        child_program_ids: Vec<String>,
    },
    /// In-band error with the stage it occurred at.
    Error { stage: String, message: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epic_event_started_serializes() {
        let evt = EpicEvent::Started;
        let v = serde_json::to_value(&evt).unwrap();
        assert_eq!(v["type"], "started");
    }

    #[test]
    fn epic_event_completed_serializes() {
        let evt = EpicEvent::Completed {
            epic_id: "EPIC-001".into(),
            overview: serde_json::json!({"goal": "Implement feature X"}),
            ticket_count: 5,
            dag_check: serde_json::json!({"valid": true}),
            child_program_ids: vec!["EPIC-002".into(), "EPIC-003".into()],
        };
        let v = serde_json::to_value(&evt).unwrap();
        assert_eq!(v["type"], "completed");
        assert_eq!(v["epic_id"], "EPIC-001");
        assert_eq!(v["ticket_count"], 5);
        assert!(v.get("overview").is_some());
        assert!(v.get("dag_check").is_some());
        assert!(v.get("child_program_ids").is_some());
    }

    #[test]
    fn epic_event_error_serializes() {
        let evt = EpicEvent::Error {
            stage: "decomposition".into(),
            message: "goal is too vague".into(),
        };
        let v = serde_json::to_value(&evt).unwrap();
        assert_eq!(v["type"], "error");
        assert_eq!(v["stage"], "decomposition");
        assert_eq!(v["message"], "goal is too vague");
    }
}
