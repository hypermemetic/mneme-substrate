//! Forecast activation — BLF binary forecasting as a Plexus skill.
//!
//! Methods:
//! - `create` — register a question with a binary, dated, observable resolution criterion
//! - `update` — fire-and-return: opens a program, spawns the trial work in
//!   the background, yields a Started event with the program_id and ends.
//!   Consumers poll `programs.status(program_id)` (when wired) or watch the
//!   filesystem `programs/<id>/manifest.json` for completion.
//! - `resolve` — record ground truth; refits Platt parameters when threshold crossed
//!
//! See `mneme/plans/MNEME/MNEME-6.md` for the contract.

use std::sync::Arc;
use std::time::Duration;

use async_stream::stream;
use chrono::{DateTime, NaiveDate, Utc};
use futures::Stream;
use serde_json::{json, Value};

use super::types::*;
use crate::mneme::context::MnemeContext;
use crate::mneme::runtime::swarm_runtime::{ParentSessionSpec, TrialParams};
use crate::mneme::swarm::aggregate::{aggregate, AggregationRule};

/// Parse the optional BLFX-9 inputs (`cutoff_date`, `blocked_urls`) into
/// an `EnvContext`. Returns `Ok(None)` when neither defense is requested
/// (production forecasting) so the caller passes through unchanged.
fn build_env_context(
    cutoff_date: Option<String>,
    blocked_urls: Option<Vec<String>>,
) -> Result<Option<crate::activations::forecast::EnvContext>, String> {
    let cutoff = match cutoff_date.as_deref() {
        None | Some("") => None,
        Some(s) => Some(
            DateTime::parse_from_rfc3339(s)
                .map(|dt| dt.with_timezone(&Utc))
                .map_err(|e| format!("could not parse cutoff_date `{}`: {}", s, e))?,
        ),
    };
    let blocked = blocked_urls.unwrap_or_default();
    if cutoff.is_none() && blocked.is_empty() {
        return Ok(None);
    }
    Ok(Some(crate::activations::forecast::EnvContext {
        cutoff_date: cutoff,
        blocked_urls: blocked,
        leak_classifier: None,
    }))
}

/// Build an empty `ForecastState` for the Started event's `prior` field.
/// Used when there's no real prior (first call) — the structured fields
/// stay empty and the schema version is current.
fn empty_state(probability: f64, confidence: ForecastConfidence) -> ForecastState {
    ForecastState {
        probability,
        raw_probability: None,
        confidence,
        evidence_for: vec![],
        evidence_against: vec![],
        open_questions: vec![],
        summary: String::new(),
        n_trials: 0,
        prior_used: None,
        belief_schema_version: BELIEF_SCHEMA_VERSION.to_string(),
    }
}

/// The forecasting skill prompt. Loaded into the parent claudecode session
/// as the system prompt so each trial reasons inside the BLF framing without
/// the activation having to repeat it in every prompt.
const FORECAST_SKILL_MD: &str = include_str!(
    "../../../../skills/skills/forecast/SKILL.md"
);

