//! BLFX-4 iterative trial loop — mock-only skeleton.
//!
//! Per Murphy 2026 Algorithm 1, each trial is an iterative loop where the
//! LLM at each step produces both an action and an updated belief; the
//! substrate executes the action, appends the result to history, and
//! continues until `Submit` or `T_max` steps.
//!
//! This module ships the loop driver decoupled from claudecode via the
//! [`StepDriver`] trait. Production wiring (claudecode multi-turn chat) is
//! deferred to a follow-up; tests exercise the loop using
//! [`QueueStepDriver`] which dispenses canned responses.
//!
//! Termination:
//! - `Action::Submit { probability }` returns the current belief with that
//!   probability folded in.
//! - Hitting `max_steps` without a submit force-submits the most recent
//!   belief and downgrades `confidence` to `Low`.
//! - Any per-step parse error aborts the trial; the caller records it as a
//!   `TrialFailure`.

use async_trait::async_trait;

use super::agent_loop::{execute_action as default_execute_action, parse_step, Action, Observation, ParseError};
use super::types::{ForecastConfidence, TrialResponse};

/// One assistant step the loop executed: what action was taken and what
/// observation came back. Belief snapshots are kept on the per-step belief.
#[derive(Debug, Clone)]
pub struct HistoryEntry {
    pub action: Action,
    pub observation: Observation,
    pub belief: TrialResponse,
}

/// Errors raised by the iterative loop driver.
#[derive(Debug, thiserror::Error)]
pub enum LoopError {
    #[error("driver: {0}")]
    Driver(String),
    #[error("parse step {step}: {source}")]
    Parse { step: u8, source: ParseError },
    #[error("loop produced no belief before exit")]
    NoBelief,
}

/// Context the loop hands the driver each step. The driver decides what
/// to put on the wire — for claudecode the driver may emit only the latest
/// observation (since the session retains history server-side); for
/// stateless mocks the driver can use [`build_step_prompt`] to render
/// the full history into the prompt.
pub struct StepContext<'a> {
    pub initial_question: &'a str,
    pub step_idx: u8,
    pub max_steps: u8,
    pub history: &'a [HistoryEntry],
}

/// Pluggable per-step driver. Production wires this to a long-running
/// claudecode session (one chat call per step) and a search worker.
/// Tests dispense canned responses via [`QueueStepDriver`].
#[async_trait]
pub trait StepDriver: Send {
    /// Send the next step to the LLM, await the response text. The text is
    /// then passed through [`parse_step`] by the loop.
    async fn next_step(&mut self, ctx: StepContext<'_>) -> Result<String, String>;

    /// Execute the action the LLM picked this step. Default impl returns
    /// the stub observations from [`super::agent_loop::execute_action`]
    /// (suitable for mock-only tests). Production drivers override this
    /// to run real WebSearch / LookupUrl / source-specific fetchers.
    async fn execute_action(&mut self, action: Action) -> Observation {
        default_execute_action(action).await
    }
}

/// Run the iterative trial loop against `driver`.
///
/// Returns the final belief (whether submitted or force-submitted at T_max).
/// `history` carries every (action, observation, belief) executed during the
/// trial; callers may persist it on the trial session for audit.
pub async fn iterative_trial<D: StepDriver>(
    driver: &mut D,
    initial_question: &str,
    max_steps: u8,
) -> Result<(TrialResponse, Vec<HistoryEntry>), LoopError> {
    let mut history: Vec<HistoryEntry> = Vec::new();
    let mut last_belief: Option<TrialResponse> = None;

    for step_idx in 0..max_steps {
        let ctx = StepContext {
            initial_question,
            step_idx,
            max_steps,
            history: &history,
        };
        let raw = driver.next_step(ctx).await.map_err(LoopError::Driver)?;
        let parsed = parse_step(&raw).map_err(|source| LoopError::Parse {
            step: step_idx,
            source,
        })?;
        last_belief = Some(parsed.belief.clone());

        match parsed.action {
            Action::Submit { probability } => {
                let mut belief = parsed.belief;
                belief.probability = probability;
                return Ok((belief, history));
            }
            other => {
                let observation = driver.execute_action(other.clone()).await;
                history.push(HistoryEntry {
                    action: other,
                    observation,
                    belief: parsed.belief,
                });
            }
        }
    }

    // T_max hit without Submit: force-submit using the most recent belief
    // and downgrade confidence to Low (per BLFX-4 acceptance criterion 3).
    let mut belief = last_belief.ok_or(LoopError::NoBelief)?;
    belief.confidence = Some(ForecastConfidence::Low);
    Ok((belief, history))
}

/// Compose the per-step prompt the LLM sees. Skeleton implementation; the
/// production prompt will live in `forecast/SKILL.md` and reference the
/// history. For now this is deterministic enough to drive tests and clear
/// enough to read.
pub fn build_step_prompt(
    initial_question: &str,
    history: &[HistoryEntry],
    step_idx: u8,
    max_steps: u8,
) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "Step {}/{} of an iterative forecast.\n\nQuestion: {}\n\n",
        step_idx + 1,
        max_steps,
        initial_question
    ));
    if history.is_empty() {
        out.push_str("(no actions taken yet)\n");
    } else {
        out.push_str("History:\n");
        for (i, entry) in history.iter().enumerate() {
            out.push_str(&format!(
                "  step {}: action={:?}\n           observation={:?}\n",
                i + 1,
                entry.action,
                entry.observation
            ));
        }
    }
    out.push_str(
        "\nReply with a fenced ```json block containing both `action` (next \
         step or `submit`) and `belief` (your current ForecastState).\n",
    );
    out
}

