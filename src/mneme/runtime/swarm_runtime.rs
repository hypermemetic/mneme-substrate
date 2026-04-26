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
}
