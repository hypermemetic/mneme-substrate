//! Swarm — orchestration primitives.
//!
//! Layer-1 building blocks the skill activations use:
//!
//! - [`aggregate`] — pure aggregation rules (logit shrinkage, concat evidence,
//!   majority enum, max severity). No LLM in the loop; deterministic math.
//! - The swarm activation itself (substrate-side; Phase 2) wraps `claudecode`
//!   `fork` + `chat_async` + `poll` to fan out and gather typed responses.
//!
//! Phase 1 ships only the pure types and aggregation math. The substrate-side
//! `SwarmRuntime` (which actually drives claudecode) lands in Phase 2.

pub mod aggregate;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Per-trial result from a swarm fan-out.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TrialResult {
    /// Index in the original fan-out (0-based).
    pub trial_index: u8,
    /// claudecode session id this trial ran in (for audit).
    pub session_id: String,
    /// The trial's typed response (matches the swarm.trial response_schema).
    pub response: Value,
    /// Wall-clock duration in milliseconds.
    pub duration_ms: u64,
}

/// Failure record for a trial that didn't produce a valid response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TrialFailure {
    pub trial_index: u8,
    pub session_id: Option<String>,
    pub error: String,
    /// The last payload Claude tried (for `respond` validation failures).
    pub last_payload: Option<Value>,
}

/// Outcome of a complete fan-out: the successes and failures.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TrialBatch {
    pub successes: Vec<TrialResult>,
    pub failures: Vec<TrialFailure>,
}

impl TrialBatch {
    pub fn empty() -> Self {
        Self {
            successes: Vec::new(),
            failures: Vec::new(),
        }
    }

    pub fn success_count(&self) -> usize {
        self.successes.len()
    }

    pub fn failure_count(&self) -> usize {
        self.failures.len()
    }

    pub fn is_empty(&self) -> bool {
        self.successes.is_empty() && self.failures.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn trial_batch_counts() {
        let batch = TrialBatch {
            successes: vec![TrialResult {
                trial_index: 0,
                session_id: "s0".into(),
                response: json!({"p": 0.5}),
                duration_ms: 100,
            }],
            failures: vec![
                TrialFailure {
                    trial_index: 1,
                    session_id: Some("s1".into()),
                    error: "schema mismatch".into(),
                    last_payload: Some(json!({})),
                },
                TrialFailure {
                    trial_index: 2,
                    session_id: None,
                    error: "timeout".into(),
                    last_payload: None,
                },
            ],
        };
        assert_eq!(batch.success_count(), 1);
        assert_eq!(batch.failure_count(), 2);
        assert!(!batch.is_empty());
    }

    #[test]
    fn trial_result_round_trips() {
        let r = TrialResult {
            trial_index: 3,
            session_id: "abc".into(),
            response: json!({"probability": 0.42}),
            duration_ms: 1234,
        };
        let s = serde_json::to_string(&r).unwrap();
        let back: TrialResult = serde_json::from_str(&s).unwrap();
        assert_eq!(r, back);
    }
}
