//! Strong typing activation — domain newtype proposals as a Plexus skill.
//!
//! Methods:
//! - `propose` — proposes newtypes for distinct domain identifiers currently typed as bare String/i64/Uuid
//!
//! ## Status (skeleton)
//!
//! Type signatures and event flow are wired. The method returns early with
//! `Error { stage: "swarm-not-wired" }` until the [`SwarmRuntime`] has a real
//! implementation (Phase 2 follow-up).
//!
//! See the strong-typing skill documentation for the contract.

use std::sync::Arc;

use super::types::*;
use async_stream::stream;
use futures::Stream;

use crate::mneme::context::MnemeContext;

/// The strong_typing activation. Holds an [`MnemeContext`] for opening programs
/// and accessing the orchestration runtime.
#[derive(Clone)]
pub struct StrongTyping {
    #[allow(dead_code)]
    context: Arc<MnemeContext>,
}

impl StrongTyping {
    pub fn new(context: Arc<MnemeContext>) -> Self {
        Self { context }
    }
}

#[plexus_macros::activation(
    namespace = "strong_typing",
    version = "0.1.0",
    description = "Proposes newtypes for distinct domain identifiers currently typed as bare String/i64/Uuid"
)]
impl StrongTyping {
    /// Propose newtypes for bare identifiers in the codebase.
    ///
    /// Analyzes the codebase to identify bare String, i64, and Uuid values that
    /// represent distinct domain concepts (user IDs, tenant IDs, account IDs, etc.)
    /// and proposes newtype wrappers to strengthen the type system. Proposals
    /// include boundary checks to verify no confusion across newtype boundaries.
    #[plexus_macros::method(params(
        codebase_root = "Path to the root of the codebase to analyze",
        focus_modules = "Optional list of modules to focus analysis on (e.g., 'auth', 'billing')"
    ))]
    async fn propose(
        &self,
        codebase_root: String,
        focus_modules: Option<Vec<String>>,
    ) -> impl Stream<Item = ProposeEvent> + Send + 'static {
        stream! {
            yield ProposeEvent::Started;
            let _ = (codebase_root, focus_modules);
            // Skeleton: in the real impl we'd
            //   1. Parse the codebase
            //   2. Identify bare String/i64/Uuid identifiers
            //   3. Call swarm to propose semantically meaningful newtypes
            //   4. Verify boundary safety across proposals
            //   5. Generate newtype definitions and migration guide
            //   6. Emit Completed with proposals and checks
            //
            // Until SwarmRuntime is wired, emit a representative failure so
            // consumers see the shape and the error handling.
            yield ProposeEvent::Error {
                stage: "swarm-not-wired".into(),
                message: "strong_typing.propose is a skeleton; SwarmRuntime is stubbed (Phase 2 follow-up)".into(),
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
    async fn propose_emits_started_then_swarm_not_wired_error() {
        let _dir = TempDir::new().unwrap();
        let context = Arc::new(MnemeContext::with_stub_swarm(_dir.path()));
        let strong_typing = StrongTyping::new(context);
        let stream = strong_typing.propose(
            "/code".into(),
            Some(vec!["auth".into(), "billing".into()]),
        )
        .await;
        let mut stream = Box::pin(stream);

        // First event: Started
        let evt = stream.next().await.expect("event");
        assert!(matches!(evt, ProposeEvent::Started));

        // Second event: Error with swarm-not-wired stage
        let evt = stream.next().await.expect("event");
        match evt {
            ProposeEvent::Error { stage, message } => {
                assert_eq!(stage, "swarm-not-wired");
                assert!(message.contains("SwarmRuntime"));
            }
            other => panic!("unexpected event: {:?}", other),
        }

        // No more events
        assert!(stream.next().await.is_none());
    }

    #[test]
    fn propose_event_started_has_correct_tag() {
        let evt = ProposeEvent::Started;
        let v = serde_json::to_value(&evt).unwrap();
        assert_eq!(v["type"], "started");
    }

    #[test]
    fn propose_event_completed_has_all_fields() {
        let evt = ProposeEvent::Completed {
            proposals: vec![serde_json::json!({"newtype": "UserId"})],
            boundary_check: serde_json::json!({"valid": true}),
        };
        let v = serde_json::to_value(&evt).unwrap();
        assert_eq!(v["type"], "completed");
        assert!(v.get("proposals").is_some());
        assert!(v.get("boundary_check").is_some());
    }

    #[test]
    fn propose_event_error_has_stage_and_message() {
        let evt = ProposeEvent::Error {
            stage: "parsing".into(),
            message: "invalid module list".into(),
        };
        let v = serde_json::to_value(&evt).unwrap();
        assert_eq!(v["type"], "error");
        assert_eq!(v["stage"], "parsing");
        assert_eq!(v["message"], "invalid module list");
    }
}
