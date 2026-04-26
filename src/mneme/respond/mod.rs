//! Respond — structured-output protocol via per-program loopback tools.
//!
//! When a skill activation needs Claude to return a typed value, it registers
//! a `respond` tool whose input schema matches the desired output. The session
//! is constrained (`disallowed_tools` excludes the natural exit path) so Claude
//! must call `respond` before terminating; the tool-call payload IS the
//! structured response.
//!
//! Phase 1 ships only the data types. The substrate-side registration code
//! (which actually wires into the loopback MCP) lands in Phase 2 and is gated
//! on MNEME-S01 spike.

pub mod schema;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::mneme::program::ProgramId;

pub use schema::{validate, SchemaError};

/// Errors raised by the respond protocol.
#[derive(Debug, thiserror::Error)]
pub enum RespondError {
    #[error("response failed schema validation after {attempts} attempts: {last_error}")]
    Validation {
        attempts: u8,
        last_payload: Option<Value>,
        last_error: String,
    },
    #[error("timed out waiting for respond call after {seconds} seconds")]
    Timeout { seconds: u32 },
    #[error("schema error: {0}")]
    Schema(#[from] SchemaError),
    #[error("internal: {0}")]
    Internal(String),
}

/// Description of a `respond` tool registered with the substrate's loopback MCP.
///
/// Tool name is namespaced by program id so concurrent programs don't collide.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RespondTool {
    /// Program this tool serves.
    pub program_id: ProgramId,
    /// JSON Schema constraining the tool's input (== the skill's output schema).
    pub schema: Value,
    /// How many invalid payloads Claude is allowed before we give up.
    pub max_attempts: u8,
    /// Wall-clock timeout for the entire respond cycle.
    pub timeout_seconds: u32,
}

impl RespondTool {
    /// Default attempts (3) and timeout (60s). Skills may tune.
    pub fn new(program_id: ProgramId, schema: Value) -> Self {
        Self {
            program_id,
            schema,
            max_attempts: 3,
            timeout_seconds: 60,
        }
    }

    /// Tool name as exposed to Claude. Includes program id for isolation.
    pub fn tool_name(&self) -> String {
        format!("respond_{}", self.program_id)
    }

    pub fn with_max_attempts(mut self, n: u8) -> Self {
        self.max_attempts = n;
        self
    }

    pub fn with_timeout_seconds(mut self, secs: u32) -> Self {
        self.timeout_seconds = secs;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn schema() -> Value {
        json!({
            "type": "object",
            "properties": {
                "value": {"type": "integer", "minimum": 0, "maximum": 10}
            },
            "required": ["value"]
        })
    }

    #[test]
    fn tool_name_includes_program_id() {
        let id = ProgramId::new();
        let tool = RespondTool::new(id.clone(), schema());
        let name = tool.tool_name();
        assert!(name.starts_with("respond_"));
        assert!(name.contains(&id.as_str()));
    }

    #[test]
    fn defaults_are_sane() {
        let tool = RespondTool::new(ProgramId::new(), schema());
        assert_eq!(tool.max_attempts, 3);
        assert_eq!(tool.timeout_seconds, 60);
    }

    #[test]
    fn builders_chain() {
        let tool = RespondTool::new(ProgramId::new(), schema())
            .with_max_attempts(5)
            .with_timeout_seconds(120);
        assert_eq!(tool.max_attempts, 5);
        assert_eq!(tool.timeout_seconds, 120);
    }
}
