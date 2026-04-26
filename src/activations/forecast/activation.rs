//! Forecast activation — BLF binary forecasting as a Plexus skill.
//!
//! Methods:
//! - `create` — register a question with a binary, dated, observable resolution criterion
//! - `update` — given a prior + new evidence, produce a new belief state
//! - `resolve` — record ground truth; refits Platt parameters when threshold crossed
//!
//! ## Status (skeleton)
//!
//! Type signatures and event flow are wired. Methods that need swarm.trial
//! return early with `Error { stage: "swarm-not-wired" }` until the
//! [`SwarmRuntime`] has a real implementation (Phase 2 follow-up). Aggregation
//! and calibration math IS live and tested.
//!
//! See [`MNEME-6`](../../../mneme/plans/MNEME/MNEME-6.md) for the contract.

use super::types::*;
use async_stream::stream;
use chrono::{DateTime, NaiveDate, Utc};
use futures::Stream;

/// The forecast activation. Stateless; per-question state lives on disk under
/// the program directory.
#[derive(Clone, Default)]
pub struct Forecast;

impl Forecast {
    pub const fn new() -> Self {
        Forecast
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
    /// the future. On success, returns the program id under which subsequent
    /// `forecast.update` calls accumulate.
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
        stream! {
            // Resolvability gate: deadline must parse and be in the future.
            match parse_deadline(&deadline) {
                Err(reason) => {
                    yield CreateEvent::ResolvabilityFailed { reason };
                    return;
                }
                Ok(deadline_dt) if deadline_dt < Utc::now() => {
                    yield CreateEvent::ResolvabilityFailed {
                        reason: format!("deadline {} is in the past", deadline),
                    };
                    return;
                }
                Ok(_) => {}
            }
            let trials = trials.unwrap_or(3);
            if !(1..=16).contains(&trials) {
                yield CreateEvent::Error {
                    message: format!("trials must be in 1..=16, got {}", trials),
                };
                return;
            }
            // Skeleton: program directory + initial belief state would be written here
            // by the program lifecycle middleware (Phase 2 follow-up). For now we
            // emit a synthetic program_id so the event shape is exercised.
            let program_id = uuid::Uuid::new_v4().to_string();
            let _ = (resolution_criterion,); // referenced via stage when real
            yield CreateEvent::Created {
                program_id,
                question,
                deadline,
            };
        }
    }

    /// Update an existing forecast with new evidence.
    ///
    /// Reads the prior belief state, runs `swarm.trial(n=trials)` conditioned
    /// on the prior, aggregates with `LogitShrinkage` + `ConcatEvidence`,
    /// applies calibration, writes the new state and emits Completed.
    #[plexus_macros::method(streaming, params(
        program_id = "Program id returned by forecast.create or a prior update",
        new_evidence = "Free-form evidence to condition this update on (may be empty)",
        trials = "Number of trials for this update (1..=16, defaults to question's setting)"
    ))]
    async fn update(
        &self,
        program_id: String,
        new_evidence: String,
        trials: Option<u8>,
    ) -> impl Stream<Item = UpdateEvent> + Send + 'static {
        stream! {
            let trials = trials.unwrap_or(3);
            if !(1..=16).contains(&trials) {
                yield UpdateEvent::Error {
                    stage: "validate".into(),
                    message: format!("trials must be in 1..=16, got {}", trials),
                };
                return;
            }
            let _ = new_evidence; // would feed into the prompt template
            // Skeleton: in the real impl we'd
            //   1. Read prior from programs/<id>/belief_state.md
            //   2. Compose system prompt = forecast SKILL.md + question + criterion + prior summary
            //   3. Create or fork parent claudecode session
            //   4. Call swarm.trial via the SwarmRuntime
            //   5. Call swarm.aggregate (LogitShrinkage + ConcatEvidence)
            //   6. Apply calibration
            //   7. Write belief_state.md, emit Completed
            //
            // Until SwarmRuntime is wired, emit a representative failure so
            // consumers see the shape and the error handling.
            yield UpdateEvent::Error {
                stage: "swarm-not-wired".into(),
                message: format!(
                    "forecast.update is a skeleton; SwarmRuntime is stubbed (Phase 2 follow-up). \
                    Would have run {} trials for program {}.",
                    trials, program_id
                ),
            };
        }
    }

    /// Record the ground truth outcome of a resolved forecast.
    ///
    /// Appends to `programs/_calibration/history.jsonl`. If the post-append
    /// count crosses [`crate::mneme::calibration::COLD_START_THRESHOLD`], refits
    /// Platt parameters and emits Recalibrated.
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
        stream! {
            let _ = (program_id, actual, resolved_at);
            // Skeleton: needs the calibration store handle wired in via the
            // substrate runtime. Phase 2 follow-up.
            yield ResolveEvent::Error {
                message: "forecast.resolve is a skeleton; needs CalibrationStore wired into the substrate runtime"
                    .to_string(),
            };
        }
    }
}

