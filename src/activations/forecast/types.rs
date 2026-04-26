//! Event and state types for the forecast activation.
//!
//! `ForecastState` is the BLF "linguistic belief state": a `(probability,
//! summary)` pair plus metadata (confidence, n_trials, prior). The summary
//! is the sufficient statistic that survives between updates.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Confidence tag on a `ForecastState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ForecastConfidence {
    /// Result is the aggregate of multiple independent trials.
    MultiTrial,
    /// Single-trial result; consumer should discount accordingly.
    SinglePass,
}

/// The BLF belief state.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ForecastState {
    /// Point estimate in [0, 1].
    pub probability: f64,
    /// Linguistic belief — the prose justification carried into the next update.
    pub summary: String,
    pub confidence: ForecastConfidence,
    /// Number of trials that contributed to this state.
    pub n_trials: u8,
    /// The prior this update conditioned on; null on first call.
    pub prior_used: Option<PriorRef>,
}

/// Compact reference to the prior state that an update conditioned on.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PriorRef {
    pub probability: f64,
    pub summary: String,
}

impl From<&ForecastState> for PriorRef {
    fn from(state: &ForecastState) -> Self {
        Self {
            probability: state.probability,
            summary: state.summary.clone(),
        }
    }
}

/// Per-trial response shape — what each fan-out trial returns via `respond`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TrialResponse {
    pub probability: f64,
    pub summary: String,
}

/// Events emitted by `forecast.create`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CreateEvent {
    /// Question was registered; returned on first call.
    Created {
        program_id: String,
        question: String,
        deadline: String,
    },
    /// The question failed the resolvability gate.
    ResolvabilityFailed { reason: String },
    /// In-band error.
    Error { message: String },
}

/// Events emitted by `forecast.update`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UpdateEvent {
    /// Update started; carries the prior state used.
    Started {
        program_id: String,
        prior: ForecastState,
    },
    /// The deadline has passed; the forecast can't be updated.
    ResolvabilityFailed { reason: String },
    /// One trial reported progress (optional, for streaming UX).
    TrialProgress {
        trial_index: u8,
        total: u8,
        status: String,
    },
    /// Aggregation complete; carries both the aggregated state and per-trial raw results.
    Aggregated {
        aggregated: ForecastState,
        raw_trials: Vec<TrialResponse>,
    },
    /// Update finished; artifact is on disk.
    Completed {
        program_id: String,
        state: ForecastState,
        artifact_path: String,
    },
    /// In-band error with the stage it occurred at.
    Error { stage: String, message: String },
}

/// Events emitted by `forecast.resolve`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResolveEvent {
    Resolved {
        program_id: String,
        predicted: f64,
        actual: bool,
    },
    /// New calibration parameters were fit because we crossed COLD_START_THRESHOLD.
    Recalibrated { a: f64, b: f64 },
    Error { message: String },
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn forecast_state_round_trips() {
        let state = ForecastState {
            probability: 0.42,
            summary: "evidence of X".to_string(),
            confidence: ForecastConfidence::MultiTrial,
            n_trials: 3,
            prior_used: Some(PriorRef {
                probability: 0.3,
                summary: "prior evidence".to_string(),
            }),
        };
        let json = serde_json::to_string(&state).unwrap();
        let back: ForecastState = serde_json::from_str(&json).unwrap();
        assert_eq!(back.probability, 0.42);
        assert_eq!(back.confidence, ForecastConfidence::MultiTrial);
        assert_eq!(back.n_trials, 3);
        assert!(back.prior_used.is_some());
    }

    #[test]
    fn confidence_serializes_kebab_case() {
        let s = serde_json::to_string(&ForecastConfidence::MultiTrial).unwrap();
        assert_eq!(s, "\"multi-trial\"");
        let s = serde_json::to_string(&ForecastConfidence::SinglePass).unwrap();
        assert_eq!(s, "\"single-pass\"");
    }

    #[test]
    fn update_event_tagged_serialization() {
        let evt = UpdateEvent::TrialProgress {
            trial_index: 2,
            total: 5,
            status: "running".into(),
        };
        let v = serde_json::to_value(&evt).unwrap();
        assert_eq!(v["type"], "trial_progress");
        assert_eq!(v["trial_index"], 2);
    }

    #[test]
    fn prior_ref_from_state() {
        let state = ForecastState {
            probability: 0.7,
            summary: "x".into(),
            confidence: ForecastConfidence::SinglePass,
            n_trials: 1,
            prior_used: None,
        };
        let pr: PriorRef = (&state).into();
        assert_eq!(pr.probability, 0.7);
        assert_eq!(pr.summary, "x");
    }

    #[test]
    fn trial_response_matches_aggregation_field_names() {
        // The aggregate logit_shrinkage rule reads `field` from each trial's
        // response. forecast uses field="probability" on TrialResponse.
        let r = TrialResponse {
            probability: 0.5,
            summary: "ok".into(),
        };
        let v = serde_json::to_value(&r).unwrap();
        assert!(v.get("probability").is_some());
        assert!(v.get("summary").is_some());
    }

    #[test]
    fn create_event_variants_serialize() {
        let evts = vec![
            CreateEvent::Created {
                program_id: "p1".into(),
                question: "Will X?".into(),
                deadline: "2026-12-31".into(),
            },
            CreateEvent::ResolvabilityFailed {
                reason: "deadline in past".into(),
            },
            CreateEvent::Error {
                message: "boom".into(),
            },
        ];
        for e in evts {
            let v = serde_json::to_value(&e).unwrap();
            assert!(v.get("type").is_some(), "missing type tag in {:?}", v);
        }
        // Spot-check tags.
        let v = serde_json::to_value(CreateEvent::ResolvabilityFailed {
            reason: "x".into(),
        })
        .unwrap();
        assert_eq!(v["type"], "resolvability_failed");
        let _ = json!({}); // suppress unused import in some toolchains
    }
}