/// Test fixture: dispenses a queue of canned LLM responses. Each call to
/// `next_step` pops the front of the queue.
pub struct QueueStepDriver {
    pub responses: std::collections::VecDeque<String>,
    pub seen_prompts: Vec<String>,
}

impl QueueStepDriver {
    pub fn new<I: IntoIterator<Item = String>>(responses: I) -> Self {
        Self {
            responses: responses.into_iter().collect(),
            seen_prompts: Vec::new(),
        }
    }
}

#[async_trait]
impl StepDriver for QueueStepDriver {
    async fn next_step(&mut self, ctx: StepContext<'_>) -> Result<String, String> {
        // Render the full-history prompt for inspection so tests can assert
        // on what the loop was building.
        self.seen_prompts.push(build_step_prompt(
            ctx.initial_question,
            ctx.history,
            ctx.step_idx,
            ctx.max_steps,
        ));
        self.responses
            .pop_front()
            .ok_or_else(|| "queue exhausted".to_string())
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

    fn belief_with(p: f64, summary: &str) -> serde_json::Value {
        json!({
            "probability": p,
            "summary": summary,
            "evidence_for": [],
            "evidence_against": [],
            "open_questions": []
        })
    }

    #[tokio::test]
    async fn submits_at_step_three_returns_submitted_belief() {
        let mut driver = QueueStepDriver::new(vec![
            step_block(
                json!({"type": "web_search", "query": "BTC", "k": 5}),
                belief_with(0.4, "no evidence yet"),
            ),
            step_block(
                json!({"type": "lookup_url", "url": "https://example.com/page"}),
                belief_with(0.55, "search done"),
            ),
            step_block(
                json!({"type": "submit", "probability": 0.62}),
                belief_with(0.6, "fetched, ready to submit"),
            ),
        ]);

        let (belief, history) = iterative_trial(&mut driver, "Will X happen?", 10)
            .await
            .unwrap();

        assert!((belief.probability - 0.62).abs() < 1e-9);
        assert_eq!(history.len(), 2, "two non-submit actions executed before submit");
        assert!(matches!(history[0].action, Action::WebSearch { .. }));
        assert!(matches!(history[1].action, Action::LookupUrl { .. }));
        assert_eq!(driver.seen_prompts.len(), 3);
    }

    #[tokio::test]
    async fn t_max_hit_without_submit_force_submits_low_confidence() {
        let make_search_step = |p: f64| {
            step_block(
                json!({"type": "web_search", "query": "again", "k": 3}),
                belief_with(p, "still researching"),
            )
        };
        let mut driver = QueueStepDriver::new(vec![
            make_search_step(0.4),
            make_search_step(0.45),
            make_search_step(0.50),
            make_search_step(0.55),
            make_search_step(0.58),
        ]);

        let (belief, history) = iterative_trial(&mut driver, "Will X happen?", 5)
            .await
            .unwrap();

        assert_eq!(history.len(), 5, "all 5 steps recorded since none submitted");
        assert!((belief.probability - 0.58).abs() < 1e-9);
        assert_eq!(belief.confidence, Some(ForecastConfidence::Low));
    }

    #[tokio::test]
    async fn driver_error_propagates_as_loop_error() {
        let mut driver = QueueStepDriver::new(vec![]);
        let err = iterative_trial(&mut driver, "Will X happen?", 3)
            .await
            .unwrap_err();
        assert!(matches!(err, LoopError::Driver(_)));
    }

    #[tokio::test]
    async fn parse_error_at_step_aborts_with_step_index() {
        let mut driver = QueueStepDriver::new(vec![
            step_block(
                json!({"type": "web_search", "query": "BTC", "k": 5}),
                belief_with(0.5, ""),
            ),
            "no fenced json here".to_string(), // step 1 (0-indexed)
        ]);
        let err = iterative_trial(&mut driver, "Will X happen?", 5)
            .await
            .unwrap_err();
        match err {
            LoopError::Parse { step, source } => {
                assert_eq!(step, 1);
                assert_eq!(source, ParseError::NoStepBlock);
            }
            other => panic!("unexpected: {:?}", other),
        }
    }

    #[tokio::test]
    async fn submit_at_first_step_returns_immediately() {
        let mut driver = QueueStepDriver::new(vec![step_block(
            json!({"type": "submit", "probability": 0.73}),
            belief_with(0.5, "training-only answer"),
        )]);
        let (belief, history) = iterative_trial(&mut driver, "Will X happen?", 10)
            .await
            .unwrap();
        assert!((belief.probability - 0.73).abs() < 1e-9);
        assert!(history.is_empty(), "no non-submit actions before submission");
    }

    #[test]
    fn build_prompt_includes_step_counter_and_question() {
        let p = build_step_prompt("Will X happen?", &[], 0, 10);
        assert!(p.contains("Step 1/10"));
        assert!(p.contains("Will X happen?"));
        assert!(p.contains("(no actions taken yet)"));
    }

    #[test]
    fn build_prompt_renders_history_actions_and_observations() {
        let history = vec![HistoryEntry {
            action: Action::WebSearch {
                query: "BTC price".into(),
                k: 5,
            },
            observation: Observation::Error {
                message: "stub".into(),
            },
            belief: serde_json::from_value(belief_with(0.5, "")).unwrap(),
        }];
        let p = build_step_prompt("Will X happen?", &history, 1, 10);
        assert!(p.contains("Step 2/10"));
        assert!(p.contains("BTC price"));
        assert!(p.contains("stub"));
    }
}