fn parse_deadline(s: &str) -> Result<DateTime<Utc>, String> {
    // Accept full RFC3339 or bare YYYY-MM-DD (interpret as end-of-day UTC).
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
    use chrono::{Duration, Utc};
    use futures::StreamExt;

    // The macro generates async fn that returns Future<Output = Stream<...>>,
    // so test pattern is: await the call to get the Stream, then pin and poll.

    async fn first_create_event(
        forecast: &Forecast,
        question: &str,
        criterion: &str,
        deadline: &str,
        trials: Option<u8>,
    ) -> CreateEvent {
        let stream = forecast
            .create(question.into(), criterion.into(), deadline.into(), trials)
            .await;
        let mut stream = Box::pin(stream);
        stream.next().await.expect("event")
    }

    #[tokio::test]
    async fn create_returns_program_id_on_valid_input() {
        let forecast = Forecast::new();
        let future = (Utc::now() + Duration::days(30))
            .format("%Y-%m-%d")
            .to_string();
        let evt = first_create_event(
            &forecast,
            "Will X?",
            "X observed in canonical source by deadline",
            &future,
            None,
        )
        .await;
        match evt {
            CreateEvent::Created { program_id, .. } => assert!(!program_id.is_empty()),
            other => panic!("unexpected: {:?}", other),
        }
    }

    #[tokio::test]
    async fn create_rejects_past_deadline() {
        let forecast = Forecast::new();
        let evt =
            first_create_event(&forecast, "Will X?", "...", "2024-01-01", None).await;
        assert!(matches!(evt, CreateEvent::ResolvabilityFailed { .. }));
    }

    #[tokio::test]
    async fn create_rejects_unparseable_deadline() {
        let forecast = Forecast::new();
        let evt =
            first_create_event(&forecast, "Will X?", "...", "not a date", None).await;
        assert!(matches!(evt, CreateEvent::ResolvabilityFailed { .. }));
    }

    #[tokio::test]
    async fn create_rejects_out_of_range_trials() {
        let forecast = Forecast::new();
        let future = (Utc::now() + Duration::days(30))
            .format("%Y-%m-%d")
            .to_string();
        let evt = first_create_event(&forecast, "Will X?", "...", &future, Some(99)).await;
        assert!(matches!(evt, CreateEvent::Error { .. }));
    }

    #[tokio::test]
    async fn update_skeleton_returns_swarm_not_wired_error() {
        let forecast = Forecast::new();
        let stream = forecast.update("p1".into(), "evidence".into(), None).await;
        let mut stream = Box::pin(stream);
        let evt = stream.next().await.expect("event");
        match evt {
            UpdateEvent::Error { stage, .. } => assert_eq!(stage, "swarm-not-wired"),
            other => panic!("unexpected: {:?}", other),
        }
    }

    #[tokio::test]
    async fn resolve_skeleton_returns_error() {
        let forecast = Forecast::new();
        let stream = forecast.resolve("p1".into(), true, None).await;
        let mut stream = Box::pin(stream);
        let evt = stream.next().await.expect("event");
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
