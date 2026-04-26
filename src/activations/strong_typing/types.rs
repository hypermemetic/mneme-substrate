//! Event and state types for the strong_typing activation.
//!
//! `ProposeEvent` variants represent the lifecycle of newtype proposal: started,
//! completed with proposed newtypes and boundary checks, or error at a specific stage.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Events emitted by `strong_typing.propose`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProposeEvent {
    /// Newtype proposal analysis started.
    Started,
    /// Proposals completed; contains suggested newtypes and boundary checks.
    Completed {
        proposals: Vec<Value>,
        boundary_check: Value,
    },
    /// In-band error with the stage it occurred at.
    Error { stage: String, message: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn propose_event_started_serializes() {
        let evt = ProposeEvent::Started;
        let v = serde_json::to_value(&evt).unwrap();
        assert_eq!(v["type"], "started");
    }

    #[test]
    fn propose_event_completed_serializes() {
        let evt = ProposeEvent::Completed {
            proposals: vec![serde_json::json!({"name": "UserId", "base_type": "String"})],
            boundary_check: serde_json::json!({"valid": true}),
        };
        let v = serde_json::to_value(&evt).unwrap();
        assert_eq!(v["type"], "completed");
        assert!(v.get("proposals").is_some());
        assert!(v.get("boundary_check").is_some());
    }

    #[test]
    fn propose_event_error_serializes() {
        let evt = ProposeEvent::Error {
            stage: "analysis".into(),
            message: "codebase parsing failed".into(),
        };
        let v = serde_json::to_value(&evt).unwrap();
        assert_eq!(v["type"], "error");
        assert_eq!(v["stage"], "analysis");
        assert_eq!(v["message"], "codebase parsing failed");
    }
}
