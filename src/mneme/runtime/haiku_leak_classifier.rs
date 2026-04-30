//! BLFX-9 layer 2 — concrete LLM-backed leak classifier.
//!
//! Layer 2 of the date-leakage defense classifies each search hit
//! through a small LLM with a tight prompt: "given this hit and the
//! freeze cutoff, does it look like the result was published / dated /
//! sourced from after the cutoff?" Hits flagged YES are dropped
//! before the reasoning model sees them.
//!
//! This is a runtime-side companion to the offline-only
//! [`crate::activations::forecast::LeakClassifier`] trait. The trait
//! lives next to the action types (so `agent_loop` is unit-testable
//! without claudecode); the LLM-backed impl lives here so the
//! claudecode dep stays out of the offline primitive.
//!
//! Construction: [`HaikuLeakClassifier::new`] takes the runtime's
//! capability registry, the claudecode handle, and a working dir.
//! Auto-attach is done in `ClaudecodeStepDriver::new_with_env` when
//! the operator passed `cutoff_date` but didn't supply a classifier
//! (i.e. the bench-008 path).

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use plexus_core::plexus::HubContext;

use crate::activations::claudecode::ClaudeCode;
use crate::activations::forecast::{LeakClassifier, SearchHit};
use crate::mneme::capabilities::CapabilityRegistry;

/// Layer 2 default impl. Classifies a search hit by asking the
/// `leak_classifier` capability (Haiku-tier in the default registry)
/// whether the hit looks like post-cutoff information.
pub struct HaikuLeakClassifier<P: HubContext + 'static> {
    capabilities: CapabilityRegistry,
    claudecode: Arc<ClaudeCode<P>>,
    working_dir: String,
}

impl<P: HubContext + 'static> HaikuLeakClassifier<P> {
    pub fn new(
        capabilities: CapabilityRegistry,
        claudecode: Arc<ClaudeCode<P>>,
        working_dir: String,
    ) -> Self {
        Self {
            capabilities,
            claudecode,
            working_dir,
        }
    }
}

/// Tight classifier prompt. Outputs a single token: YES or NO.
fn build_prompt(hit: &SearchHit, cutoff: DateTime<Utc>) -> String {
    let cutoff_str = cutoff.format("%Y-%m-%d");
    let published = hit
        .published_at
        .map(|p| p.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "unknown".to_string());
    format!(
        "We are running a backtest with a strict freeze cutoff of {cutoff}. \
         The model must NOT see information published after that date.\n\n\
         Below is one search result. Decide whether the content looks \
         like it was published, dated, or sourced from AFTER {cutoff}. \
         Be conservative: if the snippet/title references events that \
         occurred after {cutoff}, that is a leak. If you can't tell from \
         the available metadata, lean NO so we don't over-prune.\n\n\
         URL: {url}\n\
         TITLE: {title}\n\
         PUBLISHED: {published}\n\
         SNIPPET: {snippet}\n\n\
         Respond with EXACTLY one token: `YES` (this hit looks post-cutoff and should be dropped) \
         or `NO` (this hit appears pre-cutoff or undatable).\n",
        cutoff = cutoff_str,
        url = hit.url,
        title = hit.title,
        published = published,
        snippet = hit.snippet,
    )
}

/// Parse the classifier's response. Lenient — looks for the first
/// occurrence of YES or NO in any reasonable casing/wrapping. Returns
/// `Some(true)` for YES (= leaked, drop), `Some(false)` for NO,
/// `None` if neither token appears.
pub(crate) fn parse_classifier_response(s: &str) -> Option<bool> {
    let upper = s.trim().to_uppercase();
    // Look at the first standalone YES/NO token.
    for tok in upper.split(|c: char| !c.is_ascii_alphabetic()) {
        if tok == "YES" {
            return Some(true);
        }
        if tok == "NO" {
            return Some(false);
        }
    }
    None
}

