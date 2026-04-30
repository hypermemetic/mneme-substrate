//! Event and state types for the forecast activation.
//!
//! `ForecastState` is the BLF "linguistic belief state" per Murphy 2026 §3:
//! a structured semi-JSON object containing probability, confidence, evidence
//! for/against, and open questions. The summary field is a deterministic
//! prose rendering of the structured fields, retained for backwards-compat
//! and for downstream readers that prefer prose.
//!
//! Schema version: 0.2.0 (was 0.1.0 with just {probability, summary}).
//! Belief-state ablation (paper §1, §3): adding the structured fields recovers
//! ~3.0 BI compared to plain {p, summary}.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Schema version embedded in artifacts written by `forecast.update`.
/// Bump on breaking changes.
///
/// 0.3.0 (MNEME-28): adds `raw_probability` field — the pre-Platt
///   aggregated value, kept alongside the calibrated `probability` for
///   forensic comparison. Older artifacts (no field) still parse.
/// 0.2.0: added `evidence_for / evidence_against / open_questions / confidence`.
/// 0.1.0: original `{probability, summary}` shape.
pub const BELIEF_SCHEMA_VERSION: &str = "0.3.0";

/// Confidence tag on a `ForecastState`.
///
/// Two distinct senses overload this enum today: aggregation provenance
/// (multi-trial vs single-pass) and the model's self-rated certainty
/// (low/medium/high per the paper). For backwards compatibility we keep the
/// aggregation senses as variants and add the paper's three levels alongside.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ForecastConfidence {
    /// Aggregate of multiple independent trials.
    MultiTrial,
    /// Single-trial result; consumer should discount accordingly.
    SinglePass,
    /// Model self-rated low confidence in its estimate.
    Low,
    /// Model self-rated medium confidence.
    Medium,
    /// Model self-rated high confidence.
    High,
}

/// One piece of evidence the model identified, either supporting (`evidence_for`)
/// or contradicting (`evidence_against`) the predicted outcome. Mirrors the
/// paper's structured belief state fields.
///
/// Lenient deserialization: accepts either the full object form
/// `{"claim": "...", "source": "...", "weight": 0.7}` OR a bare string,
/// which is normalized to `{claim: <string>, source: None, weight: 0.5}`.
/// Five of bench-005's six failures were the model emitting evidence as
/// bare strings; the lenient parser eliminates that failure mode without
/// dropping useful data.
#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq)]
pub struct EvidenceItem {
    /// One-sentence claim summarizing the evidence.
    pub claim: String,
    /// Where the evidence came from: a URL, "training", "user-provided", etc.
    /// Optional because trials may produce evidence without explicit attribution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Model's self-rated weight of this evidence in [0, 1].
    /// Higher = more impactful on the probability estimate.
    pub weight: f64,
}

impl<'de> Deserialize<'de> for EvidenceItem {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Helper {
            Bare(String),
            Full {
                claim: String,
                #[serde(default)]
                source: Option<String>,
                #[serde(default = "default_evidence_weight")]
                weight: f64,
            },
        }
        match Helper::deserialize(deserializer)? {
            Helper::Bare(claim) => Ok(EvidenceItem {
                claim,
                source: None,
                weight: default_evidence_weight(),
            }),
            Helper::Full {
                claim,
                source,
                weight,
            } => Ok(EvidenceItem {
                claim,
                source,
                weight,
            }),
        }
    }
}

fn default_evidence_weight() -> f64 {
    0.5
}

/// The BLF belief state — paper §3 "Belief state".
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ForecastState {
    /// Calibrated point estimate in [0, 1]. After MNEME-28, this is the
    /// post-Platt-correction probability when the calibration store has
    /// fit parameters; otherwise equal to `raw_probability`.
    pub probability: f64,
    /// Pre-calibration aggregated probability — what the trials produced
    /// before Platt was applied. Kept alongside `probability` for
    /// forensic comparison and so consumers can re-calibrate against a
    /// later-fit Platt model. None on artifacts written before 0.3.0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_probability: Option<f64>,
    /// Model's self-rated confidence in this estimate.
    pub confidence: ForecastConfidence,
    /// Claims supporting the predicted outcome.
    #[serde(default)]
    pub evidence_for: Vec<EvidenceItem>,
    /// Claims contradicting the predicted outcome.
    #[serde(default)]
    pub evidence_against: Vec<EvidenceItem>,
    /// Things the model would want to know that it doesn't.
    #[serde(default)]
    pub open_questions: Vec<String>,
    /// Deterministic prose rendering of the above. Auto-generated by the
    /// substrate from the structured fields. Older artifacts may have an
    /// LLM-authored summary here.
    pub summary: String,
    /// Aggregation metadata: number of trials that contributed.
    pub n_trials: u8,
    /// The prior this update conditioned on; null on first call.
    #[serde(default)]
    pub prior_used: Option<PriorRef>,
    /// Belief schema version of this state. Readers branch on this.
    #[serde(default = "default_belief_schema_version")]
    pub belief_schema_version: String,
}

