//! Event and state types for the security_review activation.
//!
//! `AuditEvent` variants represent the lifecycle of security review: started,
//! completed with findings grouped by SOC2 control families, or error at a specific stage.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Events emitted by `security_review.audit`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AuditEvent {
    /// Security audit started.
    Started,
    /// Security audit completed; findings grouped by SOC2 control families.
    Completed {
        findings: Vec<Value>,
        summary: Value,
        aggregation_metadata: Value,
    },
    /// In-band error with the stage it occurred at.
    Error { stage: String, message: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_event_started_serializes() {
        let evt = AuditEvent::Started;
        let v = serde_json::to_value(&evt).unwrap();
        assert_eq!(v["type"], "started");
    }

    #[test]
    fn audit_event_completed_serializes() {
        let evt = AuditEvent::Completed {
            findings: vec![serde_json::json!({"family": "CC", "finding": "test"})],
            summary: serde_json::json!({"total_findings": 1}),
            aggregation_metadata: serde_json::json!({"timestamp": "2026-04-25"}),
        };
        let v = serde_json::to_value(&evt).unwrap();
        assert_eq!(v["type"], "completed");
        assert!(v.get("findings").is_some());
        assert!(v.get("summary").is_some());
        assert!(v.get("aggregation_metadata").is_some());
    }

    #[test]
    fn audit_event_error_serializes() {
        let evt = AuditEvent::Error {
            stage: "analysis".into(),
            message: "codebase scan failed".into(),
        };
        let v = serde_json::to_value(&evt).unwrap();
        assert_eq!(v["type"], "error");
        assert_eq!(v["stage"], "analysis");
        assert_eq!(v["message"], "codebase scan failed");
    }
}
