//! Ticketing activation — TDD ticket writing as a Plexus skill.
//!
//! Methods:
//! - `write` — compose a ticket that passes the two-stranger test
//!
//! ## Status (skeleton)
//!
//! Type signatures and event flow are wired. The method returns early with
//! `Error { stage: "swarm-not-wired" }` until the [`SwarmRuntime`] has a real
//! implementation (Phase 2 follow-up).
//!
//! See the ticketing skill documentation for the contract.

use super::types::*;
use async_stream::stream;
use futures::Stream;

/// The ticketing activation. Stateless; tickets are written to disk.
#[derive(Clone, Default)]
pub struct Ticketing;

impl Ticketing {
    pub const fn new() -> Self {
        Ticketing
    }
}

#[plexus_macros::activation(
    namespace = "ticketing",
    version = "0.1.0",
    description = "Writes a TDD ticket that passes the two-stranger test"
)]
impl Ticketing {
    /// Write a TDD ticket that passes the two-stranger test.
    ///
    /// Composes a ticket with epic context, problem statement, upstream dependencies,
    /// downstream consumers, and domain-specific context. The ticket is written to
    /// disk and verified against meta-checks.
    #[plexus_macros::method(params(
        epic = "The epic this ticket belongs to",
        problem = "The problem statement (what needs solving)",
        upstream_tickets = "List of ticket IDs that must complete first",
        downstream_consumers = "List of systems/actors that depend on this ticket's output",
        domain_context = "Optional domain-specific context (e.g., architectural constraints)",
        confidence = "Optional confidence level (e.g., High, Medium, Low)"
    ))]
    async fn write(
        &self,
        epic: String,
        problem: String,
        upstream_tickets: Vec<String>,
        downstream_consumers: Vec<String>,
        domain_context: Option<String>,
        confidence: Option<String>,
    ) -> impl Stream<Item = WriteEvent> + Send + 'static {
        stream! {
            yield WriteEvent::Started;
            let _ = (epic, problem, upstream_tickets, downstream_consumers, domain_context, confidence);
            // Skeleton: in the real impl we'd
            //   1. Validate epic and inputs
            //   2. Call swarm to draft ticket
            //   3. Run meta-checks (DAG consistency, etc.)
            //   4. Write ticket file
            //   5. Emit Completed with metadata
            //
            // Until SwarmRuntime is wired, emit a representative failure so
            // consumers see the shape and the error handling.
            yield WriteEvent::Error {
                stage: "swarm-not-wired".into(),
                message: "ticketing.write is a skeleton; SwarmRuntime is stubbed (Phase 2 follow-up)".into(),
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;

    #[tokio::test]
    async fn write_emits_started_then_swarm_not_wired_error() {
        let ticketing = Ticketing::new();
        let stream = ticketing.write(
            "EPIC-1".into(),
            "Implement feature X".into(),
            vec!["EPIC-1".into()],
            vec!["client-app".into()],
            None,
            None,
        )
        .await;
        let mut stream = Box::pin(stream);

        // First event: Started
        let evt = stream.next().await.expect("event");
        assert!(matches!(evt, WriteEvent::Started));

        // Second event: Error with swarm-not-wired stage
        let evt = stream.next().await.expect("event");
        match evt {
            WriteEvent::Error { stage, message } => {
                assert_eq!(stage, "swarm-not-wired");
                assert!(message.contains("SwarmRuntime"));
            }
            other => panic!("unexpected event: {:?}", other),
        }

        // No more events
        assert!(stream.next().await.is_none());
    }

    #[test]
    fn write_event_started_has_correct_tag() {
        let evt = WriteEvent::Started;
        let v = serde_json::to_value(&evt).unwrap();
        assert_eq!(v["type"], "started");
    }

    #[test]
    fn write_event_completed_has_all_fields() {
        let evt = WriteEvent::Completed {
            id: "TICK-001".into(),
            frontmatter: serde_json::json!({"epic": "EPIC-1"}),
            body: "Ticket body".into(),
            meta_checks: serde_json::json!({"valid": true}),
            file_path: "/tickets/TICK-001.md".into(),
        };
        let v = serde_json::to_value(&evt).unwrap();
        assert_eq!(v["type"], "completed");
        assert!(v.get("id").is_some());
        assert!(v.get("frontmatter").is_some());
        assert!(v.get("body").is_some());
        assert!(v.get("meta_checks").is_some());
        assert!(v.get("file_path").is_some());
    }

    #[test]
    fn write_event_error_has_stage_and_message() {
        let evt = WriteEvent::Error {
            stage: "validation".into(),
            message: "invalid input".into(),
        };
        let v = serde_json::to_value(&evt).unwrap();
        assert_eq!(v["type"], "error");
        assert_eq!(v["stage"], "validation");
        assert_eq!(v["message"], "invalid input");
    }
}
