//! BLFX iterative agent loop primitives.
//!
//! Per Murphy 2026 Algorithm 1, each trial is an iterative loop where the LLM
//! at each step produces both an action and an updated belief in a single
//! response. The substrate parses, executes the action, appends the result to
//! history, and continues until `submit` or T_max steps.
//!
//! This module defines:
//! - [`Action`] enum — the universal subset of BLF actions plus stubs for
//!   source-specific tools.
//! - [`Observation`] enum — the result of executing an action.
//! - [`parse_step`] — parser that takes the LLM's response text and returns
//!   `(Action, ForecastState)`.
//! - [`execute_action`] — async executor (delegates to claudecode tools or
//!   to a search backend, depending on the action).
//!
//! ## Action format choice (pending BLFX-S02)
//!
//! This implementation uses **custom JSON** rather than Claude's native
//! tool-use. The LLM appends a fenced JSON block at the end of its response:
//!
//! ```json
//! {
//!   "action": {"type": "web_search", "query": "...", "k": 5},
//!   "belief": { /* ForecastState fields */ }
//! }
//! ```
//!
//! The parser extracts both. This pattern matches what BLFX-2 already does
//! for the final response — same shape, just nested. If BLFX-S02 finds
//! native tool-use is meaningfully better, swapping the parser is local.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::types::TrialResponse;

/// Per-trial environment context — the date-leakage defenses (BLFX-9).
///
/// `cutoff_date` is the question's freeze date; the substrate enforces
/// that no information after this date leaks into the trial. When `None`
/// (production forecasting), all layers are no-ops and behavior matches
/// pre-BLFX-9. Constructed by `forecast.update` from the caller's
/// optional `cutoff_date` param and threaded through to the step driver.
///
/// Layer mapping:
/// 1. **Search engine date filtering** — [`apply_search_query_date_filter`]
///    appends a `before:YYYY-MM-DD` clause to outbound web search queries.
/// 2. **LLM-based leak classifier** — when [`leak_classifier`] is `Some`,
///    every search hit is run through it; flagged hits are dropped.
/// 3. **Data tool date clamping** — `FetchTimeSeries` /
///    `FetchWikipediaSection` arms read `cutoff_date` and truncate to
///    that point. (Stubbed actions today; gated through this struct
///    when implemented.)
/// 4. **URL blocking** — [`is_url_blocked`] checks each `LookupUrl`
///    target and each search hit against `blocked_urls`.
#[derive(Clone, Default)]
pub struct EnvContext {
    pub cutoff_date: Option<DateTime<Utc>>,
    pub blocked_urls: Vec<String>,
    pub leak_classifier: Option<Arc<dyn LeakClassifier>>,
}

impl std::fmt::Debug for EnvContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EnvContext")
            .field("cutoff_date", &self.cutoff_date)
            .field("blocked_urls", &self.blocked_urls)
            .field(
                "leak_classifier",
                &self.leak_classifier.as_ref().map(|_| "<dyn LeakClassifier>"),
            )
            .finish()
    }
}

/// Layer 2: post-fetch classifier that decides whether a search hit is
/// likely to contain post-cutoff information. `true` = leaked, drop it.
///
/// Production impl: a Haiku-class LLM call with a tight prompt that sees
/// the hit's url/title/snippet/published_at + the cutoff date. Stubbed
/// at the trait level here; implementations live alongside the
/// capability registry.
#[async_trait]
pub trait LeakClassifier: Send + Sync {
    async fn classify(&self, hit: &SearchHit, cutoff: DateTime<Utc>) -> bool;
}

/// Layer 1: append `before:YYYY-MM-DD` to a web-search query when a
/// cutoff is set. If no cutoff, returns the query unchanged. The
/// `before:` operator is recognized by Google and Bing; on engines that
/// don't honor it, layer 2's classifier picks up the slack.
pub fn apply_search_query_date_filter(query: &str, cutoff: Option<DateTime<Utc>>) -> String {
    match cutoff {
        None => query.to_string(),
        Some(c) => {
            let suffix = format!("before:{}", c.format("%Y-%m-%d"));
            // Idempotent: don't double-append if caller already did it.
            if query.contains("before:") {
                query.to_string()
            } else {
                format!("{} {}", query.trim(), suffix)
            }
        }
    }
}

/// Layer 4: substring-match a URL against the per-question blocklist.
/// `blocklist` entries are matched as substrings so callers can pass
/// either full URLs or domain prefixes (e.g. `polymarket.com/market/...`
/// blocks every variant of that page).
pub fn is_url_blocked(url: &str, blocklist: &[String]) -> bool {
    blocklist.iter().any(|b| !b.is_empty() && url.contains(b.as_str()))
}

