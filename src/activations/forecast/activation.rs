//! Forecast activation — BLF binary forecasting as a Plexus skill.
//!
//! Methods:
//! - `create` — register a question with a binary, dated, observable resolution criterion
//! - `update` — given a prior + new evidence, produce a new belief state
//! - `resolve` — record ground truth; refits Platt parameters when threshold crossed
//!
//! See `mneme/plans/MNEME/MNEME-6.md` for the contract.

use std::sync::Arc;
use std::time::Duration;

use async_stream::stream;
use chrono::{DateTime, NaiveDate, Utc};
use futures::Stream;
use serde_json::json;

use super::types::*;
use crate::mneme::context::MnemeContext;
use crate::mneme::runtime::swarm_runtime::TrialParams;
use crate::mneme::swarm::aggregate::{aggregate, AggregationRule};

const DEFAULT_TRIALS: u8 = 3;
const DEFAULT_LAMBDA: f64 = 0.2;
const DEFAULT_PRIOR: f64 = 0.5;
const DEFAULT_TIMEOUT_SECS: u64 = 120;

/// The forecast activation. Holds an [`MnemeContext`] for opening programs
/// and accessing the orchestration runtime.
#[derive(Clone)]
pub struct Forecast {
    context: Arc<MnemeContext>,
}

impl Forecast {
    pub fn new(context: Arc<MnemeContext>) -> Self {
        Self { context }
    }
}

