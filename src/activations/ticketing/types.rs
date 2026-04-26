//! Event and state types for the ticketing activation.
//!
//! `WriteEvent` variants represent the lifecycle of ticket creation: started,
//! completed with metadata and file path, or error at a specific stage.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[cfg(test)]
use serde_json::json;

/// Events emitted by `ticketing.write`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WriteEvent {
    /// Ticket creation started.
    Started,
    /// Ticket successfully written to disk.
    Completed {
        id: String,
        frontmatter: Value,
        body: String,
        meta_checks: Value,
        file_path: String,
    },
    /// In-band error with the stage it occurred at.
    Error { stage: String, message: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_event_started_serializes() {
        let evt = WriteEvent::Started;
        let v = serde_json::to_value(&evt).unwrap();
        assert_eq!(v["type"], "started");
    }

    #[test]
    fn write_event_completed_serializes() {
        let evt = WriteEvent::Completed {
            id: "TICK-001".into(),
            frontmatter: json!({"epic": "ticketing"}),
            body: "Implementation plan".into(),
            meta_checks: json!({}),
            file_path: "/tmp/TICK-001.md".into(),
        };
        let v = serde_json::to_value(&evt).unwrap();
        assert_eq!(v["type"], "completed");
        assert_eq!(v["id"], "TICK-001");
        assert!(v.get("frontmatter").is_some());
        assert!(v.get("body").is_some());
        assert!(v.get("meta_checks").is_some());
        assert!(v.get("file_path").is_some());
    }

    #[test]
    fn write_event_error_serializes() {
        let evt = WriteEvent::Error {
            stage: "validation".into(),
            message: "invalid epic name".into(),
        };
        let v = serde_json::to_value(&evt).unwrap();
        assert_eq!(v["type"], "error");
        assert_eq!(v["stage"], "validation");
        assert_eq!(v["message"], "invalid epic name");
    }
}
