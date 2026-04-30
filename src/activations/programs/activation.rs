//! Programs activation — query + inspect program directories.
//!
//! Backed by the `MnemeStorage` SQLite index for `status`/`list` (fast,
//! queryable) and the filesystem (manifest.json/artifact.json/error.json) for
//! `inspect` (the truth).

use std::sync::Arc;

use async_stream::stream;
use futures::Stream;

use super::types::*;
use crate::mneme::context::MnemeContext;
use crate::mneme::program::ProgramId;
use crate::mneme::storage::ProgramRow;

/// The programs activation. Holds an [`MnemeContext`] for storage + filesystem
/// access.
#[derive(Clone)]
pub struct Programs {
    context: Arc<MnemeContext>,
}

impl Programs {
    pub fn new(context: Arc<MnemeContext>) -> Self {
        Self { context }
    }
}

#[plexus_macros::activation(
    namespace = "programs",
    version = "0.1.0",
    description = "Query and inspect program directories produced by mneme skill invocations"
)]
impl Programs {
    /// Get the current status of one program by id.
    ///
    /// Reads from the SQLite index (the `mneme_programs` table). For full
    /// detail (manifest, artifact, error), use `programs.inspect`.
    #[plexus_macros::method(params(program_id = "Program id (UUID)"))]
    async fn status(&self, program_id: String) -> impl Stream<Item = StatusEvent> + Send + 'static {
        let context = self.context.clone();
        stream! {
            let storage = match context.storage() {
                Some(s) => s.clone(),
                None => {
                    yield StatusEvent::Error {
                        message: "no SQLite storage attached to this substrate".into(),
                    };
                    return;
                }
            };
            let pid = match parse_program_id(&program_id) {
                Ok(p) => p,
                Err(e) => { yield StatusEvent::Error { message: e }; return; }
            };
            match storage.get(&pid).await {
                Ok(Some(row)) => yield StatusEvent::Status { summary: row_to_summary(row) },
                Ok(None) => yield StatusEvent::NotFound { program_id },
                Err(e) => yield StatusEvent::Error { message: e.to_string() },
            }
        }
    }

    /// List programs, newest first. Optional status filter.
    #[plexus_macros::method(streaming, params(
        status = "Optional status filter: running | completed | failed",
        limit = "Max programs to return (default 50)"
    ))]
    async fn list(
        &self,
        status: Option<String>,
        limit: Option<u32>,
    ) -> impl Stream<Item = ListEvent> + Send + 'static {
        let context = self.context.clone();
        stream! {
            let storage = match context.storage() {
                Some(s) => s.clone(),
                None => {
                    yield ListEvent::Error {
                        message: "no SQLite storage attached".into(),
                    };
                    return;
                }
            };
            let status_filter = match status.as_deref().map(parse_status) {
                Some(Some(s)) => Some(s),
                Some(None) => {
                    yield ListEvent::Error {
                        message: format!("unknown status `{}`; expected running|completed|failed", status.unwrap_or_default()),
                    };
                    return;
                }
                None => None,
            };
            let limit = limit.unwrap_or(50);
            match storage.list(status_filter, limit).await {
                Ok(rows) => {
                    let count = rows.len() as u32;
                    for row in rows {
                        yield ListEvent::Program { summary: row_to_summary(row) };
                    }
                    yield ListEvent::Completed { count };
                }
                Err(e) => yield ListEvent::Error { message: e.to_string() },
            }
        }
    }

    /// Block until a fire-and-return program reaches a terminal state,
    /// streaming `Progress` on each status change and ending with
    /// `Completed { artifact }` / `Failed { error }` / `TimedOut` /
    /// `NotFound`.
    ///
    /// Designed for shell-piping after `forecast.update` (which itself
    /// is fire-and-return):
    ///
    /// ```bash
    /// PID=$(synapse forecast update --program-id X --new-evidence "..." \
    ///       | jq -r 'select(.content.type == "started") | .content.program_id')
    /// synapse programs wait --program-id "$PID"
    /// ```
    #[plexus_macros::method(streaming, params(
        program_id = "Program id (UUID)",
        poll_interval_ms = "How often to check status; default 2000",
        timeout_secs = "Hard cap before TimedOut; default 900 (15 min)"
    ))]
    async fn wait(
        &self,
        program_id: String,
        poll_interval_ms: Option<u64>,
        timeout_secs: Option<u64>,
    ) -> impl Stream<Item = WaitEvent> + Send + 'static {
        let context = self.context.clone();
        stream! {
            let pid = match parse_program_id(&program_id) {
                Ok(p) => p,
                Err(_) => {
                    yield WaitEvent::NotFound { program_id };
                    return;
                }
            };
            let dir = crate::mneme::program::ProgramDirectory::open(
                context.programs_root(),
                pid,
            );
            let manifest_path = dir.manifest_path();
            let artifact_path = dir.artifact_path();
            let error_path = dir.error_path();

            let poll_ms = poll_interval_ms.unwrap_or(2000).max(100);
            let timeout = std::time::Duration::from_secs(timeout_secs.unwrap_or(900));
            let started_at = std::time::Instant::now();
            let mut last_status: Option<String> = None;

            loop {
                let waited_ms = started_at.elapsed().as_millis() as u64;

                if !manifest_path.exists() {
                    // First poll: program directory not (yet) created.
                    // Return NotFound rather than spinning until timeout.
                    if last_status.is_none() {
                        yield WaitEvent::NotFound { program_id };
                        return;
                    }
                    // Subsequent: shouldn't happen since manifest is written
                    // before the substrate yields Started, but treat as
                    // timeout-equivalent.
                    yield WaitEvent::TimedOut {
                        program_id,
                        last_status: last_status.unwrap_or_else(|| "unknown".into()),
                        waited_ms,
                    };
                    return;
                }

                let manifest_status = std::fs::read(&manifest_path)
                    .ok()
                    .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
                    .and_then(|v| v.get("status").and_then(|s| s.as_str()).map(String::from))
                    .unwrap_or_else(|| "unknown".to_string());

                // Emit Progress on initial poll OR on status change.
                let changed = last_status.as_deref() != Some(manifest_status.as_str());
                if changed {
                    yield WaitEvent::Progress {
                        program_id: program_id.clone(),
                        status: manifest_status.clone(),
                        age_ms: waited_ms,
                    };
                    last_status = Some(manifest_status.clone());
                }

                match manifest_status.as_str() {
                    "completed" => {
                        let artifact = std::fs::read(&artifact_path)
                            .ok()
                            .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
                            .unwrap_or_else(|| serde_json::json!({}));
                        yield WaitEvent::Completed {
                            program_id,
                            artifact,
                            waited_ms,
                        };
                        return;
                    }
                    "failed" => {
                        let error = std::fs::read(&error_path)
                            .ok()
                            .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
                            .unwrap_or_else(|| serde_json::json!({"message": "no error.json"}));
                        yield WaitEvent::Failed {
                            program_id,
                            error,
                            waited_ms,
                        };
                        return;
                    }
                    _ => {}
                }

                if started_at.elapsed() >= timeout {
                    yield WaitEvent::TimedOut {
                        program_id,
                        last_status: last_status.unwrap_or_else(|| "unknown".into()),
                        waited_ms,
                    };
                    return;
                }

                tokio::time::sleep(std::time::Duration::from_millis(poll_ms)).await;
            }
        }
    }

    /// Full inspection of one program: manifest + artifact (or error) +
    /// trace line count.
    #[plexus_macros::method(params(program_id = "Program id (UUID)"))]
    async fn inspect(
        &self,
        program_id: String,
    ) -> impl Stream<Item = InspectEvent> + Send + 'static {
        let context = self.context.clone();
        stream! {
            let pid = match parse_program_id(&program_id) {
                Ok(p) => p,
                Err(e) => { yield InspectEvent::Error { message: e }; return; }
            };
            let dir = crate::mneme::program::ProgramDirectory::open(
                context.programs_root(),
                pid,
            );
            let manifest_path = dir.manifest_path();
            if !manifest_path.exists() {
                yield InspectEvent::NotFound { program_id };
                return;
            }
            let manifest_bytes = match std::fs::read(&manifest_path) {
                Ok(b) => b,
                Err(e) => { yield InspectEvent::Error { message: format!("read manifest: {}", e) }; return; }
            };
            let manifest: serde_json::Value = match serde_json::from_slice(&manifest_bytes) {
                Ok(v) => v,
                Err(e) => { yield InspectEvent::Error { message: format!("parse manifest: {}", e) }; return; }
            };
            let artifact = std::fs::read(dir.artifact_path())
                .ok()
                .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok());
            let error = std::fs::read(dir.error_path())
                .ok()
                .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok());
            let trace_count = std::fs::read_to_string(dir.trace_path())
                .ok()
                .map(|s| s.lines().filter(|l| !l.trim().is_empty()).count() as u32)
                .unwrap_or(0);
            yield InspectEvent::Detail {
                detail: ProgramDetail {
                    program_id,
                    manifest,
                    artifact,
                    error,
                    trace_count,
                },
            };
        }
    }
}