#[async_trait]
impl<P: HubContext + 'static> LeakClassifier for HaikuLeakClassifier<P> {
    async fn classify(&self, hit: &SearchHit, cutoff: DateTime<Utc>) -> bool {
        let prompt = build_prompt(hit, cutoff);
        let response = match self
            .capabilities
            .invoke(
                self.claudecode.clone(),
                "leak_classifier",
                prompt,
                self.working_dir.clone(),
            )
            .await
        {
            Ok(r) => r,
            Err(e) => {
                // Capability unavailable / network error: fall back to
                // NOT-leaked. Layer 1 already filtered the search query,
                // so the cost of an unclassified hit slipping through is
                // bounded; the cost of dropping every hit on a transient
                // failure is much worse.
                tracing::warn!(
                    url = %hit.url,
                    error = %e,
                    "BLFX-9 layer 2: classifier failed, defaulting to NOT-leaked"
                );
                return false;
            }
        };

        match parse_classifier_response(&response) {
            Some(b) => b,
            None => {
                // Lenient: if we can't parse, lean NOT-leaked so we
                // don't accidentally over-prune.
                tracing::warn!(
                    response = %response.trim(),
                    "BLFX-9 layer 2: classifier returned unparseable response; defaulting to NOT-leaked"
                );
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_yes_drops_hit() {
        assert_eq!(parse_classifier_response("YES"), Some(true));
        assert_eq!(parse_classifier_response("yes\n"), Some(true));
        assert_eq!(parse_classifier_response("YES."), Some(true));
        assert_eq!(parse_classifier_response("Answer: YES"), Some(true));
    }

    #[test]
    fn parse_no_keeps_hit() {
        assert_eq!(parse_classifier_response("NO"), Some(false));
        assert_eq!(parse_classifier_response("no"), Some(false));
        assert_eq!(parse_classifier_response("NO."), Some(false));
        assert_eq!(parse_classifier_response("Answer: NO\n"), Some(false));
    }

    #[test]
    fn parse_picks_first_token_when_both_appear() {
        // Defensive: model says "YES, this is post-cutoff, but NO the snippet..."
        // First decision token wins. (Vanishingly rare; the prompt asks
        // for one token only.)
        assert_eq!(
            parse_classifier_response("YES — context: NO indication of pre-cutoff"),
            Some(true)
        );
        assert_eq!(
            parse_classifier_response("NO — although YES could be argued..."),
            Some(false)
        );
    }

    #[test]
    fn parse_unparseable_returns_none() {
        assert_eq!(parse_classifier_response(""), None);
        assert_eq!(parse_classifier_response("maybe"), None);
        assert_eq!(parse_classifier_response("indeterminate"), None);
    }

    #[test]
    fn build_prompt_includes_cutoff_and_hit_fields() {
        let cutoff = DateTime::parse_from_rfc3339("2026-03-15T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let hit = SearchHit {
            id: "r1".into(),
            url: "https://example.com/a".into(),
            title: "Headline title".into(),
            snippet: "preview snippet".into(),
            published_at: Some(
                DateTime::parse_from_rfc3339("2026-04-10T00:00:00Z")
                    .unwrap()
                    .with_timezone(&Utc),
            ),
        };
        let p = build_prompt(&hit, cutoff);
        assert!(p.contains("2026-03-15"));
        assert!(p.contains("https://example.com/a"));
        assert!(p.contains("Headline title"));
        assert!(p.contains("preview snippet"));
        assert!(p.contains("2026-04-10"));
        assert!(p.contains("YES"));
        assert!(p.contains("NO"));
    }

    #[test]
    fn build_prompt_handles_unknown_published_at() {
        let cutoff = DateTime::parse_from_rfc3339("2026-03-15T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let hit = SearchHit {
            id: "r1".into(),
            url: "https://example.com/a".into(),
            title: "t".into(),
            snippet: "s".into(),
            published_at: None,
        };
        let p = build_prompt(&hit, cutoff);
        assert!(p.contains("PUBLISHED: unknown"));
    }
}
