//! ForecastBench data types — minimal fields needed for question rendering
//! + resolution-joining. Fields we don't use are deserialized as-is via
//! `serde::Value` so we round-trip the file shape without committing to it.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A question's id can be either a single string (normal market or dataset
/// question) or an array of strings (combination question — predicts the
/// joint probability of two underlying markets). Phase 1 loader skips
/// combinations.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(untagged)]
pub enum FBQuestionId {
    Single(String),
    Combo(Vec<String>),
}

impl FBQuestionId {
    pub fn as_single(&self) -> Option<&str> {
        match self {
            FBQuestionId::Single(s) => Some(s.as_str()),
            FBQuestionId::Combo(_) => None,
        }
    }
    pub fn is_combo(&self) -> bool {
        matches!(self, FBQuestionId::Combo(_))
    }
}

/// Top-level shape of `<DATE>-llm.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FBQuestionSet {
    pub forecast_due_date: String,
    pub question_set: String,
    pub questions: Vec<FBQuestion>,
}

/// One question. Market and dataset questions share this shape; some fields
/// are populated only for one or the other. Combination questions (where
/// `id` is an array of constituent ids) are deserialized into the Combo
/// variant and the loader skips them.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FBQuestion {
    pub id: FBQuestionId,
    pub source: String,
    pub question: String,
    pub resolution_criteria: String,
    #[serde(default)]
    pub background: Option<String>,
    pub url: String,
    pub freeze_datetime: String,
    pub freeze_datetime_value: String,
    pub freeze_datetime_value_explanation: String,
    /// Market questions: ISO 8601. Dataset questions: "N/A".
    #[serde(default)]
    pub market_info_resolution_datetime: Option<String>,
    /// Dataset questions only: list of horizons in days.
    #[serde(default)]
    pub forecast_horizons: Vec<i64>,
}

/// Top-level shape of `<DATE>_resolution_set.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FBResolutionSet {
    pub forecast_due_date: String,
    pub question_set: String,
    pub resolutions: Vec<FBResolution>,
}

/// One resolution row. For dataset questions there are multiple resolutions
/// per `id` (one per horizon); the join key is `(id, resolution_date)`.
/// Combination resolutions also carry array-form ids.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FBResolution {
    pub id: FBQuestionId,
    pub source: String,
    /// Dataset questions: "up"/"down" (single) or array of those for combos.
    /// Market questions: null. Kept as opaque Value since we don't score on it
    /// at Phase 1 (markets-only).
    #[serde(default)]
    pub direction: Option<Value>,
    /// ISO 8601 date (YYYY-MM-DD).
    pub resolution_date: String,
    /// Resolved probability in `[0, 1]` (markets can resolve fractionally on
    /// ambiguous outcomes). Use directly for Brier; threshold at 0.5 when
    /// persisting to `CalibrationStore`.
    pub resolved_to: f64,
    pub resolved: bool,
}

/// A market question paired with its single resolution. Produced by
/// [`super::loader::join_market_questions`]. Dataset questions are
/// excluded from this shape (see Phase 1 scope in `mod.rs` doc).
#[derive(Debug, Clone)]
pub struct MarketQuestionWithResolution {
    pub question: FBQuestion,
    pub resolution: FBResolution,
    pub resolution_datetime_utc: DateTime<Utc>,
}

/// True if this source is a market (one question → one resolution).
pub fn is_market_source(source: &str) -> bool {
    matches!(source, "manifold" | "metaculus" | "polymarket" | "infer")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn market_source_detection() {
        for s in &["manifold", "metaculus", "polymarket", "infer"] {
            assert!(is_market_source(s), "{} should be market", s);
        }
        for s in &["acled", "fred", "yfinance", "wikipedia", "dbnomics"] {
            assert!(!is_market_source(s), "{} should NOT be market", s);
        }
    }

    #[test]
    fn fb_question_round_trips_minimal_shape() {
        let json = r#"{
            "id": "abc",
            "source": "manifold",
            "question": "Will X?",
            "resolution_criteria": "Resolves YES if X.",
            "url": "https://example.com",
            "freeze_datetime": "2024-07-12T00:00:00+00:00",
            "freeze_datetime_value": "0.5",
            "freeze_datetime_value_explanation": "market value"
        }"#;
        let q: FBQuestion = serde_json::from_str(json).unwrap();
        assert_eq!(q.id.as_single(), Some("abc"));
        assert_eq!(q.source, "manifold");
        assert!(q.background.is_none());
        assert!(q.market_info_resolution_datetime.is_none());
        assert!(q.forecast_horizons.is_empty());
    }

    #[test]
    fn fb_resolution_round_trips() {
        let json = r#"{"id":"abc","source":"manifold","direction":null,"resolution_date":"2024-12-31","resolved_to":1.0,"resolved":true}"#;
        let r: FBResolution = serde_json::from_str(json).unwrap();
        assert_eq!(r.id.as_single(), Some("abc"));
        assert_eq!(r.resolved_to, 1.0);
        assert!(r.resolved);
    }

    #[test]
    fn fb_question_id_accepts_array_for_combination() {
        let json = r#"{
            "id": ["a", "b"],
            "source": "manifold",
            "question": "Will both A and B?",
            "resolution_criteria": "...",
            "url": "https://e.x",
            "freeze_datetime": "2024-07-12T00:00:00+00:00",
            "freeze_datetime_value": "0.5",
            "freeze_datetime_value_explanation": "..."
        }"#;
        let q: FBQuestion = serde_json::from_str(json).unwrap();
        assert!(q.id.is_combo());
        assert!(q.id.as_single().is_none());
    }
}