fn default_belief_schema_version() -> String {
    "0.1.0".to_string() // for older artifacts that omit the field
}

impl ForecastState {
    /// Render `summary` deterministically from the structured fields.
    /// Used by the substrate after aggregation; ensures consistent prose.
    pub fn render_summary(&self) -> String {
        let mut parts = Vec::new();
        if !self.evidence_for.is_empty() {
            let claims: Vec<String> = self
                .evidence_for
                .iter()
                .map(|e| format!("({:.2}) {}", e.weight, e.claim))
                .collect();
            parts.push(format!("FOR: {}", claims.join("; ")));
        }
        if !self.evidence_against.is_empty() {
            let claims: Vec<String> = self
                .evidence_against
                .iter()
                .map(|e| format!("({:.2}) {}", e.weight, e.claim))
                .collect();
            parts.push(format!("AGAINST: {}", claims.join("; ")));
        }
        if !self.open_questions.is_empty() {
            parts.push(format!("OPEN: {}", self.open_questions.join("; ")));
        }
        parts.join(" || ")
    }

    /// Refresh `summary` from the current structured fields.
    pub fn rerender_summary(&mut self) {
        self.summary = self.render_summary();
    }
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

/// Per-trial response shape — what each fan-out trial returns.
///
/// This is the schema demanded by the SKILL.md prompt. Trials produce JSON
/// matching this shape; the substrate parses it into ForecastState fragments
/// before aggregation.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct TrialResponse {
    pub probability: f64,
    /// Optional in v0.2.0: trials may produce structured fields and skip
    /// the prose summary. The substrate auto-renders the summary at
    /// aggregation time.
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub confidence: Option<ForecastConfidence>,
    #[serde(default)]
    pub evidence_for: Vec<EvidenceItem>,
    #[serde(default)]
    pub evidence_against: Vec<EvidenceItem>,
    #[serde(default)]
    pub open_questions: Vec<String>,
}

/// Events emitted by `forecast.create`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CreateEvent {
    Created {
        program_id: String,
        question: String,
        deadline: String,
    },
    ResolvabilityFailed { reason: String },
    Error { message: String },
}

