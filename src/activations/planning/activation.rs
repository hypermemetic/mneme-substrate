//! Planning activation — epic decomposition as a Plexus skill.
//!
//! Methods:
//! - `epic` — break a goal into a dependency DAG of tickets
//!
//! ## Status (skeleton)
//!
//! Type signatures and event flow are wired. The method returns early with
//! `Error { stage: "swarm-not-wired" }` until the [`SwarmRuntime`] has a real
//! implementation (Phase 2 follow-up).
//!
//! See the planning skill documentation for the contract.

use std::sync::Arc;

use super::types::*;
use async_stream::stream;
use futures::Stream;

use crate::mneme::context::MnemeContext;

/// The planning activation. Holds an [`MnemeContext`] for opening programs
/// and accessing the orchestration runtime.
#[derive(Clone)]
pub struct Planning {
    #[allow(dead_code)]
    context: Arc<MnemeContext>,
}

impl Planning {
    pub fn new(context: Arc<MnemeContext>) -> Self {
        Self { context }
    }
}

#[plexus_macros::activation(
    namespace = "planning",
    version = "0.1.0",
    description = "Breaks a goal into a dependency DAG of tickets"
)]
impl Planning {
    /// Break a goal into a dependency DAG of tickets.
    ///
    /// Decomposes a high-level goal into an epic of atomic, independent tickets
    /// arranged in a dependency DAG. The DAG is verified for cycle safety and
    /// critical path analysis. Tickets are written to disk and cross-linked.
    #[plexus_macros::method(params(
        goal = "The high-level goal to decompose",
        constraints = "List of architectural or organizational constraints",
        epic_prefix = "Prefix for generated ticket IDs (e.g., 'EPIC')",
        domain_context = "Optional domain-specific context",
        existing_work = "Optional reference to existing tickets or prior decompositions"
    ))]
    async fn epic(
        &self,
        goal: String,
        constraints: Vec<String>,
        epic_prefix: String,
        domain_context: Option<String>,
        existing_work: Option<String>,
    ) -> impl Stream<Item = EpicEvent> + Send + 'static {
        stream! {
            yield EpicEvent::Started;
            let _ = (goal, constraints, epic_prefix, domain_context, existing_work);
            // Skeleton: in the real impl we'd
            //   1. Validate goal and constraints
            //   2. Call swarm to decompose into tickets
            //   3. Build dependency DAG
            //   4. Run DAG checks (cycles, critical path, etc.)
            //   5. Write epic file and child tickets
            //   6. Emit Completed with metadata
            //
            // Until SwarmRuntime is wired, emit a representative failure so
            // consumers see the shape and the error handling.
            yield EpicEvent::Error {
                stage: "swarm-not-wired".into(),
                message: "planning.epic is a skeleton; SwarmRuntime is stubbed (Phase 2 follow-up)".into(),
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mneme::context::MnemeContext;
    use futures::StreamExt;
    use tempfile::TempDir;

    #[tokio::test]
    async fn epic_emits_started_then_swarm_not_wired_error() {
        let _dir = TempDir::new().unwrap();
        let context = Arc::new(MnemeContext::with_stub_swarm(_dir.path()));
        let planning = Planning::new(context);
        let stream = planning.epic(
            "Implement multi-tenant architecture".into(),
            vec!["Must support 1000s of tenants".into()],
            "ARCH".into(),
            None,
            None,
        )
        .await;
        let mut stream = Box::pin(stream);

        // First event: Started
        let evt = stream.next().await.expect("event");
        assert!(matches!(evt, EpicEvent::Started));

        // Second event: Error with swarm-not-wired stage
        let evt = stream.next().await.expect("event");
        match evt {
            EpicEvent::Error { stage, message } => {
                assert_eq!(stage, "swarm-not-wired");
                assert!(message.contains("SwarmRuntime"));
            }
            other => panic!("unexpected event: {:?}", other),
        }

        // No more events
        assert!(stream.next().await.is_none());
    }

    #[test]
    fn epic_event_started_has_correct_tag() {
        let evt = EpicEvent::Started;
        let v = serde_json::to_value(&evt).unwrap();
        assert_eq!(v["type"], "started");
    }

    #[test]
    fn epic_event_completed_has_all_fields() {
        let evt = EpicEvent::Completed {
            epic_id: "EPIC-001".into(),
            overview: serde_json::json!({"goal": "test"}),
            ticket_count: 3,
            dag_check: serde_json::json!({"valid": true}),
            child_program_ids: vec!["EPIC-002".into()],
        };
        let v = serde_json::to_value(&evt).unwrap();
        assert_eq!(v["type"], "completed");
        assert!(v.get("epic_id").is_some());
        assert!(v.get("overview").is_some());
        assert!(v.get("ticket_count").is_some());
        assert!(v.get("dag_check").is_some());
        assert!(v.get("child_program_ids").is_some());
    }

    #[test]
    fn epic_event_error_has_stage_and_message() {
        let evt = EpicEvent::Error {
            stage: "decomposition".into(),
            message: "goal too vague".into(),
        };
        let v = serde_json::to_value(&evt).unwrap();
        assert_eq!(v["type"], "error");
        assert_eq!(v["stage"], "decomposition");
        assert_eq!(v["message"], "goal too vague");
    }
}
