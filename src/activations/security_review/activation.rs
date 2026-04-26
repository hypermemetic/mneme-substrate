//! Security review activation — structured SOC2 security audits as a Plexus skill.
//!
//! Methods:
//! - `audit` — structured security review grouped by SOC2 control families
//!
//! ## Status (skeleton)
//!
//! Type signatures and event flow are wired. The method returns early with
//! `Error { stage: "swarm-not-wired" }` until the [`SwarmRuntime`] has a real
//! implementation (Phase 2 follow-up).
//!
//! See the security-review skill documentation for the contract.

use std::sync::Arc;

use super::types::*;
use async_stream::stream;
use futures::Stream;

use crate::mneme::context::MnemeContext;

/// The security_review activation. Holds an [`MnemeContext`] for opening programs
/// and accessing the orchestration runtime.
#[derive(Clone)]
pub struct SecurityReview {
    #[allow(dead_code)]
    context: Arc<MnemeContext>,
}

impl SecurityReview {
    pub fn new(context: Arc<MnemeContext>) -> Self {
        Self { context }
    }
}

#[plexus_macros::activation(
    namespace = "security_review",
    version = "0.1.0",
    description = "Structured security review grouped by SOC2 control families"
)]
impl SecurityReview {
    /// Perform a structured security review of the codebase.
    ///
    /// Audits the codebase against SOC2 control families, analyzing authentication
    /// models, tenant isolation, deployment topology, and other critical security
    /// boundaries. Findings are grouped by control family and cross-checked for
    /// consistency across multiple independent trials.
    #[plexus_macros::method(params(
        codebase_root = "Path to the root of the codebase to audit",
        auth_model = "Description of the authentication/authorization model",
        tenant_model = "Description of the multi-tenancy model (if applicable)",
        deployment_topology = "Description of how the system is deployed (e.g., on-prem, cloud, hybrid)",
        trials = "Number of independent audit trials for cross-validation (1..=16, default 3)"
    ))]
    async fn audit(
        &self,
        codebase_root: String,
        auth_model: String,
        tenant_model: String,
        deployment_topology: String,
        trials: Option<u8>,
    ) -> impl Stream<Item = AuditEvent> + Send + 'static {
        stream! {
            yield AuditEvent::Started;
            let _ = (codebase_root, auth_model, tenant_model, deployment_topology, trials);
            // Skeleton: in the real impl we'd
            //   1. Validate inputs and codebase path
            //   2. Call swarm to analyze code against SOC2 controls
            //   3. Run multiple trials and aggregate findings
            //   4. Cross-check for consistency
            //   5. Generate report grouped by control family
            //   6. Emit Completed with findings and metadata
            //
            // Until SwarmRuntime is wired, emit a representative failure so
            // consumers see the shape and the error handling.
            yield AuditEvent::Error {
                stage: "swarm-not-wired".into(),
                message: "security_review.audit is a skeleton; SwarmRuntime is stubbed (Phase 2 follow-up)".into(),
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
    async fn audit_emits_started_then_swarm_not_wired_error() {
        let _dir = TempDir::new().unwrap();
        let context = Arc::new(MnemeContext::with_stub_swarm(_dir.path()));
        let security_review = SecurityReview::new(context);
        let stream = security_review.audit(
            "/code".into(),
            "OAuth 2.0".into(),
            "Account-based isolation".into(),
            "Kubernetes multi-region".into(),
            Some(3),
        )
        .await;
        let mut stream = Box::pin(stream);

        // First event: Started
        let evt = stream.next().await.expect("event");
        assert!(matches!(evt, AuditEvent::Started));

        // Second event: Error with swarm-not-wired stage
        let evt = stream.next().await.expect("event");
        match evt {
            AuditEvent::Error { stage, message } => {
                assert_eq!(stage, "swarm-not-wired");
                assert!(message.contains("SwarmRuntime"));
            }
            other => panic!("unexpected event: {:?}", other),
        }

        // No more events
        assert!(stream.next().await.is_none());
    }

    #[test]
    fn audit_event_started_has_correct_tag() {
        let evt = AuditEvent::Started;
        let v = serde_json::to_value(&evt).unwrap();
        assert_eq!(v["type"], "started");
    }

    #[test]
    fn audit_event_completed_has_all_fields() {
        let evt = AuditEvent::Completed {
            findings: vec![serde_json::json!({"finding": "test"})],
            summary: serde_json::json!({"status": "pass"}),
            aggregation_metadata: serde_json::json!({"trials": 3}),
        };
        let v = serde_json::to_value(&evt).unwrap();
        assert_eq!(v["type"], "completed");
        assert!(v.get("findings").is_some());
        assert!(v.get("summary").is_some());
        assert!(v.get("aggregation_metadata").is_some());
    }

    #[test]
    fn audit_event_error_has_stage_and_message() {
        let evt = AuditEvent::Error {
            stage: "analysis".into(),
            message: "scan failed".into(),
        };
        let v = serde_json::to_value(&evt).unwrap();
        assert_eq!(v["type"], "error");
        assert_eq!(v["stage"], "analysis");
        assert_eq!(v["message"], "scan failed");
    }
}
