//! Trace entries — one per layer-1 (`swarm.*`) call within a program.
//!
//! Persisted as JSONL at `programs/<program_id>/trace.jsonl`. One line per
//! orchestration call. Trial-level events live separately in
//! `programs/<id>/sessions/<session_id>.json` (the claudecode session export).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

/// The orchestration operation that produced this trace entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceOp {
    SwarmTrial,
    SwarmAggregate,
    SwarmSequential,
    SwarmRace,
    /// Reserved for child-program loopback calls.
    LoopbackChild,
}

/// Whether the operation succeeded or errored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceOutcome {
    Ok,
    Err,
}

/// One line in `trace.jsonl`. Append-only, monotonically `seq`-numbered.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceEntry {
    /// Monotonic sequence number within the program; starts at 1.
    pub seq: u32,
    /// When this entry was recorded.
    pub at: DateTime<Utc>,
    /// Which orchestration operation.
    pub op: TraceOp,
    /// Skill-defined small summary of the args (kept inline; full args may be elsewhere).
    pub args_summary: Value,
    /// claudecode session ids spawned by this op.
    pub child_session_ids: Vec<String>,
    /// Child program ids spawned via loopback (typically empty for swarm ops).
    pub child_program_ids: Vec<String>,
    /// Whether the op succeeded.
    pub outcome: TraceOutcome,
    /// Wall-clock duration in milliseconds.
    pub duration_ms: u64,
}

impl TraceEntry {
    pub fn new(
        seq: u32,
        op: TraceOp,
        args_summary: Value,
        child_session_ids: Vec<String>,
        outcome: TraceOutcome,
        duration: Duration,
    ) -> Self {
        Self {
            seq,
            at: Utc::now(),
            op,
            args_summary,
            child_session_ids,
            child_program_ids: Vec::new(),
            outcome,
            duration_ms: duration.as_millis() as u64,
        }
    }

    pub fn with_child_programs(mut self, ids: Vec<String>) -> Self {
        self.child_program_ids = ids;
        self
    }

    /// Render as a single JSONL line (no trailing newline; caller appends).
    pub fn to_jsonl(&self) -> serde_json::Result<String> {
        serde_json::to_string(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture() -> TraceEntry {
        TraceEntry::new(
            1,
            TraceOp::SwarmTrial,
            json!({"n": 3, "parent_session": "p1"}),
            vec!["s1".to_string(), "s2".to_string(), "s3".to_string()],
            TraceOutcome::Ok,
            Duration::from_millis(2500),
        )
    }

    #[test]
    fn jsonl_is_one_line() {
        let entry = fixture();
        let line = entry.to_jsonl().unwrap();
        assert!(!line.contains('\n'));
    }

    #[test]
    fn round_trips_through_serde() {
        let entry = fixture();
        let line = entry.to_jsonl().unwrap();
        let back: TraceEntry = serde_json::from_str(&line).unwrap();
        assert_eq!(back.seq, 1);
        assert_eq!(back.op, TraceOp::SwarmTrial);
        assert_eq!(back.outcome, TraceOutcome::Ok);
        assert_eq!(back.duration_ms, 2500);
        assert_eq!(back.child_session_ids.len(), 3);
    }

    #[test]
    fn with_child_programs_attaches_ids() {
        let entry = fixture().with_child_programs(vec!["child-1".to_string()]);
        assert_eq!(entry.child_program_ids, vec!["child-1".to_string()]);
    }

    #[test]
    fn op_serializes_snake_case() {
        let line = fixture().to_jsonl().unwrap();
        assert!(line.contains("\"op\":\"swarm_trial\""));
    }
}