const DEFAULT_TRIALS: u8 = 3;
// Per Murphy 2026 §4, optimal α = 1.0 on ForecastBench (no shrinkage).
// Our previous λ=0.2 default shrunk every aggregated probability 20%
// toward 0.5 — wrong direction for confident-but-correct predictions
// and the largest single algorithmic divergence from the paper that
// can be fixed in one line. BLFX-5 will replace this with LOO-CV-tuned
// α once the calibration store has enough resolved observations
// (MNEME-26 seeded the first 20).
const DEFAULT_LAMBDA: f64 = 0.0;
const DEFAULT_PRIOR: f64 = 0.5;
const DEFAULT_TIMEOUT_SECS: u64 = 600;
// BLFX-18: iterative loop default. Paired n=94 result showed mneme+iterative
// at BI 84 vs crowd 57 (Δ +26.61 BI, 95% CI excludes 0). Murphy 2026 uses
// T_max=10; we default to 5 because cost scales linearly and the marginal
// gain from steps 6-10 is unmeasured in our setup.
const DEFAULT_ITERATIVE_MAX_STEPS: u8 = 5;

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

            // Open the program (async because storage may write to SQLite).
            let program = match context.open_program(
                "forecast.create",
                json!({
                    "question": &question,
                    "resolution_criterion": resolution_criterion,
                    "deadline": &deadline,
                    "trials": trials,
                }),
            ).await {
                Ok(p) => p,
                Err(e) => {
                    yield CreateEvent::Error { message: format!("open_program: {}", e) };
                    return;
                }
            };
            let program_id = program.id().to_string();

            let artifact = json!({
                "question": &question,
                "deadline": &deadline,
                "trials": trials,
                "state": null,
            });
            if let Err(e) = program.close_completed(&artifact, "0.1.0").await {
                yield CreateEvent::Error { message: format!("close_completed: {}", e) };
                return;
            }

            yield CreateEvent::Created { program_id, question, deadline };
        }
    }

    /// Update an existing forecast with new evidence. **Fire-and-return.**
    ///
    /// Opens a fresh program for this update, spawns the trial+aggregate+close
    /// work in the background, and returns immediately with a Started event
    /// carrying the new program_id. Consumers poll `programs.status` (when
    /// wired) or read `programs/<id>/manifest.json` to see when the work
    /// completes.
    #[plexus_macros::method(streaming, params(
        program_id = "Program id from forecast.create or a prior update (used as the parent question id)",
        new_evidence = "Free-form evidence to condition this update on (may be empty)",
        trials = "Number of trials for this update (1..=16, defaults to 3)",
        parent_session = "Optional parent claudecode session name; defaults to 'forecast-parent'",
        allowed_tools = "Optional list of tools to allow per trial (e.g. [\"WebSearch\", \"Read\"]); None uses the runtime default",
        iterative_max_steps = "If Some, run each trial as an iterative BLF loop with up to N steps (Murphy 2026 Algorithm 1). If None or 0, run as a single chat call (legacy mode). Default None.",
        cutoff_date = "BLFX-9 date-leakage defense: ISO 8601 freeze date. When set, every web search query is filtered with `before:YYYY-MM-DD` and the per-question URL blocklist is enforced. None (production) leaves all defenses inert.",
        blocked_urls = "BLFX-9 layer 4: URLs that are forbidden for this question (e.g. the prediction market's resolution page). Substring match — full URL or domain prefix both work."
    ))]
    async fn update(
        &self,
        program_id: String,
        new_evidence: String,
        trials: Option<u8>,
        parent_session: Option<String>,
        allowed_tools: Option<Vec<String>>,
        iterative_max_steps: Option<u8>,
        cutoff_date: Option<String>,
        blocked_urls: Option<Vec<String>>,
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
                    "allowed_tools": &allowed_tools,
                }),
            ).await {
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

            // Ensure the parent session exists with the forecast SKILL.md as
            // its system prompt. ParentSessionSpec hashes the system_prompt
            // into the actual underlying session name (MNEME-21), so SKILL.md
            // changes produce a fresh session rather than inheriting the
            // stale prompt from a previous substrate run.
            let spec = ParentSessionSpec {
                name: parent_session.clone(),
                system_prompt: FORECAST_SKILL_MD.to_string(),
                working_dir: context.programs_root().to_string_lossy().to_string(),
                model: "sonnet".to_string(),
            };
            // The trial fan-out needs the resolved name; both ensure and trial
            // see the same content-hashed name for the same SKILL.md.
            let resolved_parent = spec.resolved_name();
            if let Err(e) = context.swarm().ensure_parent_session(spec).await {
                let _ = program.close_failed("EnsureSession", &e.to_string(), "ensure_parent_session").await;
                yield UpdateEvent::Error {
                    stage: "ensure_parent_session".into(),
                    message: e.to_string(),
                };
                return;
            }

            // BLFX-9: build EnvContext from caller-provided cutoff_date /
            // blocked_urls. Bad cutoff_date string aborts the update with
            // a clear error rather than silently disabling the defense.
            let env = match build_env_context(cutoff_date, blocked_urls) {
                Ok(env) => env,
                Err(message) => {
                    let _ = program.close_failed("InvalidCutoff", &message, "build_env_context").await;
                    yield UpdateEvent::Error {
                        stage: "validate".into(),
                        message,
                    };
                    return;
                }
            };

            // Spawn the actual work in the background. The stream returns
            // immediately after yielding Started; the background task drives
            // the trial fan-out + aggregation + program close.
            let context_for_task = context.clone();
            tokio::spawn(run_update_in_background(
                context_for_task,
                program,
                program_id.clone(),
                new_evidence,
                trials,
                resolved_parent,
                allowed_tools,
                iterative_max_steps,
                env,
            ));

            yield UpdateEvent::Started {
                program_id: update_program_id,
                prior: empty_state(DEFAULT_PRIOR, ForecastConfidence::SinglePass),
            };
        }
    }

    /// Record the ground truth outcome of a resolved forecast. Reads the
    /// program's artifact, extracts the predicted probability, and appends
    /// a [`crate::mneme::calibration::ResolvedObservation`] to the
    /// substrate's calibration store at `programs_root/_calibration/`.
    /// If the post-append history crosses [`crate::mneme::calibration::COLD_START_THRESHOLD`],
    /// emits a `Recalibrated` event with the newly-fit Platt parameters.
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
        let context = self.context.clone();
        stream! {
            let resolved_at_dt = match resolved_at.as_deref() {
                None => Utc::now(),
                Some(s) => match DateTime::parse_from_rfc3339(s) {
                    Ok(dt) => dt.with_timezone(&Utc),
                    Err(e) => {
                        yield ResolveEvent::Error {
                            message: format!("could not parse resolved_at `{}`: {}", s, e),
                        };
                        return;
                    }
                },
            };

            // Look up the predicted probability from the program's artifact.
            let artifact_path = context
                .programs_root()
                .join(&program_id)
                .join("artifact.json");
            let predicted = match std::fs::read(&artifact_path) {
                Ok(bytes) => match serde_json::from_slice::<Value>(&bytes) {
                    Ok(v) => match v.get("probability").and_then(|p| p.as_f64()) {
                        Some(p) => p,
                        None => {
                            yield ResolveEvent::Error {
                                message: format!(
                                    "artifact at {} has no `probability` field",
                                    artifact_path.display()
                                ),
                            };
                            return;
                        }
                    },
                    Err(e) => {
                        yield ResolveEvent::Error {
                            message: format!("artifact JSON parse: {}", e),
                        };
                        return;
                    }
                },
                Err(e) => {
                    yield ResolveEvent::Error {
                        message: format!("could not read {}: {}", artifact_path.display(), e),
                    };
                    return;
                }
            };

            // Open the calibration store and record.
            let calibration_root = context.programs_root().join("_calibration");
            let store = match crate::mneme::calibration::CalibrationStore::open(&calibration_root) {
                Ok(s) => s,
                Err(e) => {
                    yield ResolveEvent::Error {
                        message: format!("calibration store open: {}", e),
                    };
                    return;
                }
            };
            let obs = crate::mneme::calibration::ResolvedObservation {
                program_id: program_id.clone(),
                predicted,
                actual,
                deadline: None,
                resolved_at: resolved_at_dt,
            };
            if let Err(e) = store.record(&obs) {
                yield ResolveEvent::Error {
                    message: format!("calibration store record: {}", e),
                };
                return;
            }

            yield ResolveEvent::Resolved {
                program_id,
                predicted,
                actual,
            };

            // If `record` re-fit the bias (post-cold-start), surface it.
            if let Ok(Some(params)) = store.read_bias() {
                yield ResolveEvent::Recalibrated {
                    a: params.a,
                    b: params.b,
                };
            }
        }
    }
}