/// One action the LLM picks per step.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Action {
    /// Issue a web search.
    WebSearch { query: String, k: u8 },
    /// Filter + summarize prior search results by their result_ids.
    SummarizeResults { result_ids: Vec<String> },
    /// Fetch and read a specific URL.
    LookupUrl { url: String },
    /// Source-specific time-series fetcher (yfinance / FRED / DBnomics).
    /// Stub in this implementation; returns Observation::Error("not implemented").
    FetchTimeSeries { source: String, key: String },
    /// Source-specific Wikipedia section fetcher with revision-as-of-date.
    /// Stub in this implementation.
    FetchWikipediaSection { article: String, section: String },
    /// Terminal action — submit the final probability.
    Submit { probability: f64 },
}

/// One search result hit returned by `WebSearch`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct SearchHit {
    pub id: String,
    pub url: String,
    pub title: String,
    pub snippet: String,
    pub published_at: Option<DateTime<Utc>>,
}

/// One time-series data point returned by `FetchTimeSeries`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct TimeSeriesPoint {
    pub timestamp: DateTime<Utc>,
    pub value: f64,
}

/// The result of executing one Action.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Observation {
    SearchResults { results: Vec<SearchHit> },
    Summary { text: String },
    PageContent {
        url: String,
        content: String,
        fetched_at: DateTime<Utc>,
    },
    TimeSeries {
        source: String,
        key: String,
        points: Vec<TimeSeriesPoint>,
    },
    WikipediaSection {
        article: String,
        section: String,
        content: String,
    },
    Submitted { probability: f64 },
    Error { message: String },
}

/// Errors raised by the parser.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ParseError {
    #[error("no fenced JSON step block found in LLM output")]
    NoStepBlock,
    #[error("step JSON failed to parse: {0}")]
    BadJson(String),
    #[error("step missing field `{0}`")]
    MissingField(&'static str),
    #[error("action validation: {0}")]
    InvalidAction(String),
    #[error("belief validation: {0}")]
    InvalidBelief(String),
}

/// Parsed step: the action the LLM picked AND the belief state at this step.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedStep {
    pub action: Action,
    pub belief: TrialResponse,
}

/// Parse one LLM response into a (Action, belief) pair.
///
/// Looks for a fenced ```json block whose content has both an `action` field
/// and a `belief` field. The `action` is parsed as [`Action`]; the `belief`
/// is parsed as [`TrialResponse`] (which is the per-trial belief shape;
/// matches the structured belief state from BLFX-2).
pub fn parse_step(llm_output: &str) -> Result<ParsedStep, ParseError> {
    let raw = extract_step_block(llm_output).ok_or(ParseError::NoStepBlock)?;
    let value: serde_json::Value =
        serde_json::from_str(&raw).map_err(|e| ParseError::BadJson(e.to_string()))?;

    let action_value = value.get("action").ok_or(ParseError::MissingField("action"))?;
    let belief_value = value.get("belief").ok_or(ParseError::MissingField("belief"))?;

    let action: Action = serde_json::from_value(action_value.clone())
        .map_err(|e| ParseError::InvalidAction(e.to_string()))?;
    validate_action(&action)?;

    let belief: TrialResponse = serde_json::from_value(belief_value.clone())
        .map_err(|e| ParseError::InvalidBelief(e.to_string()))?;
    validate_belief(&belief)?;

    Ok(ParsedStep { action, belief })
}

fn validate_action(action: &Action) -> Result<(), ParseError> {
    match action {
        Action::WebSearch { query, k } => {
            if query.trim().is_empty() {
                return Err(ParseError::InvalidAction("query is empty".into()));
            }
            if *k == 0 || *k > 20 {
                return Err(ParseError::InvalidAction(format!(
                    "k={} out of range [1, 20]",
                    k
                )));
            }
        }
        Action::SummarizeResults { result_ids } => {
            if result_ids.is_empty() {
                return Err(ParseError::InvalidAction("result_ids is empty".into()));
            }
        }
        Action::LookupUrl { url } => {
            if !url.starts_with("http://") && !url.starts_with("https://") {
                return Err(ParseError::InvalidAction(format!(
                    "url `{}` is not http(s)",
                    url
                )));
            }
        }
        Action::Submit { probability } => {
            if !(0.0..=1.0).contains(probability) || !probability.is_finite() {
                return Err(ParseError::InvalidAction(format!(
                    "probability {} not in [0, 1]",
                    probability
                )));
            }
        }
        _ => {} // FetchTimeSeries / FetchWikipediaSection: no validation; stubs anyway
    }
    Ok(())
}

fn validate_belief(belief: &TrialResponse) -> Result<(), ParseError> {
    if !(0.0..=1.0).contains(&belief.probability) || !belief.probability.is_finite() {
        return Err(ParseError::InvalidBelief(format!(
            "probability {} not in [0, 1]",
            belief.probability
        )));
    }
    Ok(())
}