#[plexus_macros::activation(
    namespace = "forecast",
    version = "0.1.0",
    description = "Bayesian Linguistic Forecaster — binary forecasting with multi-trial aggregation"
)]
impl Forecast {
    /// Create a new forecast question.
    ///
    /// The resolution criterion must be observable and the deadline must be in
    /// the future. On success, opens a program directory and returns its id.
    #[plexus_macros::method(params(
        question = "The forecasting question (binary outcome by deadline)",
        resolution_criterion = "How the outcome will be observed",
        deadline = "ISO 8601 date by which the question resolves",
        trials = "Default trial count for subsequent updates (1..=16, default 3)"
    ))]
    async fn create(
        &self,
        question: String,
        resolution_criterion: String,
        deadline: String,
        trials: Option<u8>,
    ) -> impl Stream<Item = CreateEvent> + Send + 'static {
        let context = self.context.clone();
        stream! {
            // Resolvability gate.
            match parse_deadline(&deadline) {
                Err(reason) => { yield CreateEvent::ResolvabilityFailed { reason }; return; }
                Ok(deadline_dt) if deadline_dt < Utc::now() => {
                    yield CreateEvent::ResolvabilityFailed {
                        reason: format!("deadline {} is in the past", deadline),
                    };
                    return;
                }
                Ok(_) => {}
            }
            let trials = trials.unwrap_or(DEFAULT_TRIALS);
            if !(1..=16).contains(&trials) {
                yield CreateEvent::Error {
                    message: format!("trials must be in 1..=16, got {}", trials),
                };
                return;
            }

            // Open the program.
            let program = match context.open_program(
                "forecast.create",
                json!({
                    "question": &question,
                    "resolution_criterion": resolution_criterion,
                    "deadline": &deadline,
                    "trials": trials,
                }),
            ) {
                Ok(p) => p,
                Err(e) => {
                    yield CreateEvent::Error { message: format!("open_program: {}", e) };
                    return;
                }
            };
            let program_id = program.id().to_string();

            // For create, no orchestration — write the initial empty artifact and close.
            let artifact = json!({
                "question": &question,
                "deadline": &deadline,
                "trials": trials,
                "state": null,
            });
            if let Err(e) = program.close_completed(&artifact, "0.1.0") {
                yield CreateEvent::Error { message: format!("close_completed: {}", e) };
                return;
            }

            yield CreateEvent::Created { program_id, question, deadline };
        }
    }

    /// Update an existing forecast with new evidence.
    ///
    /// Opens a fresh program for this update, runs `swarm.trial`, aggregates
    /// with logit-shrinkage + concat-evidence, writes the new belief state.
    #[plexus_macros::method(streaming, params(
        program_id = "Program id from forecast.create or a prior update (used as the parent question id)",
        new_evidence = "Free-form evidence to condition this update on (may be empty)",
        trials = "Number of trials for this update (1..=16, defaults to 3)",
        parent_session = "Optional parent claudecode session name; defaults to 'forecast-parent'"
    ))]
    async fn update(
        &self,
        program_id: String,
        new_evidence: String,
        trials: Option<u8>,
        parent_session: Option<String>,
    ) -> impl Stream<Item = UpdateEvent> + Send + 'static {
        let context = self.context.clone();
        stream! {
            let trials = trials.unwrap_or(DEFAULT_TRIALS);
            if !(1..=16).contains(&trials) {
                yield UpdateEvent::Error {
                    stage: "validate".into(),
                    message: format!("trials must be in 1..=16, got {}", trials),
                };
                return;
            }
            let parent_session = parent_session.unwrap_or_else(|| "forecast-parent".to_string());

            // Open a fresh program for this update.
            let program = match context.open_program(
                "forecast.update",
                json!({
                    "question_program_id": &program_id,
                    "new_evidence": &new_evidence,
                    "trials": trials,
                }),
            ) {
                Ok(p) => p,
                Err(e) => {
                    yield UpdateEvent::Error {
                        stage: "open_program".into(),
                        message: e.to_string(),
                    };
                    return;
                }
            };
            let update_program_id = program.id().to_string();

            // Build the trial prompt. Carries the question id and the new evidence;
            // the SKILL.md (when wired in as a system prompt) provides the BLF framing.
            let prompt = format!(
                "Forecast update for question program {}.\n\nNew evidence:\n{}\n\nReturn a JSON object with fields `probability` (a number in [0,1]) and `summary` (a one-paragraph evidence summary).",
                program_id, new_evidence
            );
            let response_schema = json!({
                "type": "object",
                "properties": {
                    "probability": {"type": "number", "minimum": 0.0, "maximum": 1.0},
                    "summary": {"type": "string"}
                },
                "required": ["probability", "summary"]
            });

            // Run the trial fan-out.
            let params = TrialParams {
                parent_session,
                prompt,
                response_schema,
                n: trials,
                diversify: Some("Reasoning style #%i (analytic / contrarian / base-rate-grounded)".into()),
                timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECS),
            };
            let batch = match context.swarm().trial(&program, params).await {
                Ok(b) => b,
                Err(e) => {
                    let _ = program.close_failed("SwarmError", &e.to_string(), "swarm.trial");
                    yield UpdateEvent::Error {
                        stage: "swarm.trial".into(),
                        message: e.to_string(),
                    };
                    return;
                }
            };

            if batch.success_count() == 0 {
                let msg = format!("all {} trials failed", batch.failure_count());
                let _ = program.close_failed("AllTrialsFailed", &msg, "swarm.trial");
                yield UpdateEvent::Error {
                    stage: "swarm.trial".into(),
                    message: msg,
                };
                return;
            }

            // Aggregate.
            let trial_responses: Vec<_> = batch.successes.iter().map(|t| t.response.clone()).collect();
            let raw_trials_typed: Vec<TrialResponse> = trial_responses.iter()
                .filter_map(|v| serde_json::from_value::<TrialResponse>(v.clone()).ok())
                .collect();

            let logit = match aggregate(&trial_responses, &AggregationRule::LogitShrinkage {
                field: "probability".into(),
                prior: DEFAULT_PRIOR,
                lambda: DEFAULT_LAMBDA,
            }) {
                Ok(v) => v,
                Err(e) => {
                    let _ = program.close_failed("AggregateError", &e.to_string(), "aggregate.logit");
                    yield UpdateEvent::Error {
                        stage: "aggregate".into(),
                        message: e.to_string(),
                    };
                    return;
                }
            };

            let summary = match aggregate(&trial_responses, &AggregationRule::ConcatEvidence {
                field: "summary".into(),
                separator: "\n\n".into(),
            }) {
                Ok(v) => v["aggregated"].as_str().unwrap_or("").to_string(),
                Err(_) => String::new(), // soft-fail: empty summary is acceptable
            };

            let probability = logit["aggregated"].as_f64().unwrap_or(DEFAULT_PRIOR);
            let n_trials = batch.success_count() as u8;
            let confidence = if n_trials > 1 {
                ForecastConfidence::MultiTrial
            } else {
                ForecastConfidence::SinglePass
            };
            let state = ForecastState {
                probability,
                summary,
                confidence,
                n_trials,
                prior_used: None,
            };

            yield UpdateEvent::Aggregated {
                aggregated: state.clone(),
                raw_trials: raw_trials_typed,
            };

            // Write artifact + close.
            if let Err(e) = program.close_completed(&state, "0.1.0") {
                yield UpdateEvent::Error {
                    stage: "close_completed".into(),
                    message: e.to_string(),
                };
                return;
            }

            yield UpdateEvent::Completed {
                program_id: update_program_id.clone(),
                state,
                artifact_path: format!("programs/{}/artifact.json", update_program_id),
            };
        }
    }

    /// Record the ground truth outcome of a resolved forecast.
    ///
    /// Stub for now — the calibration store wiring lands when the resolve
    /// pipeline is needed. Tracked as an open concern in `mneme/ISSUES.md`.
    #[plexus_macros::method(params(
        program_id = "Program id of the forecast being resolved",
        actual = "Whether the predicted event happened",
        resolved_at = "ISO 8601 timestamp; defaults to now"
    ))]
    async fn resolve(
        &self,
        program_id: String,
        actual: bool,
        resolved_at: Option<String>,
    ) -> impl Stream<Item = ResolveEvent> + Send + 'static {
        let _ = (program_id, actual, resolved_at);
        stream! {
            yield ResolveEvent::Error {
                message: "forecast.resolve is not yet wired to the calibration store".to_string(),
            };
        }
    }
}

