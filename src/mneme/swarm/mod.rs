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

/// Token usage for one trial. Mirrors claudecode::ChatUsage but lives in the
/// swarm namespace so swarm types don't depend on claudecode types. Populated
/// by the swarm runtime from the trial's terminal Complete event when
/// available; absent otherwise.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct TrialUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cost_usd: Option<f64>,
    pub num_turns: Option<i32>,
}

impl TrialUsage {
    /// Sum two usages field-wise. None is treated as 0 for sums; an
    /// all-None summand is the identity. The result has Some(_) for any
    /// field where at least one summand had Some(_).
    pub fn merge(&self, other: &TrialUsage) -> TrialUsage {
        fn add(a: Option<u64>, b: Option<u64>) -> Option<u64> {
            match (a, b) {
                (None, None) => None,
                (Some(x), None) | (None, Some(x)) => Some(x),
                (Some(x), Some(y)) => Some(x + y),
            }
        }
        fn add_f(a: Option<f64>, b: Option<f64>) -> Option<f64> {
            match (a, b) {
                (None, None) => None,
                (Some(x), None) | (None, Some(x)) => Some(x),
                (Some(x), Some(y)) => Some(x + y),
            }
        }
        fn add_i(a: Option<i32>, b: Option<i32>) -> Option<i32> {
            match (a, b) {
                (None, None) => None,
                (Some(x), None) | (None, Some(x)) => Some(x),
                (Some(x), Some(y)) => Some(x + y),
            }
        }
        TrialUsage {
            input_tokens: add(self.input_tokens, other.input_tokens),
            output_tokens: add(self.output_tokens, other.output_tokens),
            cost_usd: add_f(self.cost_usd, other.cost_usd),
            num_turns: add_i(self.num_turns, other.num_turns),
        }
    }
}

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
    /// Token usage for this trial; None if the runtime couldn't capture it
    /// (e.g., mock runtime, claudecode without terminal Complete event).
    #[serde(default)]
    pub usage: Option<TrialUsage>,
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
                usage: None,
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
            usage: Some(TrialUsage {
                input_tokens: Some(1500),
                output_tokens: Some(300),
                cost_usd: Some(0.012),
                num_turns: Some(3),
            }),
        };
        let s = serde_json::to_string(&r).unwrap();
        let back: TrialResult = serde_json::from_str(&s).unwrap();
        assert_eq!(r, back);
    }

    #[test]
    fn usage_merge_sums_fields() {
        let a = TrialUsage {
            input_tokens: Some(100),
            output_tokens: Some(50),
            cost_usd: Some(0.001),
            num_turns: Some(1),
        };
        let b = TrialUsage {
            input_tokens: Some(200),
            output_tokens: None,
            cost_usd: Some(0.002),
            num_turns: Some(2),
        };
        let m = a.merge(&b);
        assert_eq!(m.input_tokens, Some(300));
        assert_eq!(m.output_tokens, Some(50));
        assert!((m.cost_usd.unwrap() - 0.003).abs() < 1e-9);
        assert_eq!(m.num_turns, Some(3));
    }

    #[test]
    fn usage_merge_with_default() {
        let a = TrialUsage {
            input_tokens: Some(10),
            ..Default::default()
        };
        let m = a.merge(&TrialUsage::default());
        assert_eq!(m.input_tokens, Some(10));
        assert_eq!(m.output_tokens, None);
    }
}