/// Extract the contents of the LAST fenced ```json block in `text`.
/// (Last so a model can show worked examples earlier in its prose.)
fn extract_step_block(text: &str) -> Option<String> {
    let marker = "```json";
    let start = text.rfind(marker)?;
    let after_marker = &text[start + marker.len()..];
    let end = after_marker.find("```")?;
    Some(after_marker[..end].trim().to_string())
}

/// Execute one action. Pure-async; cooperates with claudecode tool subset
/// (WebSearch, Read) via the configured backend.
///
/// In this implementation:
/// - WebSearch / LookupUrl: returns Observation::Error("not yet implemented")
///   for now — actual integration with claudecode's WebSearch tool lands
///   when BLFX-4 (the iterative loop) wires things together.
/// - Submit: returns Observation::Submitted; the loop in BLFX-4 sees this
///   and terminates.
///
/// This keeps BLFX-3 standalone-testable; BLFX-4 is what assembles
/// parser + executor + claudecode into the actual loop.
pub async fn execute_action(action: Action) -> Observation {
    match action {
        Action::Submit { probability } => Observation::Submitted { probability },
        Action::WebSearch { .. } | Action::LookupUrl { .. } | Action::SummarizeResults { .. } => {
            Observation::Error {
                message: "execute_action is a stub; integration lands in BLFX-4".into(),
            }
        }
        Action::FetchTimeSeries { .. } | Action::FetchWikipediaSection { .. } => {
            Observation::Error {
                message: "source-specific data tools are stubbed; future ticket".into(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn step_block(action: serde_json::Value, belief: serde_json::Value) -> String {
        format!(
            "Reasoning here.\n\n```json\n{}\n```\n",
            serde_json::to_string_pretty(&json!({"action": action, "belief": belief})).unwrap()
        )
    }

    fn good_belief() -> serde_json::Value {
        json!({
            "probability": 0.5,
            "summary": "",
            "evidence_for": [],
            "evidence_against": [],
            "open_questions": []
        })
    }

    #[test]
    fn parse_web_search() {
        let text = step_block(
            json!({"type": "web_search", "query": "BTC price", "k": 5}),
            good_belief(),
        );
        let step = parse_step(&text).unwrap();
        match step.action {
            Action::WebSearch { query, k } => {
                assert_eq!(query, "BTC price");
                assert_eq!(k, 5);
            }
            other => panic!("unexpected: {:?}", other),
        }
        assert_eq!(step.belief.probability, 0.5);
    }

    #[test]
    fn parse_submit() {
        let text = step_block(json!({"type": "submit", "probability": 0.42}), good_belief());
        let step = parse_step(&text).unwrap();
        match step.action {
            Action::Submit { probability } => assert_eq!(probability, 0.42),
            other => panic!("unexpected: {:?}", other),
        }
    }

    #[test]
    fn parse_lookup_url() {
        let text = step_block(
            json!({"type": "lookup_url", "url": "https://example.com/page"}),
            good_belief(),
        );
        let step = parse_step(&text).unwrap();
        assert!(matches!(step.action, Action::LookupUrl { .. }));
    }

    #[test]
    fn parse_summarize_results() {
        let text = step_block(
            json!({"type": "summarize_results", "result_ids": ["r1", "r2"]}),
            good_belief(),
        );
        let step = parse_step(&text).unwrap();
        match step.action {
            Action::SummarizeResults { result_ids } => assert_eq!(result_ids.len(), 2),
            other => panic!("unexpected: {:?}", other),
        }
    }

    #[test]
    fn parse_no_block_errors() {
        assert_eq!(parse_step("just prose").unwrap_err(), ParseError::NoStepBlock);
    }

    #[test]
    fn parse_missing_action_errors() {
        let text = "```json\n{\"belief\": {\"probability\": 0.5}}\n```";
        match parse_step(text).unwrap_err() {
            ParseError::MissingField(f) => assert_eq!(f, "action"),
            other => panic!("unexpected: {:?}", other),
        }
    }

    #[test]
    fn parse_missing_belief_errors() {
        let text = "```json\n{\"action\": {\"type\": \"submit\", \"probability\": 0.5}}\n```";
        match parse_step(text).unwrap_err() {
            ParseError::MissingField(f) => assert_eq!(f, "belief"),
            other => panic!("unexpected: {:?}", other),
        }
    }

    #[test]
    fn parse_invalid_action_url() {
        let text = step_block(json!({"type": "lookup_url", "url": "not-a-url"}), good_belief());
        assert!(matches!(parse_step(&text).unwrap_err(), ParseError::InvalidAction(_)));
    }

    #[test]
    fn parse_invalid_action_k_out_of_range() {
        let text = step_block(
            json!({"type": "web_search", "query": "x", "k": 100}),
            good_belief(),
        );
        assert!(matches!(parse_step(&text).unwrap_err(), ParseError::InvalidAction(_)));
    }

    #[test]
    fn parse_invalid_action_empty_query() {
        let text = step_block(
            json!({"type": "web_search", "query": "", "k": 5}),
            good_belief(),
        );
        assert!(matches!(parse_step(&text).unwrap_err(), ParseError::InvalidAction(_)));
    }

    #[test]
    fn parse_invalid_action_submit_out_of_range() {
        let text = step_block(json!({"type": "submit", "probability": 1.5}), good_belief());
        assert!(matches!(parse_step(&text).unwrap_err(), ParseError::InvalidAction(_)));
    }

    #[test]
    fn parse_invalid_belief() {
        let text = step_block(
            json!({"type": "submit", "probability": 0.5}),
            json!({"probability": 2.0, "summary": ""}),
        );
        assert!(matches!(parse_step(&text).unwrap_err(), ParseError::InvalidBelief(_)));
    }

    #[test]
    fn parse_uses_last_fenced_block() {
        // Earlier example block + later real block; parser picks the last.
        let text = format!(
            "Example:\n```json\n{{\"action\": {{\"type\": \"submit\", \"probability\": 0.99}}, \"belief\": {{\"probability\": 0.99}}}}\n```\n\nActual:\n{}",
            step_block(json!({"type": "submit", "probability": 0.42}), good_belief())
        );
        let step = parse_step(&text).unwrap();
        match step.action {
            Action::Submit { probability } => assert_eq!(probability, 0.42),
            other => panic!("unexpected: {:?}", other),
        }
    }

    #[tokio::test]
    async fn execute_submit_returns_submitted() {
        let obs = execute_action(Action::Submit { probability: 0.7 }).await;
        match obs {
            Observation::Submitted { probability } => assert_eq!(probability, 0.7),
            other => panic!("unexpected: {:?}", other),
        }
    }

    #[tokio::test]
    async fn execute_web_search_is_stub_for_now() {
        let obs = execute_action(Action::WebSearch {
            query: "x".into(),
            k: 3,
        })
        .await;
        assert!(matches!(obs, Observation::Error { .. }));
    }

    #[tokio::test]
    async fn execute_data_tool_stubs() {
        let obs = execute_action(Action::FetchTimeSeries {
            source: "fred".into(),
            key: "GDP".into(),
        })
        .await;
        assert!(matches!(obs, Observation::Error { .. }));
    }

    // BLFX-9 layer-1 tests: date-filter on search queries.

    #[test]
    fn date_filter_no_cutoff_returns_query_unchanged() {
        assert_eq!(
            apply_search_query_date_filter("BTC price", None),
            "BTC price"
        );
    }

    #[test]
    fn date_filter_appends_before_clause() {
        let cutoff = DateTime::parse_from_rfc3339("2026-03-15T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let q = apply_search_query_date_filter("BTC price", Some(cutoff));
        assert_eq!(q, "BTC price before:2026-03-15");
    }

    #[test]
    fn date_filter_idempotent_when_already_present() {
        let cutoff = DateTime::parse_from_rfc3339("2026-03-15T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let q = apply_search_query_date_filter("BTC price before:2026-01-01", Some(cutoff));
        // Caller's existing before: clause wins — we don't override it.
        assert_eq!(q, "BTC price before:2026-01-01");
    }

    // BLFX-9 layer-4 tests: URL blocklist.

    #[test]
    fn url_blocklist_empty_blocks_nothing() {
        assert!(!is_url_blocked("https://example.com/page", &[]));
    }

    #[test]
    fn url_blocklist_blocks_substring_match() {
        let blocklist = vec!["polymarket.com/market/will-x-happen".to_string()];
        assert!(is_url_blocked(
            "https://polymarket.com/market/will-x-happen",
            &blocklist
        ));
        assert!(is_url_blocked(
            "https://www.polymarket.com/market/will-x-happen?utm=foo",
            &blocklist
        ));
    }

    #[test]
    fn url_blocklist_passes_unrelated_urls() {
        let blocklist = vec!["polymarket.com/market/will-x".to_string()];
        assert!(!is_url_blocked("https://example.com/news", &blocklist));
        assert!(!is_url_blocked(
            "https://polymarket.com/market/will-y",
            &blocklist
        ));
    }

    #[test]
    fn url_blocklist_ignores_empty_entries() {
        // An empty string in the blocklist must NOT match every URL —
        // that would bricks all lookups when callers pass a sloppy list.
        let blocklist = vec!["".to_string()];
        assert!(!is_url_blocked("https://example.com", &blocklist));
    }
}