/// The body of `forecast.update`'s background task. Owns the program for its
/// lifetime; closes it (completed or failed) before returning.
async fn run_update_in_background(
    context: Arc<MnemeContext>,
    program: crate::mneme::program::Program,
    parent_question_program_id: String,
    new_evidence: String,
    trials: u8,
    parent_session: String,
    allowed_tools: Option<Vec<String>>,
    iterative_max_steps: Option<u8>,
    env: Option<crate::activations::forecast::EnvContext>,
) {
    let prompt = format!(
        "Forecast update for question program {}.\n\nNew evidence:\n{}\n\nReturn a JSON object with fields `probability` (a number in [0,1]) and `summary` (a one-paragraph evidence summary).",
        parent_question_program_id, new_evidence
    );
    let response_schema = json!({
        "type": "object",
        "properties": {
            "probability": {"type": "number", "minimum": 0.0, "maximum": 1.0},
            "summary": {"type": "string"}
        },
        "required": ["probability", "summary"]
    });

    // Default to iterative T_max=5 per BLFX-18 result (paired n=94: BI 84
    // vs crowd 57, delta +26.61 BI with 95% CI [-0.1047, -0.0284] excluding
    // 0 — iterative wins decisively at p<0.05).
    // Treat Some(0) as explicit opt-out (single-shot) so callers can revert
    // without juggling Option semantics over the wire.
    let iterative = match iterative_max_steps {
        None => Some(DEFAULT_ITERATIVE_MAX_STEPS),
        Some(0) => None,
        Some(t) => Some(t),
    };

    let params = TrialParams {
        parent_session,
        prompt,
        response_schema,
        n: trials,
        diversify: Some("Reasoning style #%i (analytic / contrarian / base-rate-grounded)".into()),
        timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECS),
        allowed_tools,
        iterative_max_steps: iterative,
        env,
    };

    let batch = match context.swarm().trial(&program, params).await {
        Ok(b) => b,
        Err(e) => {
            let _ = program.close_failed("SwarmError", &e.to_string(), "swarm.trial").await;
            return;
        }
    };

    if batch.success_count() == 0 {
        let msg = format!("all {} trials failed", batch.failure_count());
        let _ = program.close_failed("AllTrialsFailed", &msg, "swarm.trial").await;
        return;
    }

    let trial_responses: Vec<_> = batch.successes.iter().map(|t| t.response.clone()).collect();

    let logit = match aggregate(&trial_responses, &AggregationRule::LogitShrinkage {
        field: "probability".into(),
        prior: DEFAULT_PRIOR,
        lambda: DEFAULT_LAMBDA,
    }) {
        Ok(v) => v,
        Err(e) => {
            let _ = program.close_failed("AggregateError", &e.to_string(), "aggregate.logit").await;
            return;
        }
    };

    // Merge structured fields across trials. Each trial may produce
    // evidence_for / evidence_against / open_questions; we concatenate
    // (preserving trial order) for evidence and union for open_questions
    // (de-duplicating exact string matches).
    let parsed_trials: Vec<TrialResponse> = trial_responses
        .iter()
        .filter_map(|v| serde_json::from_value::<TrialResponse>(v.clone()).ok())
        .collect();

    let mut evidence_for: Vec<EvidenceItem> = vec![];
    let mut evidence_against: Vec<EvidenceItem> = vec![];
    let mut open_questions_set: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for t in &parsed_trials {
        evidence_for.extend(t.evidence_for.iter().cloned());
        evidence_against.extend(t.evidence_against.iter().cloned());
        for q in &t.open_questions {
            open_questions_set.insert(q.clone());
        }
    }
    let open_questions: Vec<String> = open_questions_set.into_iter().collect();

    let raw_probability = logit["aggregated"].as_f64().unwrap_or(DEFAULT_PRIOR);
    let n_trials = batch.success_count() as u8;
    let confidence = if n_trials > 1 {
        ForecastConfidence::MultiTrial
    } else {
        ForecastConfidence::SinglePass
    };

    // MNEME-28: apply Platt calibration when the calibration store has
    // fit parameters. Cold-start (None bias) → identity; post-cold-start
    // (Some bias) → calibrated. raw_probability is always the pre-Platt
    // aggregate; probability is what consumers should use.
    let calibration_root = context.programs_root().join("_calibration");
    let probability = match crate::mneme::calibration::CalibrationStore::open(&calibration_root) {
        Ok(store) => match store.read_bias() {
            Ok(Some(params)) => {
                match crate::mneme::calibration::platt_apply(params, raw_probability) {
                    Ok(calibrated) => {
                        tracing::info!(
                            raw = %format_args!("{:.4}", raw_probability),
                            calibrated = %format_args!("{:.4}", calibrated),
                            a = %format_args!("{:.4}", params.a),
                            b = %format_args!("{:.4}", params.b),
                            "calibration_applied"
                        );
                        calibrated
                    }
                    Err(e) => {
                        tracing::warn!("platt_apply failed, using raw: {}", e);
                        raw_probability
                    }
                }
            }
            Ok(None) => raw_probability, // cold-start: identity
            Err(e) => {
                tracing::warn!("calibration store read_bias failed, using raw: {}", e);
                raw_probability
            }
        },
        Err(e) => {
            tracing::warn!("calibration store open failed, using raw: {}", e);
            raw_probability
        }
    };

    let mut state = ForecastState {
        probability,
        raw_probability: Some(raw_probability),
        confidence,
        evidence_for,
        evidence_against,
        open_questions,
        summary: String::new(),
        n_trials,
        prior_used: None,
        belief_schema_version: BELIEF_SCHEMA_VERSION.to_string(),
    };
    // Auto-generate the summary from the structured fields. If trials produced
    // no structured evidence (legacy v0.1.0 trial output), fall back to
    // concatenating their prose summaries instead.
    state.rerender_summary();
    if state.summary.is_empty() {
        let legacy_summary = aggregate(
            &trial_responses,
            &AggregationRule::ConcatEvidence {
                field: "summary".into(),
                separator: "\n\n".into(),
            },
        )
        .ok()
        .and_then(|v| v["aggregated"].as_str().map(String::from))
        .unwrap_or_default();
        state.summary = legacy_summary;
    }

    if let Err(e) = program.close_completed(&state, BELIEF_SCHEMA_VERSION).await {
        tracing::error!("forecast.update close_completed failed: {}", e);
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

    /// Poll a program's manifest until status is no longer "running" or
    /// the timeout expires.
    async fn await_program_status(
        programs_root: &std::path::Path,
        program_id: &str,
        timeout: std::time::Duration,
    ) -> serde_json::Value {
        let start = std::time::Instant::now();
        let manifest_path = programs_root.join(program_id).join("manifest.json");
        loop {
            if let Ok(text) = std::fs::read_to_string(&manifest_path) {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                    if v["status"].as_str() != Some("running") {
                        return v;
                    }
                }
            }
            if start.elapsed() > timeout {
                panic!("program {} did not finish within {:?}", program_id, timeout);
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    }

    #[tokio::test]
    async fn create_returns_program_id_and_writes_directory() {
        let (dir, forecast) = forecast_with_stub();
        let future = (Utc::now() + ChronoDuration::days(30))
            .format("%Y-%m-%d")
            .to_string();
        let stream = forecast
            .create("Will X?".into(), "X observed by deadline".into(), future, None)
            .await;
        let mut s = Box::pin(stream);
        let evt = s.next().await.expect("event");
        match evt {
            CreateEvent::Created { program_id, .. } => {
                assert!(!program_id.is_empty());
                let prog_dir = dir.path().join(&program_id);
                assert!(prog_dir.exists());
                assert!(prog_dir.join("manifest.json").exists());
                assert!(prog_dir.join("artifact.json").exists());
            }
            other => panic!("unexpected: {:?}", other),
        }
    }

    #[tokio::test]
    async fn create_rejects_past_deadline() {
        let (_dir, forecast) = forecast_with_stub();
        let stream = forecast
            .create("Will X?".into(), "...".into(), "2024-01-01".into(), None)
            .await;
        let mut s = Box::pin(stream);
        let evt = s.next().await.expect("event");
        assert!(matches!(evt, CreateEvent::ResolvabilityFailed { .. }));
    }

    #[tokio::test]
    async fn create_rejects_unparseable_deadline() {
        let (_dir, forecast) = forecast_with_stub();
        let stream = forecast
            .create("Will X?".into(), "...".into(), "not a date".into(), None)
            .await;
        let mut s = Box::pin(stream);
        let evt = s.next().await.expect("event");
        assert!(matches!(evt, CreateEvent::ResolvabilityFailed { .. }));
    }

    #[tokio::test]
    async fn create_rejects_out_of_range_trials() {
        let (_dir, forecast) = forecast_with_stub();
        let future = (Utc::now() + ChronoDuration::days(30))
            .format("%Y-%m-%d")
            .to_string();
        let stream = forecast
            .create("Will X?".into(), "...".into(), future, Some(99))
            .await;
        let mut s = Box::pin(stream);
        let evt = s.next().await.expect("event");
        assert!(matches!(evt, CreateEvent::Error { .. }));
    }

    #[tokio::test]
    async fn update_returns_started_immediately() {
        let (dir, forecast) = forecast_with_mock(vec![
            json!({"probability": 0.6, "summary": "trial 1"}),
            json!({"probability": 0.7, "summary": "trial 2"}),
        ]);
        let stream = forecast
            .update("Q-001".into(), "evidence".into(), Some(2), None, None, None, None, None)
            .await;
        let mut s = Box::pin(stream);
        let evt = s.next().await.expect("event");
        let program_id = match evt {
            UpdateEvent::Started { program_id, .. } => program_id,
            other => panic!("unexpected: {:?}", other),
        };
        // Stream should end after Started (fire-and-return).
        assert!(s.next().await.is_none(), "stream should end after Started");

        // Background task completes; manifest reflects it.
        let manifest = await_program_status(dir.path(), &program_id, std::time::Duration::from_secs(5)).await;
        assert_eq!(manifest["status"], "completed");
    }

    #[tokio::test]
    async fn update_with_stub_runtime_closes_program_as_failed() {
        let (dir, forecast) = forecast_with_stub();
        let stream = forecast
            .update("Q-001".into(), "evidence".into(), None, None, None, None, None, None)
            .await;
        let mut s = Box::pin(stream);
        let evt = s.next().await.expect("event");
        let program_id = match evt {
            UpdateEvent::Started { program_id, .. } => program_id,
            other => panic!("unexpected: {:?}", other),
        };
        let manifest = await_program_status(dir.path(), &program_id, std::time::Duration::from_secs(2)).await;
        assert_eq!(manifest["status"], "failed");
    }

    #[tokio::test]
    async fn update_validation_error_does_not_open_program() {
        let (_dir, forecast) = forecast_with_stub();
        let stream = forecast
            .update("Q-001".into(), "evidence".into(), Some(99), None, None, None, None, None)
            .await;
        let mut s = Box::pin(stream);
        let evt = s.next().await.expect("event");
        match evt {
            UpdateEvent::Error { stage, .. } => assert_eq!(stage, "validate"),
            other => panic!("unexpected: {:?}", other),
        }
    }

    #[tokio::test]
    async fn update_applies_platt_when_calibration_store_has_bias() {
        // Pre-seed the calibration store at the temp programs_root so
        // forecast.update reads non-cold-start bias and applies it.
        use crate::mneme::calibration::{CalibrationStore, ResolvedObservation, COLD_START_THRESHOLD};
        use chrono::Utc;

        let (dir, forecast) = forecast_with_mock(vec![
            // Trial responses centered around 0.7 — somewhere away from 0.5
            // so calibration will visibly move the value.
            json!({"probability": 0.72, "summary": "trial 0"}),
            json!({"probability": 0.68, "summary": "trial 1"}),
        ]);
        let calibration_root = dir.path().join("_calibration");
        let store = CalibrationStore::open(&calibration_root).unwrap();
        // Seed enough observations to cross COLD_START_THRESHOLD with a
        // pattern that produces a non-identity Platt fit. Mix of correct
        // and wrong predictions at varied confidence so a≠1, b≠0.
        for i in 0..(COLD_START_THRESHOLD + 5) {
            let predicted = if i % 3 == 0 { 0.85 } else { 0.45 };
            let actual = i % 2 == 0;
            store
                .record(&ResolvedObservation {
                    program_id: format!("seed-{}", i),
                    predicted,
                    actual,
                    deadline: None,
                    resolved_at: Utc::now(),
                })
                .unwrap();
        }
        let bias = store.read_bias().unwrap().expect("post-cold-start bias");
        // Sanity: the seeded data should NOT have produced an identity fit.
        assert!(
            (bias.a - 1.0).abs() > 1e-6 || bias.b.abs() > 1e-6,
            "seeded bias should be non-identity; got a={}, b={}",
            bias.a,
            bias.b
        );

        // Run forecast.update.
        let stream = forecast
            .update("Q-PLATT-001".into(), "evidence".into(), Some(2), None, None, None, None, None)
            .await;
        let mut s = Box::pin(stream);
        let evt = s.next().await.expect("event");
        let program_id = match evt {
            UpdateEvent::Started { program_id, .. } => program_id,
            other => panic!("unexpected: {:?}", other),
        };

        // Wait for completion + read the artifact.
        let _manifest =
            await_program_status(dir.path(), &program_id, std::time::Duration::from_secs(5)).await;
        let artifact_bytes =
            std::fs::read(dir.path().join(&program_id).join("artifact.json")).unwrap();
        let artifact: Value = serde_json::from_slice(&artifact_bytes).unwrap();

        let calibrated = artifact["probability"].as_f64().unwrap();
        let raw = artifact["raw_probability"]
            .as_f64()
            .expect("raw_probability present in v0.3 artifact");
        // Calibrated must differ from raw — Platt was applied.
        assert!(
            (calibrated - raw).abs() > 1e-6,
            "calibrated {} should differ from raw {} when bias != identity",
            calibrated,
            raw
        );
        // Both must be valid probabilities.
        assert!((0.0..=1.0).contains(&calibrated));
        assert!((0.0..=1.0).contains(&raw));
        // Schema version should be 0.3.0.
        assert_eq!(artifact["belief_schema_version"], "0.3.0");
    }

    #[tokio::test]
    async fn resolve_errors_when_program_artifact_missing() {
        let (_dir, forecast) = forecast_with_stub();
        let stream = forecast.resolve("nope".into(), true, None).await;
        let mut s = Box::pin(stream);
        match s.next().await.expect("event") {
            ResolveEvent::Error { message } => {
                assert!(message.contains("could not read"), "got: {}", message);
            }
            other => panic!("unexpected: {:?}", other),
        }
    }

    #[tokio::test]
    async fn resolve_records_observation_and_yields_resolved() {
        // Run an update first so an artifact exists, then resolve it.
        let (dir, forecast) = forecast_with_mock(vec![
            json!({"probability": 0.62, "summary": "trial 0"}),
            json!({"probability": 0.55, "summary": "trial 1"}),
        ]);
        let stream = forecast
            .update("Q-RESOLVE-001".into(), "evidence".into(), Some(2), None, None, None, None, None)
            .await;
        let mut s = Box::pin(stream);
        let evt = s.next().await.expect("update started");
        let program_id = match evt {
            UpdateEvent::Started { program_id, .. } => program_id,
            other => panic!("unexpected: {:?}", other),
        };
        let _ = await_program_status(dir.path(), &program_id, std::time::Duration::from_secs(5)).await;

        // Read the predicted probability for the assertion.
        let artifact_bytes = std::fs::read(dir.path().join(&program_id).join("artifact.json")).unwrap();
        let artifact: Value = serde_json::from_slice(&artifact_bytes).unwrap();
        let predicted_in_artifact = artifact["probability"].as_f64().unwrap();

        // Resolve.
        let stream = forecast.resolve(program_id.clone(), true, None).await;
        let mut s = Box::pin(stream);
        match s.next().await.expect("resolve event") {
            ResolveEvent::Resolved { program_id: pid, predicted, actual } => {
                assert_eq!(pid, program_id);
                assert!((predicted - predicted_in_artifact).abs() < 1e-9);
                assert!(actual);
            }
            other => panic!("unexpected: {:?}", other),
        }

        // Calibration store should have one row.
        let store = crate::mneme::calibration::CalibrationStore::open(
            dir.path().join("_calibration"),
        ).unwrap();
        let history = store.read_history().unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].program_id, program_id);
        assert!(history[0].actual);
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
