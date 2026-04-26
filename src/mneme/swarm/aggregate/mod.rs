//! Aggregation rules — combine N trial responses into one.
//!
//! Each rule is pure: takes `Vec<Value>` (per-trial responses) and returns one
//! `Value`. No LLM in the loop. Determinism + testability.
//!
//! Four rules in MVP:
//! - [`logit::logit_shrinkage`] — Bayesian shrinkage toward a prior
//! - [`concat::concat_evidence`] — join per-trial summaries with attribution
//! - [`majority::majority_enum`] — most-common discrete value
//! - [`severity::max_severity`] — max along a ladder (e.g., security findings)

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub mod concat;
pub mod logit;
pub mod majority;
pub mod severity;

/// Errors raised by aggregation.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum AggregateError {
    #[error("trials list is empty")]
    Empty,
    #[error("trial {trial_index} is missing field `{field}`")]
    MissingField { trial_index: usize, field: String },
    #[error("trial {trial_index} field `{field}` has wrong type: {detail}")]
    WrongType {
        trial_index: usize,
        field: String,
        detail: String,
    },
    #[error("invalid parameter: {0}")]
    InvalidParam(String),
}

/// Tagged enum of supported aggregation rules. The skill picks one when calling
/// `swarm.aggregate`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "rule", rename_all = "snake_case")]
pub enum AggregationRule {
    /// Logit-space Bayesian shrinkage toward a prior. For probabilities in (0, 1).
    LogitShrinkage {
        /// Field name in each trial whose value is the probability.
        field: String,
        /// Prior probability in (0, 1).
        prior: f64,
        /// Shrinkage weight in [0, 1]. 0 = trial mean only; 1 = prior only.
        lambda: f64,
    },
    /// Concatenate per-trial string summaries with attribution.
    ConcatEvidence {
        field: String,
        /// Separator inserted between trials (e.g., "\n\n").
        separator: String,
    },
    /// Majority vote on a discrete string-valued field.
    MajorityEnum { field: String },
    /// Maximum along a severity ladder.
    MaxSeverity {
        field: String,
        /// Ordered ladder; later entries are higher severity.
        ladder: Vec<String>,
    },
}

/// Apply an aggregation rule to a batch of trial responses.
pub fn aggregate(trials: &[Value], rule: &AggregationRule) -> Result<Value, AggregateError> {
    if trials.is_empty() {
        return Err(AggregateError::Empty);
    }
    match rule {
        AggregationRule::LogitShrinkage {
            field,
            prior,
            lambda,
        } => logit::logit_shrinkage(trials, field, *prior, *lambda),
        AggregationRule::ConcatEvidence { field, separator } => {
            concat::concat_evidence(trials, field, separator)
        }
        AggregationRule::MajorityEnum { field } => majority::majority_enum(trials, field),
        AggregationRule::MaxSeverity { field, ladder } => {
            severity::max_severity(trials, field, ladder)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn empty_trials_errors() {
        let rule = AggregationRule::ConcatEvidence {
            field: "x".into(),
            separator: ", ".into(),
        };
        assert_eq!(aggregate(&[], &rule), Err(AggregateError::Empty));
    }

    #[test]
    fn dispatches_to_logit() {
        let trials = vec![json!({"p": 0.5}), json!({"p": 0.5})];
        let rule = AggregationRule::LogitShrinkage {
            field: "p".into(),
            prior: 0.3,
            lambda: 0.0,
        };
        let result = aggregate(&trials, &rule).unwrap();
        // No prior pull (lambda=0): aggregated should equal raw mean (0.5).
        assert!((result["aggregated"].as_f64().unwrap() - 0.5).abs() < 1e-9);
    }

    #[test]
    fn rule_round_trips_through_serde() {
        let rule = AggregationRule::LogitShrinkage {
            field: "probability".into(),
            prior: 0.5,
            lambda: 0.2,
        };
        let json = serde_json::to_string(&rule).unwrap();
        let back: AggregationRule = serde_json::from_str(&json).unwrap();
        match back {
            AggregationRule::LogitShrinkage { field, prior, lambda } => {
                assert_eq!(field, "probability");
                assert_eq!(prior, 0.5);
                assert_eq!(lambda, 0.2);
            }
            _ => panic!("wrong variant"),
        }
    }
}