fn parse_deadline(s: &str) -> Result<DateTime<Utc>, String> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Ok(dt.with_timezone(&Utc));
    }
    if let Ok(d) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        let end = d
            .and_hms_opt(23, 59, 59)
            .ok_or_else(|| "invalid date components".to_string())?;
        return Ok(DateTime::<Utc>::from_naive_utc_and_offset(end, Utc));
    }
    Err(format!("could not parse deadline `{}`; expected ISO 8601", s))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mneme::runtime::swarm_runtime::DeterministicMockSwarmRuntime;
    use chrono::{Duration as ChronoDuration, Utc};
    use futures::StreamExt;
    use tempfile::TempDir;

    fn forecast_with_mock(responses: Vec<serde_json::Value>) -> (TempDir, Forecast) {
        let dir = TempDir::new().unwrap();
        let mock = Arc::new(DeterministicMockSwarmRuntime::new(responses));
        let context = Arc::new(MnemeContext::new(dir.path(), mock));
        (dir, Forecast::new(context))
    }

    fn forecast_with_stub() -> (TempDir, Forecast) {
        let dir = TempDir::new().unwrap();
        let context = Arc::new(MnemeContext::with_stub_swarm(dir.path()));
        (dir, Forecast::new(context))
    }

    async fn first_event<S, T>(stream: S) -> T
    where
        S: futures::Stream<Item = T>,
    {
        let mut s = Box::pin(stream);
        s.next().await.expect("event")
    }

    #[tokio::test]
    async fn create_returns_program_id_and_writes_directory() {
        let (dir, forecast) = forecast_with_stub();
        let future = (Utc::now() + ChronoDuration::days(30))
            .format("%Y-%m-%d")
            .to_string();
        let stream = forecast.create("Will X?".into(), "X observed by deadline".into(), future, None).await;
        let evt = first_event(stream).await;
        match evt {
            CreateEvent::Created { program_id, .. } => {
                assert!(!program_id.is_empty());
                let prog_dir = dir.path().join(&program_id);
                assert!(prog_dir.exists(), "program directory should exist");
                assert!(prog_dir.join("manifest.json").exists());
                assert!(prog_dir.join("artifact.json").exists());
            }
            other => panic!("unexpected: {:?}", other),
        }
    }

    #[tokio::test]
    async fn create_rejects_past_deadline() {
        let (_dir, forecast) = forecast_with_stub();
        let stream = forecast.create("Will X?".into(), "...".into(), "2024-01-01".into(), None).await;
        let evt = first_event(stream).await;
        assert!(matches!(evt, CreateEvent::ResolvabilityFailed { .. }));
    }

    #[tokio::test]
    async fn create_rejects_unparseable_deadline() {
        let (_dir, forecast) = forecast_with_stub();
        let stream = forecast.create("Will X?".into(), "...".into(), "not a date".into(), None).await;
        let evt = first_event(stream).await;
        assert!(matches!(evt, CreateEvent::ResolvabilityFailed { .. }));
    }

    #[tokio::test]
    async fn create_rejects_out_of_range_trials() {
        let (_dir, forecast) = forecast_with_stub();
        let future = (Utc::now() + ChronoDuration::days(30)).format("%Y-%m-%d").to_string();
        let stream = forecast.create("Will X?".into(), "...".into(), future, Some(99)).await;
        let evt = first_event(stream).await;
        assert!(matches!(evt, CreateEvent::Error { .. }));
    }

    #[tokio::test]
    async fn update_with_stub_emits_swarm_error() {
        let (_dir, forecast) = forecast_with_stub();
        let stream = forecast.update("p1".into(), "evidence".into(), None, None).await;
        let evt = first_event(stream).await;
        match evt {
            UpdateEvent::Error { stage, .. } => assert_eq!(stage, "swarm.trial"),
            other => panic!("unexpected: {:?}", other),
        }
    }

    #[tokio::test]
    async fn update_with_mock_runs_full_pipeline() {
        let (dir, forecast) = forecast_with_mock(vec![
            json!({"probability": 0.62, "summary": "trial 1: signals positive"}),
            json!({"probability": 0.55, "summary": "trial 2: mixed"}),
            json!({"probability": 0.70, "summary": "trial 3: structural factors favor"}),
        ]);
        let stream = forecast.update("Q-001".into(), "BTC at $95k".into(), Some(3), None).await;
        let mut s = Box::pin(stream);

        // First: Aggregated event with the raw trials.
        let evt = s.next().await.expect("aggregated event");
        match evt {
            UpdateEvent::Aggregated { aggregated, raw_trials } => {
                assert!(aggregated.probability > 0.5 && aggregated.probability < 0.7);
                assert_eq!(aggregated.confidence, ForecastConfidence::MultiTrial);
                assert_eq!(aggregated.n_trials, 3);
                assert!(aggregated.summary.contains("trial 1: signals positive"));
                assert_eq!(raw_trials.len(), 3);
            }
            other => panic!("unexpected first event: {:?}", other),
        }

        // Second: Completed event with the artifact.
        let evt = s.next().await.expect("completed event");
        match evt {
            UpdateEvent::Completed { program_id, state, artifact_path: _ } => {
                let prog_dir = dir.path().join(&program_id);
                assert!(prog_dir.join("artifact.json").exists());
                assert!(prog_dir.join("manifest.json").exists());
                assert!(prog_dir.join("trace.jsonl").exists(), "swarm.trial should have recorded a trace entry");
                assert_eq!(state.n_trials, 3);
            }
            other => panic!("unexpected second event: {:?}", other),
        }
    }

    #[tokio::test]
    async fn update_when_all_trials_fail_emits_error() {
        // Mock with zero responses → all trials report "ran out" failures.
        let (_dir, forecast) = forecast_with_mock(vec![]);
        let stream = forecast.update("Q-001".into(), "evidence".into(), Some(2), None).await;
        let evt = first_event(stream).await;
        match evt {
            UpdateEvent::Error { stage, message } => {
                assert_eq!(stage, "swarm.trial");
                assert!(message.contains("trials failed"));
            }
            other => panic!("unexpected: {:?}", other),
        }
    }

    #[tokio::test]
    async fn resolve_returns_not_wired_error() {
        let (_dir, forecast) = forecast_with_stub();
        let stream = forecast.resolve("p1".into(), true, None).await;
        let evt = first_event(stream).await;
        assert!(matches!(evt, ResolveEvent::Error { .. }));
    }

    #[test]
    fn parse_deadline_accepts_iso_date() {
        assert!(parse_deadline("2026-12-31").is_ok());
    }

    #[test]
    fn parse_deadline_accepts_rfc3339() {
        assert!(parse_deadline("2026-12-31T23:59:59Z").is_ok());
    }

    #[test]
    fn parse_deadline_rejects_garbage() {
        assert!(parse_deadline("yesterday").is_err());
    }
}