/// Events emitted by `forecast.update`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UpdateEvent {
    Started {
        program_id: String,
        prior: ForecastState,
    },
    ResolvabilityFailed { reason: String },
    TrialProgress {
        trial_index: u8,
        total: u8,
        status: String,
    },
    Aggregated {
        aggregated: ForecastState,
        raw_trials: Vec<TrialResponse>,
    },
    Completed {
        program_id: String,
        state: ForecastState,
        artifact_path: String,
    },
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
    Recalibrated { a: f64, b: f64 },
    Error { message: String },
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ev(claim: &str, weight: f64) -> EvidenceItem {
        EvidenceItem {
            claim: claim.into(),
            source: None,
            weight,
        }
    }

    #[test]
    fn evidence_item_accepts_bare_string() {
        // bench-005 failure pattern: model emitted evidence as a bare string
        // instead of an object. Lenient parser must accept and normalize.
        let s = r#""Market price frozen at 0.02""#;
        let item: EvidenceItem = serde_json::from_str(s).unwrap();
        assert_eq!(item.claim, "Market price frozen at 0.02");
        assert!(item.source.is_none());
        assert_eq!(item.weight, 0.5);
    }

    #[test]
    fn evidence_item_accepts_full_object() {
        let s = r#"{"claim": "X happened", "source": "url", "weight": 0.8}"#;
        let item: EvidenceItem = serde_json::from_str(s).unwrap();
        assert_eq!(item.claim, "X happened");
        assert_eq!(item.source.as_deref(), Some("url"));
        assert_eq!(item.weight, 0.8);
    }

    #[test]
    fn evidence_item_accepts_object_missing_weight() {
        let s = r#"{"claim": "X happened"}"#;
        let item: EvidenceItem = serde_json::from_str(s).unwrap();
        assert_eq!(item.claim, "X happened");
        assert_eq!(item.weight, 0.5);
    }

    #[test]
    fn evidence_list_mixed_string_and_object() {
        // Realistic failure shape from bench-005: model mixed forms.
        let s = r#"[
            {"claim": "structured one", "weight": 0.7},
            "bare string two",
            {"claim": "structured three", "source": "url", "weight": 0.6}
        ]"#;
        let items: Vec<EvidenceItem> = serde_json::from_str(s).unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[1].claim, "bare string two");
        assert_eq!(items[1].weight, 0.5);
    }

    fn structured_state() -> ForecastState {
        ForecastState {
            probability: 0.42,
            raw_probability: Some(0.42),
            confidence: ForecastConfidence::Medium,
            evidence_for: vec![ev("X is happening", 0.7), ev("market signal positive", 0.5)],
            evidence_against: vec![ev("macro headwind", 0.6)],
            open_questions: vec!["regulatory outcome".into()],
            summary: String::new(),
            n_trials: 3,
            prior_used: Some(PriorRef {
                probability: 0.3,
                summary: "prior evidence".into(),
            }),
            belief_schema_version: BELIEF_SCHEMA_VERSION.into(),
        }
    }

    #[test]
    fn forecast_state_round_trips_full_shape() {
        let state = structured_state();
        let json = serde_json::to_string(&state).unwrap();
        let back: ForecastState = serde_json::from_str(&json).unwrap();
        assert_eq!(back.probability, 0.42);
        assert_eq!(back.confidence, ForecastConfidence::Medium);
        assert_eq!(back.evidence_for.len(), 2);
        assert_eq!(back.evidence_against.len(), 1);
        assert_eq!(back.open_questions.len(), 1);
        assert_eq!(back.belief_schema_version, BELIEF_SCHEMA_VERSION);
    }

    #[test]
    fn forecast_state_old_v01_artifact_still_parses() {
        // Older artifacts are missing the new fields; serde defaults handle them.
        let old_json = r#"{
            "probability": 0.5,
            "summary": "old",
            "confidence": "multi-trial",
            "n_trials": 2
        }"#;
        let state: ForecastState = serde_json::from_str(old_json).unwrap();
        assert_eq!(state.probability, 0.5);
        assert!(state.evidence_for.is_empty());
        assert!(state.evidence_against.is_empty());
        assert!(state.open_questions.is_empty());
        assert_eq!(state.belief_schema_version, "0.1.0");
    }

    #[test]
    fn render_summary_deterministic_and_parseable() {
        let state = structured_state();
        let s = state.render_summary();
        assert!(s.contains("FOR:"));
        assert!(s.contains("AGAINST:"));
        assert!(s.contains("OPEN:"));
        assert!(s.contains("X is happening"));
        // Render twice: identical output.
        let s2 = state.render_summary();
        assert_eq!(s, s2);
    }

    #[test]
    fn render_summary_handles_empty_fields() {
        let mut s = structured_state();
        s.evidence_against.clear();
        s.open_questions.clear();
        let rendered = s.render_summary();
        assert!(rendered.contains("FOR:"));
        assert!(!rendered.contains("AGAINST:"));
        assert!(!rendered.contains("OPEN:"));
    }

    #[test]
    fn render_summary_completely_empty() {
        let s = ForecastState {
            probability: 0.5,
            raw_probability: None,
            confidence: ForecastConfidence::SinglePass,
            evidence_for: vec![],
            evidence_against: vec![],
            open_questions: vec![],
            summary: String::new(),
            n_trials: 1,
            prior_used: None,
            belief_schema_version: BELIEF_SCHEMA_VERSION.into(),
        };
        assert_eq!(s.render_summary(), "");
    }

    #[test]
    fn rerender_summary_updates_field() {
        let mut s = structured_state();
        s.summary = "stale".into();
        s.rerender_summary();
        assert_ne!(s.summary, "stale");
        assert!(s.summary.contains("FOR:"));
    }

    #[test]
    fn confidence_serializes_kebab_case() {
        assert_eq!(serde_json::to_string(&ForecastConfidence::MultiTrial).unwrap(), "\"multi-trial\"");
        assert_eq!(serde_json::to_string(&ForecastConfidence::SinglePass).unwrap(), "\"single-pass\"");
        assert_eq!(serde_json::to_string(&ForecastConfidence::Low).unwrap(), "\"low\"");
        assert_eq!(serde_json::to_string(&ForecastConfidence::Medium).unwrap(), "\"medium\"");
        assert_eq!(serde_json::to_string(&ForecastConfidence::High).unwrap(), "\"high\"");
    }

    #[test]
    fn evidence_item_round_trips() {
        let e = EvidenceItem {
            claim: "claim".into(),
            source: Some("https://example.com".into()),
            weight: 0.7,
        };
        let json = serde_json::to_string(&e).unwrap();
        let back: EvidenceItem = serde_json::from_str(&json).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn evidence_item_omits_none_source() {
        let e = ev("claim", 0.5);
        let v = serde_json::to_value(&e).unwrap();
        assert!(v.get("source").is_none(), "source should be omitted when None");
    }

    #[test]
    fn trial_response_accepts_minimal_v01_shape() {
        // Backwards compat: trials still able to return {probability, summary}
        // get a default ForecastState with empty structured fields.
        let json = r#"{"probability": 0.5, "summary": "ok"}"#;
        let r: TrialResponse = serde_json::from_str(json).unwrap();
        assert_eq!(r.probability, 0.5);
        assert!(r.evidence_for.is_empty());
    }

    #[test]
    fn trial_response_accepts_full_v02_shape() {
        let json = r#"{
            "probability": 0.6,
            "confidence": "high",
            "evidence_for": [{"claim": "c1", "weight": 0.7}],
            "evidence_against": [],
            "open_questions": ["q1"]
        }"#;
        let r: TrialResponse = serde_json::from_str(json).unwrap();
        assert_eq!(r.probability, 0.6);
        assert_eq!(r.evidence_for.len(), 1);
        assert_eq!(r.open_questions, vec!["q1".to_string()]);
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
        let _ = json!({});
    }
}
