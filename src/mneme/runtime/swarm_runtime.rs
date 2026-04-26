//! Swarm runtime — the substrate-side implementation of the swarm primitives.
//!
//! ## What this needs to do
//!
//! Implement the `swarm` activation methods (`trial`, `aggregate`, eventually
//! `sequential` and `race`) by composing `claudecode` operations. For
//! `swarm.trial` specifically:
//!
//! 1. Fork the parent claudecode session N times (one per trial).
//! 2. Attribute each fork to the calling program (via [`SessionAttribution`]).
//! 3. Register a [`RespondTool`] for the calling program in the [`ToolRegistry`].
//! 4. Send the same prompt to each fork via `chat_async` + `poll`.
//! 5. Await all trials with a timeout; collect the `respond` tool payloads.
//! 6. Validate payloads against the schema (via [`crate::mneme::respond::validate`]).
//! 7. Return a [`TrialBatch`] of successes + failures.
//! 8. Record one [`TraceEntry`] for the whole swarm.trial call against the program.
//!
//! Aggregation is pure: just call [`crate::mneme::swarm::aggregate::aggregate`]
//! with the rule and the per-trial responses.
//!
//! ## Where this hooks in
//!
//! Needs a handle to the substrate's claudecode activation instance (to call
//! its public methods directly, not through Plexus dispatch — see the
//! "Plexus is the boundary protocol" architecture decision in mneme/README.md).
//!
//! The substrate's `builder.rs` constructs the claudecode activation and the
//! DynamicHub. The same construction site can wire the SwarmRuntime with a
//! handle to claudecode.
//!
//! ## Status
//!
//! Stub for the actual claudecode-driving methods (which need a live
//! ClaudeCode handle and the fork/chat/poll round-trip). Pure aggregation is
//! fully implemented and tested via [`crate::mneme::swarm::aggregate`].
//!
//! The trait below is the contract. A real impl lands when the claudecode
//! handle wiring is in place; a mock impl in tests demonstrates the shape.

use std::time::Duration;

use serde_json::Value;

use crate::mneme::program::Program;
use crate::mneme::respond::RespondError;
use crate::mneme::swarm::{
    aggregate::{aggregate, AggregateError, AggregationRule},
    TrialBatch,
};