fn row_to_summary(row: ProgramRow) -> ProgramSummary {
    ProgramSummary {
        program_id: row.program_id,
        parent_program_id: row.parent_program_id,
        entry_skill: row.entry_skill,
        status: status_string(row.status),
        started_at: row.started_at,
        finished_at: row.finished_at,
        depth: row.depth,
    }
}

fn parse_program_id(s: &str) -> Result<ProgramId, String> {
    let uuid = uuid::Uuid::parse_str(s)
        .map_err(|e| format!("invalid program_id `{}`: {}", s, e))?;
    Ok(ProgramId::from_uuid(uuid))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mneme::storage::MnemeStorage;
    use futures::StreamExt;
    use serde_json::json;
    use tempfile::TempDir;

    async fn programs_with_data() -> (TempDir, Programs, ProgramId, ProgramId) {
        let dir = TempDir::new().unwrap();
        let storage = Arc::new(MnemeStorage::open_in_memory().await.unwrap());
        let context = Arc::new(
            MnemeContext::with_stub_swarm(dir.path()).with_storage(storage.clone()),
        );

        // Create two programs via context (which writes manifest + indexes).
        let p1 = context.open_program("forecast.update", json!({"q": "Q1"})).await.unwrap();
        let p1_id = p1.id().clone();
        p1.close_completed(&json!({"probability": 0.6}), "0.1.0").await.unwrap();

        let p2 = context.open_program("ticketing.write", json!({})).await.unwrap();
        let p2_id = p2.id().clone();
        // p2 left running.

        (dir, Programs::new(context), p1_id, p2_id)
    }

    #[tokio::test]
    async fn status_returns_existing_program() {
        let (_dir, programs, p1_id, _) = programs_with_data().await;
        let stream = programs.status(p1_id.to_string()).await;
        let mut s = Box::pin(stream);
        let evt = s.next().await.expect("event");
        match evt {
            StatusEvent::Status { summary } => {
                assert_eq!(summary.program_id, p1_id.to_string());
                assert_eq!(summary.entry_skill, "forecast.update");
                assert_eq!(summary.status, "completed");
            }
            other => panic!("unexpected: {:?}", other),
        }
    }

    #[tokio::test]
    async fn status_unknown_returns_not_found() {
        let (_dir, programs, _, _) = programs_with_data().await;
        let unknown = uuid::Uuid::new_v4().to_string();
        let stream = programs.status(unknown.clone()).await;
        let mut s = Box::pin(stream);
        let evt = s.next().await.expect("event");
        assert!(matches!(evt, StatusEvent::NotFound { .. }));
    }

    #[tokio::test]
    async fn status_invalid_uuid_returns_error() {
        let (_dir, programs, _, _) = programs_with_data().await;
        let stream = programs.status("not a uuid".into()).await;
        let mut s = Box::pin(stream);
        let evt = s.next().await.expect("event");
        assert!(matches!(evt, StatusEvent::Error { .. }));
    }

    #[tokio::test]
    async fn list_streams_all_programs() {
        let (_dir, programs, _, _) = programs_with_data().await;
        let stream = programs.list(None, None).await;
        let mut s = Box::pin(stream);

        let mut summaries = vec![];
        let mut count = None;
        while let Some(evt) = s.next().await {
            match evt {
                ListEvent::Program { summary } => summaries.push(summary),
                ListEvent::Completed { count: c } => count = Some(c),
                ListEvent::Error { message } => panic!("error: {}", message),
            }
        }
        assert_eq!(summaries.len(), 2);
        assert_eq!(count, Some(2));
    }

    #[tokio::test]
    async fn list_filters_by_status() {
        let (_dir, programs, _, _) = programs_with_data().await;
        let stream = programs.list(Some("completed".into()), None).await;
        let mut s = Box::pin(stream);

        let mut completed_count = 0;
        while let Some(evt) = s.next().await {
            if let ListEvent::Program { summary } = evt {
                assert_eq!(summary.status, "completed");
                completed_count += 1;
            }
        }
        assert_eq!(completed_count, 1);
    }

    #[tokio::test]
    async fn list_unknown_status_returns_error() {
        let (_dir, programs, _, _) = programs_with_data().await;
        let stream = programs.list(Some("bogus".into()), None).await;
        let mut s = Box::pin(stream);
        let evt = s.next().await.expect("event");
        assert!(matches!(evt, ListEvent::Error { .. }));
    }

    #[tokio::test]
    async fn inspect_returns_full_detail() {
        let (_dir, programs, p1_id, _) = programs_with_data().await;
        let stream = programs.inspect(p1_id.to_string()).await;
        let mut s = Box::pin(stream);
        let evt = s.next().await.expect("event");
        match evt {
            InspectEvent::Detail { detail } => {
                assert_eq!(detail.program_id, p1_id.to_string());
                assert!(detail.manifest.get("status").is_some());
                assert!(detail.artifact.is_some());
                assert!(detail.error.is_none());
            }
            other => panic!("unexpected: {:?}", other),
        }
    }

    #[tokio::test]
    async fn inspect_missing_returns_not_found() {
        let (_dir, programs, _, _) = programs_with_data().await;
        let unknown = uuid::Uuid::new_v4().to_string();
        let stream = programs.inspect(unknown).await;
        let mut s = Box::pin(stream);
        let evt = s.next().await.expect("event");
        assert!(matches!(evt, InspectEvent::NotFound { .. }));
    }

    // ─── programs.wait ───────────────────────────────────────────────────

    #[tokio::test]
    async fn wait_completed_program_returns_artifact_immediately() {
        let (_dir, programs, p1_id, _) = programs_with_data().await;
        // p1 is already completed in programs_with_data().
        let stream = programs.wait(p1_id.to_string(), Some(50), Some(5)).await;
        let mut s = Box::pin(stream);
        let mut events = vec![];
        while let Some(evt) = s.next().await {
            events.push(evt);
        }
        // First event is a Progress with status="completed"; second is Completed.
        assert!(matches!(events.first(), Some(WaitEvent::Progress { status, .. }) if status == "completed"));
        assert!(matches!(
            events.last(),
            Some(WaitEvent::Completed { artifact, .. }) if artifact.get("probability").is_some()
        ));
    }

    #[tokio::test]
    async fn wait_unknown_program_returns_not_found() {
        let (_dir, programs, _, _) = programs_with_data().await;
        let unknown = uuid::Uuid::new_v4().to_string();
        let stream = programs.wait(unknown, Some(50), Some(2)).await;
        let mut s = Box::pin(stream);
        let evt = s.next().await.expect("event");
        assert!(matches!(evt, WaitEvent::NotFound { .. }));
    }

    #[tokio::test]
    async fn wait_running_program_emits_progress_then_terminal() {
        // Open a program but don't close it. Spawn a task that closes it
        // after one poll cycle. Wait should emit Progress(running) then
        // Progress(completed) then Completed.
        let dir = TempDir::new().unwrap();
        let storage = Arc::new(MnemeStorage::open_in_memory().await.unwrap());
        let context = Arc::new(
            MnemeContext::with_stub_swarm(dir.path()).with_storage(storage.clone()),
        );
        let prog = context.open_program("test.skill", json!({})).await.unwrap();
        let pid = prog.id().clone();

        // Spawn a task that closes after a short delay.
        let prog_for_close = prog;
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
            let _ = prog_for_close.close_completed(&json!({"x": 1}), "0.1.0").await;
        });

        let programs = Programs::new(context);
        let stream = programs.wait(pid.to_string(), Some(50), Some(5)).await;
        let mut s = Box::pin(stream);

        let mut saw_running = false;
        let mut saw_completed_terminal = false;
        while let Some(evt) = s.next().await {
            match evt {
                WaitEvent::Progress { status, .. } if status == "running" => saw_running = true,
                WaitEvent::Completed { .. } => {
                    saw_completed_terminal = true;
                    break;
                }
                _ => {}
            }
        }
        assert!(saw_running, "expected at least one Progress(running)");
        assert!(saw_completed_terminal, "expected terminal Completed");
    }

    #[tokio::test]
    async fn wait_failed_program_returns_failed() {
        let dir = TempDir::new().unwrap();
        let storage = Arc::new(MnemeStorage::open_in_memory().await.unwrap());
        let context = Arc::new(
            MnemeContext::with_stub_swarm(dir.path()).with_storage(storage.clone()),
        );
        let prog = context.open_program("test.fail", json!({})).await.unwrap();
        let pid = prog.id().clone();
        prog.close_failed("TestError", "ran out of nope", "test").await.unwrap();

        let programs = Programs::new(context);
        let stream = programs.wait(pid.to_string(), Some(50), Some(5)).await;
        let mut s = Box::pin(stream);
        let mut saw_failed_terminal = false;
        while let Some(evt) = s.next().await {
            if let WaitEvent::Failed { error, .. } = evt {
                let msg = error.get("message").and_then(|m| m.as_str()).unwrap_or("");
                assert!(msg.contains("ran out of nope"));
                saw_failed_terminal = true;
                break;
            }
        }
        assert!(saw_failed_terminal);
    }

    #[tokio::test]
    async fn wait_running_program_times_out() {
        let dir = TempDir::new().unwrap();
        let storage = Arc::new(MnemeStorage::open_in_memory().await.unwrap());
        let context = Arc::new(
            MnemeContext::with_stub_swarm(dir.path()).with_storage(storage.clone()),
        );
        let prog = context.open_program("test.slow", json!({})).await.unwrap();
        let pid = prog.id().clone();
        // Don't close. Wait with a tiny timeout.

        let programs = Programs::new(context);
        let stream = programs.wait(pid.to_string(), Some(100), Some(1)).await; // 1 sec
        let mut s = Box::pin(stream);
        let mut saw_timeout = false;
        while let Some(evt) = s.next().await {
            if let WaitEvent::TimedOut { last_status, .. } = evt {
                assert_eq!(last_status, "running");
                saw_timeout = true;
                break;
            }
        }
        assert!(saw_timeout);
        let _ = prog;
    }
}