/// Errors raised by swarm runtime operations.
#[derive(Debug, thiserror::Error)]
pub enum SwarmError {
    #[error("validation: n={n} exceeds cap={cap}")]
    NCap { n: u8, cap: u8 },
    #[error("respond: {0}")]
    Respond(#[from] RespondError),
    #[error("aggregate: {0}")]
    Aggregate(#[from] AggregateError),
    #[error("not implemented yet: {0}")]
    NotImplemented(&'static str),
}

/// Cap on N for `swarm.trial`. Pending MNEME-S03 spike findings.
pub const SWARM_TRIAL_N_CAP: u8 = 16;

/// Parameters for a `swarm.trial` invocation.
#[derive(Debug, Clone)]
pub struct TrialParams {
    pub parent_session: String,
    pub prompt: String,
    pub response_schema: Value,
    pub n: u8,
    pub diversify: Option<String>,
    pub timeout: Duration,
}

impl TrialParams {
    pub fn validate(&self) -> Result<(), SwarmError> {
        if self.n == 0 || self.n > SWARM_TRIAL_N_CAP {
            return Err(SwarmError::NCap {
                n: self.n,
                cap: SWARM_TRIAL_N_CAP,
            });
        }
        Ok(())
    }
}

/// The runtime trait. Real implementation drives claudecode; tests use mocks.
#[async_trait::async_trait]
pub trait SwarmRuntime: Send + Sync {
    /// Execute a swarm.trial fan-out against `program`'s context. Records a
    /// trace entry on the program when complete.
    async fn trial(&self, program: &Program, params: TrialParams) -> Result<TrialBatch, SwarmError>;
}

/// Pure aggregation — no runtime needed. Wraps the underlying math so callers
/// can route through the swarm namespace consistently.
pub fn run_aggregate(trials: &[Value], rule: &AggregationRule) -> Result<Value, SwarmError> {
    Ok(aggregate(trials, rule)?)
}

/// Stub implementation that always returns NotImplemented. Used until the
/// claudecode handle wiring lands. Demonstrates the trait shape; consumers
/// can compile against this.
pub struct StubSwarmRuntime;

#[async_trait::async_trait]
impl SwarmRuntime for StubSwarmRuntime {
    async fn trial(&self, _program: &Program, _params: TrialParams) -> Result<TrialBatch, SwarmError> {
        Err(SwarmError::NotImplemented(
            "SwarmRuntime::trial requires claudecode handle wiring (Phase 2 follow-up)",
        ))
    }
}

/// Test fixture: a SwarmRuntime that returns pre-loaded responses without
/// touching claudecode. Lets the full forecast pipeline (program lifecycle +
/// trial fan-out + aggregation + calibration) be exercised end-to-end in
/// unit tests, separately from the claudecode integration which is the
/// remaining substrate-level work.
///
/// Construct with [`DeterministicMockSwarmRuntime::new`] passing per-trial
/// JSON responses. The mock records a [`TraceEntry`] on the program just
/// like the real runtime will.
pub struct DeterministicMockSwarmRuntime {
    responses: std::sync::Mutex<Vec<Value>>,
}

impl DeterministicMockSwarmRuntime {
    /// New mock pre-loaded with `responses`. Each call to [`trial`] consumes
    /// the front N (where N comes from `params.n`) and returns them as
    /// successful trials.
    pub fn new(responses: Vec<Value>) -> Self {
        Self {
            responses: std::sync::Mutex::new(responses),
        }
    }
}

#[async_trait::async_trait]
impl SwarmRuntime for DeterministicMockSwarmRuntime {
    async fn trial(&self, program: &Program, params: TrialParams) -> Result<TrialBatch, SwarmError> {
        params.validate()?;

        let start = std::time::Instant::now();
        let mut guard = self.responses.lock().expect("poisoned");
        let n = params.n as usize;
        let take = n.min(guard.len());
        let drained: Vec<Value> = guard.drain(..take).collect();
        drop(guard);

        let mut successes = Vec::with_capacity(take);
        let mut child_session_ids = Vec::with_capacity(take);
        for (i, response) in drained.into_iter().enumerate() {
            let session_id = format!("mock-trial-{}-{}", program.id(), i);
            child_session_ids.push(session_id.clone());
            successes.push(crate::mneme::swarm::TrialResult {
                trial_index: i as u8,
                session_id,
                response,
                duration_ms: 1,
            });
        }
        let failures = (take..n)
            .map(|i| crate::mneme::swarm::TrialFailure {
                trial_index: i as u8,
                session_id: None,
                error: "mock ran out of pre-loaded responses".to_string(),
                last_payload: None,
            })
            .collect::<Vec<_>>();

        // Record the trace entry on the program, the same way the real runtime will.
        let entry = crate::mneme::program::TraceEntry::new(
            program.next_seq(),
            crate::mneme::program::TraceOp::SwarmTrial,
            serde_json::json!({
                "n": params.n,
                "parent_session": params.parent_session,
                "diversify": params.diversify,
            }),
            child_session_ids,
            crate::mneme::program::TraceOutcome::Ok,
            start.elapsed(),
        );
        program.record_trace(&entry).map_err(|e| {
            SwarmError::NotImplemented(Box::leak(format!("record_trace: {}", e).into_boxed_str()))
        })?;

        Ok(TrialBatch { successes, failures })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn n_zero_is_invalid() {
        let p = TrialParams {
            parent_session: "x".into(),
            prompt: "y".into(),
            response_schema: json!({}),
            n: 0,
            diversify: None,
            timeout: Duration::from_secs(10),
        };
        assert!(matches!(p.validate(), Err(SwarmError::NCap { .. })));
    }

    #[test]
    fn n_above_cap_is_invalid() {
        let p = TrialParams {
            parent_session: "x".into(),
            prompt: "y".into(),
            response_schema: json!({}),
            n: SWARM_TRIAL_N_CAP + 1,
            diversify: None,
            timeout: Duration::from_secs(10),
        };
        assert!(matches!(p.validate(), Err(SwarmError::NCap { .. })));
    }

    #[test]
    fn n_at_cap_is_valid() {
        let p = TrialParams {
            parent_session: "x".into(),
            prompt: "y".into(),
            response_schema: json!({}),
            n: SWARM_TRIAL_N_CAP,
            diversify: None,
            timeout: Duration::from_secs(10),
        };
        p.validate().unwrap();
    }

    #[test]
    fn run_aggregate_dispatches_to_logit() {
        let trials = vec![json!({"p": 0.5}), json!({"p": 0.5})];
        let rule = AggregationRule::LogitShrinkage {
            field: "p".into(),
            prior: 0.5,
            lambda: 0.0,
        };
        let r = run_aggregate(&trials, &rule).unwrap();
        assert!((r["aggregated"].as_f64().unwrap() - 0.5).abs() < 1e-9);
    }

    #[tokio::test]
    async fn stub_runtime_returns_not_implemented() {
        use tempfile::TempDir;
        let root = TempDir::new().unwrap();
        let prog = Program::open(root.path(), "x", json!({}), "v", "v").unwrap();
        let stub = StubSwarmRuntime;
        let params = TrialParams {
            parent_session: "p".into(),
            prompt: "go".into(),
            response_schema: json!({"type": "object"}),
            n: 3,
            diversify: None,
            timeout: Duration::from_secs(10),
        };
        let err = stub.trial(&prog, params).await.unwrap_err();
        assert!(matches!(err, SwarmError::NotImplemented(_)));
    }

    #[tokio::test]
    async fn mock_runtime_returns_preloaded_trials() {
        use tempfile::TempDir;
        let root = TempDir::new().unwrap();
        let prog = Program::open(root.path(), "x", json!({}), "v", "v").unwrap();
        let mock = DeterministicMockSwarmRuntime::new(vec![
            json!({"probability": 0.6, "summary": "trial 0"}),
            json!({"probability": 0.5, "summary": "trial 1"}),
            json!({"probability": 0.7, "summary": "trial 2"}),
        ]);
        let params = TrialParams {
            parent_session: "parent".into(),
            prompt: "Forecast X".into(),
            response_schema: json!({"type": "object"}),
            n: 3,
            diversify: None,
            timeout: Duration::from_secs(10),
        };
        let batch = mock.trial(&prog, params).await.unwrap();
        assert_eq!(batch.success_count(), 3);
        assert_eq!(batch.failure_count(), 0);
        // Trace was recorded.
        let trace_path = prog.directory().trace_path();
        let text = std::fs::read_to_string(trace_path).unwrap();
        assert_eq!(text.lines().count(), 1);
    }

    #[tokio::test]
    async fn mock_runtime_records_failures_when_underprovisioned() {
        use tempfile::TempDir;
        let root = TempDir::new().unwrap();
        let prog = Program::open(root.path(), "x", json!({}), "v", "v").unwrap();
        let mock = DeterministicMockSwarmRuntime::new(vec![json!({"probability": 0.5, "summary": "only one"})]);
        let params = TrialParams {
            parent_session: "p".into(),
            prompt: "go".into(),
            response_schema: json!({"type": "object"}),
            n: 3,
            diversify: None,
            timeout: Duration::from_secs(10),
        };
        let batch = mock.trial(&prog, params).await.unwrap();
        assert_eq!(batch.success_count(), 1);
        assert_eq!(batch.failure_count(), 2);
    }

    /// End-to-end pipeline test: program → trial → aggregate → calibration →
    /// artifact. Proves the entire forecast.update pipeline works without
    /// claudecode integration.
    #[tokio::test]
    async fn pipeline_e2e_program_trial_aggregate_calibrate_artifact() {
        use crate::mneme::calibration::{CalibrationStore, ResolvedObservation, COLD_START_THRESHOLD};
        use crate::mneme::swarm::aggregate::{aggregate, AggregationRule};
        use chrono::Utc;
        use tempfile::TempDir;

        // Set up the program directory and the calibration store.
        let root = TempDir::new().unwrap();
        let calibration_root = root.path().join("_calibration");
        let store = CalibrationStore::open(&calibration_root).unwrap();

        // Pre-populate calibration history so Platt fits (cold-start passed).
        for i in 0..COLD_START_THRESHOLD {
            store
                .record(&ResolvedObservation {
                    program_id: format!("seed-{}", i),
                    predicted: 0.6 + 0.01 * (i as f64),
                    actual: i % 2 == 0,
                    deadline: None,
                    resolved_at: Utc::now(),
                })
                .unwrap();
        }
        let bias = store.read_bias().unwrap().expect("post-cold-start should have bias");

        // Open a program for the forecast update.
        let prog = Program::open(
            root.path(),
            "forecast.update",
            json!({"question": "Will X by 2026-12-31?", "evidence": "..."}),
            "0.6.3",
            "0.1.0",
        )
        .unwrap();

        // Mock runtime returns three plausible trials.
        let mock = DeterministicMockSwarmRuntime::new(vec![
            json!({"probability": 0.62, "summary": "Trial reading 1: market signals lean positive."}),
            json!({"probability": 0.55, "summary": "Trial reading 2: mixed evidence; recent volatility."}),
            json!({"probability": 0.70, "summary": "Trial reading 3: structural factors favor outcome."}),
        ]);

        // Run the trial.
        let params = TrialParams {
            parent_session: "parent-session".into(),
            prompt: "Will X by 2026-12-31?".into(),
            response_schema: json!({
                "type": "object",
                "properties": {
                    "probability": {"type": "number", "minimum": 0.0, "maximum": 1.0},
                    "summary": {"type": "string"}
                },
                "required": ["probability", "summary"]
            }),
            n: 3,
            diversify: Some("Reasoning style #%i (analytic / contrarian / base-rate)".into()),
            timeout: Duration::from_secs(10),
        };
        let batch = mock.trial(&prog, params).await.unwrap();
        assert_eq!(batch.success_count(), 3);

        // Aggregate the trials in logit space (BLF shrinkage toward prior 0.5).
        let trial_responses: Vec<Value> =
            batch.successes.iter().map(|t| t.response.clone()).collect();
        let logit_result = aggregate(
            &trial_responses,
            &AggregationRule::LogitShrinkage {
                field: "probability".into(),
                prior: 0.5,
                lambda: 0.2,
            },
        )
        .unwrap();
        let raw_aggregated = logit_result["aggregated"].as_f64().unwrap();
        // Trial mean is ~0.62; shrunk 20% toward prior 0.5 should be in (0.5, 0.62).
        assert!(
            raw_aggregated > 0.5 && raw_aggregated < 0.62,
            "raw aggregated probability {} should sit between prior and trial mean",
            raw_aggregated
        );

        // Concatenate the trial summaries (the BLF carry-forward statistic).
        let summary_result = aggregate(
            &trial_responses,
            &AggregationRule::ConcatEvidence {
                field: "summary".into(),
                separator: "\n\n".into(),
            },
        )
        .unwrap();
        let summary = summary_result["aggregated"].as_str().unwrap();
        assert!(summary.contains("Trial reading 1"));
        assert!(summary.contains("Trial reading 2"));
        assert!(summary.contains("Trial reading 3"));
        assert!(summary.contains("[trial 1]:"));

        // Apply Platt calibration.
        let calibrated = crate::mneme::calibration::platt_apply(bias, raw_aggregated).unwrap();
        // Calibrated should still be a valid probability and shouldn't deviate
        // wildly from the raw aggregated value (Platt on a well-mixed cold-start
        // dataset is usually close to identity).
        assert!(
            calibrated > 0.0 && calibrated < 1.0,
            "calibrated probability out of range: {}",
            calibrated
        );

        // Build the artifact and write it.
        let artifact = json!({
            "probability": calibrated,
            "raw_probability": raw_aggregated,
            "summary": summary,
            "n_trials": batch.success_count(),
            "calibration_applied": true,
        });
        let dir_clone = prog.directory().clone();
        prog.close_completed(&artifact, "0.1.0").unwrap();

        // Verify the on-disk state.
        assert!(dir_clone.artifact_path().exists());
        assert!(dir_clone.manifest_path().exists());
        assert!(dir_clone.trace_path().exists());

        let manifest = dir_clone.read_manifest().unwrap();
        assert_eq!(
            manifest.status,
            crate::mneme::program::ProgramStatus::Completed
        );
        assert_eq!(manifest.entry_skill, "forecast.update");

        let trace_text = std::fs::read_to_string(dir_clone.trace_path()).unwrap();
        assert_eq!(trace_text.lines().count(), 1, "exactly one swarm.trial trace entry");

        let artifact_bytes = std::fs::read(dir_clone.artifact_path()).unwrap();
        let artifact_back: Value = serde_json::from_slice(&artifact_bytes).unwrap();
        assert!(artifact_back["probability"].as_f64().is_some());
        assert!(artifact_back["summary"].as_str().unwrap().contains("trial 1"));
    }
}
