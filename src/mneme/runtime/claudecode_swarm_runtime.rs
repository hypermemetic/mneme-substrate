//! Real `SwarmRuntime` implementation that drives `claudecode` in-process.
//!
//! For each trial: fork the parent session, send the prompt via blocking
//! `chat()`, drain the event stream, take the final assistant text, attempt
//! to parse it as the requested response shape. Without the `respond` tool
//! wired (pending MNEME-S01), structured-output enforcement is best-effort
//! — payloads that don't parse become `TrialFailure`s rather than throwing.
//!
//! The runtime holds an `Arc<ClaudeCode<P>>` that's threaded in by
//! `builder.rs` at substrate construction. The same `ClaudeCode` instance
//! is what the Plexus dispatch hits from external callers; in-process
//! composition uses it directly.

use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::StreamExt;
use plexus_core::plexus::HubContext;
use serde_json::Value;

use crate::activations::claudecode::{ChatEvent, ChatUsage, ClaudeCode, CreateResult, ForkResult, GetResult, Model};
use crate::activations::forecast::iterative_trial;
use crate::mneme::program::{Program, TraceEntry, TraceOp, TraceOutcome};
use crate::mneme::runtime::claudecode_step_driver::ClaudecodeStepDriver;
use crate::mneme::runtime::swarm_runtime::{ParentSessionSpec, SwarmError, SwarmRuntime, TrialParams};
use crate::mneme::swarm::{TrialBatch, TrialFailure, TrialResult, TrialUsage};

/// Production `SwarmRuntime` impl. Wraps a shared `ClaudeCode` activation
/// instance and uses its public `fork` + `chat` methods for each trial.
pub struct ClaudeCodeSwarmRuntime<P: HubContext + 'static> {
    claudecode: Arc<ClaudeCode<P>>,
}

impl<P: HubContext + 'static> ClaudeCodeSwarmRuntime<P> {
    pub fn new(claudecode: Arc<ClaudeCode<P>>) -> Self {
        Self { claudecode }
    }
}

impl<P: HubContext + 'static> std::fmt::Debug for ClaudeCodeSwarmRuntime<P> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClaudeCodeSwarmRuntime").finish_non_exhaustive()
    }
}

#[async_trait::async_trait]
impl<P: HubContext + 'static> SwarmRuntime for ClaudeCodeSwarmRuntime<P> {
    /// Idempotent. Resolves the spec to a content-hashed session name (so
    /// system_prompt changes produce a fresh session per MNEME-21) and creates
    /// the session if it doesn't exist.
    ///
    /// The caller's logical `spec.name` is NOT the underlying claudecode
    /// session name; the actual name includes a content hash. Callers who
    /// need to fork or chat with the session must call `spec.resolved_name()`
    /// (or use the helper threaded through ParentSessionSpec).
    async fn ensure_parent_session(&self, spec: ParentSessionSpec) -> Result<(), SwarmError> {
        let resolved = spec.resolved_name();
        // Fast path: resolved-name session already exists with current SKILL.md.
        let get_stream = self.claudecode.get(resolved.clone()).await;
        let mut get_stream = Box::pin(get_stream);
        if let Some(GetResult::Ok { .. }) = get_stream.next().await {
            return Ok(());
        }
        // Slow path: create with the resolved name.
        let model = match spec.model.as_str() {
            "opus" => Model::Opus,
            "sonnet" => Model::Sonnet,
            "haiku" => Model::Haiku,
            other => {
                return Err(SwarmError::NotImplemented(Box::leak(
                    format!("unknown model `{}`; expected opus/sonnet/haiku", other)
                        .into_boxed_str(),
                )))
            }
        };
        let create_stream = self
            .claudecode
            .create(
                resolved.clone(),
                spec.working_dir.clone(),
                model,
                Some(spec.system_prompt),
                None,
                None,
            )
            .await;
        let mut create_stream = Box::pin(create_stream);
        match create_stream.next().await {
            Some(CreateResult::Ok { .. }) => Ok(()),
            Some(CreateResult::Err { message }) => {
                Err(SwarmError::NotImplemented(Box::leak(
                    format!("create session `{}`: {}", resolved, message).into_boxed_str(),
                )))
            }
            None => Err(SwarmError::NotImplemented(
                "create returned no result",
            )),
        }
    }

    async fn trial(
        &self,
        program: &Program,
        params: TrialParams,
    ) -> Result<TrialBatch, SwarmError> {
        params.validate()?;

        let start = Instant::now();
        let mut successes = Vec::with_capacity(params.n as usize);
        let mut failures = Vec::with_capacity(params.n as usize);
        let mut child_session_ids = Vec::with_capacity(params.n as usize);

        for trial_index in 0..params.n {
            let trial_session_name =
                format!("{}-trial-{}", program.id(), trial_index);

            let prompt = compose_prompt(&params.prompt, params.diversify.as_deref(), trial_index);
            let trial_start = Instant::now();

            let trial_outcome = match params.iterative_max_steps {
                Some(t_max) => {
                    run_iterative_trial(
                        self.claudecode.clone(),
                        params.parent_session.clone(),
                        trial_session_name.clone(),
                        prompt,
                        t_max,
                        params.timeout,
                        params.allowed_tools.clone(),
                    )
                    .await
                }
                None => {
                    run_one_trial(
                        self.claudecode.clone(),
                        params.parent_session.clone(),
                        trial_session_name.clone(),
                        prompt,
                        params.timeout,
                        params.allowed_tools.clone(),
                    )
                    .await
                }
            };

            match trial_outcome {
                Ok((text, usage)) => {
                    child_session_ids.push(trial_session_name.clone());
                    let response = parse_response(&text);
                    successes.push(TrialResult {
                        trial_index,
                        session_id: trial_session_name,
                        response,
                        duration_ms: trial_start.elapsed().as_millis() as u64,
                        usage,
                    });
                }
                Err(error) => {
                    tracing::error!(
                        trial_index,
                        session = %trial_session_name,
                        "trial failed: {}",
                        error
                    );
                    failures.push(TrialFailure {
                        trial_index,
                        session_id: Some(trial_session_name),
                        error,
                        last_payload: None,
                    });
                }
            }
        }

        // Sum per-trial usage so the trace entry surfaces total tokens / cost
        // for this swarm fan-out. Trials with `usage == None` (mock runtimes,
        // claudecode runs with no terminal Complete event) contribute 0.
        let usage_total = successes
            .iter()
            .filter_map(|s| s.usage.clone())
            .fold(TrialUsage::default(), |acc, u| acc.merge(&u));
        let trials_with_usage = successes.iter().filter(|s| s.usage.is_some()).count();

        // Record the trace entry on the program.
        let entry = TraceEntry::new(
            program.next_seq(),
            TraceOp::SwarmTrial,
            serde_json::json!({
                "n": params.n,
                "parent_session": params.parent_session,
                "diversify": params.diversify,
                "iterative_max_steps": params.iterative_max_steps,
                "usage_total": usage_total,
                "trials_with_usage": trials_with_usage,
                "failure_messages": failures.iter().map(|f| &f.error).collect::<Vec<_>>(),
            }),
            child_session_ids,
            if failures.is_empty() { TraceOutcome::Ok } else { TraceOutcome::Err },
            start.elapsed(),
        );
        program.record_trace(&entry).map_err(|e| {
            SwarmError::NotImplemented(Box::leak(format!("record_trace: {}", e).into_boxed_str()))
        })?;

        Ok(TrialBatch { successes, failures })
    }
}

fn compose_prompt(base: &str, diversify: Option<&str>, trial_index: u8) -> String {
    match diversify {
        Some(template) => {
            let header = template.replace("%i", &trial_index.to_string());
            format!("{}\n\n{}", header, base)
        }
        None => base.to_string(),
    }
}

/// Drive one trial: fork the parent, run blocking `chat`, collect the
/// assistant text, return it. Errors as `String` for ergonomic forwarding
/// into [`TrialFailure`].
async fn run_one_trial<P: HubContext + 'static>(
    claudecode: Arc<ClaudeCode<P>>,
    parent: String,
    new_name: String,
    prompt: String,
    timeout: Duration,
    allowed_tools: Option<Vec<String>>,
) -> Result<(String, Option<TrialUsage>), String> {
    // Fork.
    let fork_stream = claudecode.fork(parent.clone(), new_name.clone()).await;
    let mut fork_stream = Box::pin(fork_stream);
    match fork_stream.next().await {
        Some(ForkResult::Ok { .. }) => {}
        Some(ForkResult::Err { message }) => {
            return Err(format!("fork failed: {}", message));
        }
        None => return Err("fork returned no result".into()),
    }

    // Chat with timeout. Default tools (WebSearch + Read) when caller didn't
    // specify — so trials can actually research rather than reasoning from
    // training alone. Caller can pass an explicit list (or empty Vec for none).
    let allowed_tools = allowed_tools.or_else(|| {
        Some(vec!["WebSearch".to_string(), "Read".to_string()])
    });
    let chat_future = async move {
        let stream = claudecode.chat(new_name, prompt, None, allowed_tools).await;
        let mut stream = Box::pin(stream);
        let mut buffer = String::new();
        while let Some(event) = stream.next().await {
            match event {
                ChatEvent::Content { text } => buffer.push_str(&text),
                ChatEvent::Complete { usage, .. } => {
                    return Ok((buffer, usage.map(chat_usage_to_trial_usage)));
                }
                ChatEvent::Err { message } => return Err(format!("chat error: {}", message)),
                _ => {}
            }
        }
        // Stream ended without Complete.
        if buffer.is_empty() {
            Err("chat ended without content".into())
        } else {
            Ok((buffer, None))
        }
    };

    match tokio::time::timeout(timeout, chat_future).await {
        Ok(result) => result,
        Err(_) => Err(format!("trial timed out after {:?}", timeout)),
    }
}

fn chat_usage_to_trial_usage(u: ChatUsage) -> TrialUsage {
    TrialUsage {
        input_tokens: u.input_tokens,
        output_tokens: u.output_tokens,
        cost_usd: u.cost_usd,
        num_turns: u.num_turns,
    }
}

/// Drive one trial as an iterative BLF loop (Murphy 2026 Algorithm 1).
/// Forks the parent session, then runs `iterative_trial<ClaudecodeStepDriver>`
/// which sends one chat per step and parses (action, belief) from each
/// response. Returns the final belief serialized as JSON for downstream
/// aggregation, plus the accumulated per-step token usage.
async fn run_iterative_trial<P: HubContext + 'static>(
    claudecode: Arc<ClaudeCode<P>>,
    parent: String,
    new_name: String,
    initial_question: String,
    max_steps: u8,
    timeout: Duration,
    allowed_tools: Option<Vec<String>>,
) -> Result<(String, Option<TrialUsage>), String> {
    // Fork the parent session for this trial.
    let fork_stream = claudecode.fork(parent.clone(), new_name.clone()).await;
    let mut fork_stream = Box::pin(fork_stream);
    match fork_stream.next().await {
        Some(ForkResult::Ok { .. }) => {}
        Some(ForkResult::Err { message }) => return Err(format!("fork failed: {}", message)),
        None => return Err("fork returned no result".into()),
    }

    // Default tools (WebSearch + Read) when caller didn't specify — same
    // policy as single-shot run_one_trial.
    let allowed_tools = allowed_tools.or_else(|| {
        Some(vec!["WebSearch".to_string(), "Read".to_string()])
    });

    let mut driver =
        ClaudecodeStepDriver::new(claudecode, new_name.clone(), allowed_tools);

    let loop_future = async {
        iterative_trial(&mut driver, &initial_question, max_steps).await
    };

    match tokio::time::timeout(timeout, loop_future).await {
        Ok(Ok((belief, _history))) => {
            let belief_json = serde_json::to_string(&belief)
                .map_err(|e| format!("serialize belief: {}", e))?;
            let usage = driver.into_usage();
            let usage_opt = if usage == TrialUsage::default() {
                None
            } else {
                Some(usage)
            };
            Ok((belief_json, usage_opt))
        }
        Ok(Err(loop_err)) => Err(format!("iterative trial: {}", loop_err)),
        Err(_) => Err(format!("iterative trial timed out after {:?}", timeout)),
    }
}

/// Parse the assistant's final text as the trial's typed response.
///
/// Strategy (best-effort, since `respond` tool isn't wired):
/// 1. Try to extract a fenced ```json``` block and parse it.
/// 2. Else try to parse the whole text as JSON.
/// 3. Else wrap the text as `{"text": "..."}` so consumers always get an object.
fn parse_response(text: &str) -> Value {
    // (1) Look for fenced JSON block.
    if let Some(json) = extract_json_block(text) {
        if let Ok(value) = serde_json::from_str::<Value>(&json) {
            return value;
        }
    }
    // (2) Whole text as JSON.
    if let Ok(value) = serde_json::from_str::<Value>(text.trim()) {
        return value;
    }
    // (3) Wrap as object.
    serde_json::json!({ "text": text })
}

fn extract_json_block(text: &str) -> Option<String> {
    let start_marker = "```json";
    let start = text.find(start_marker)?;
    let after_marker = &text[start + start_marker.len()..];
    let end = after_marker.find("```")?;
    Some(after_marker[..end].trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn compose_prompt_no_diversify() {
        let p = compose_prompt("the question", None, 0);
        assert_eq!(p, "the question");
    }

    #[test]
    fn compose_prompt_with_diversify_substitutes_index() {
        let p = compose_prompt("the question", Some("Reasoning style #%i (analytic)"), 2);
        assert_eq!(p, "Reasoning style #2 (analytic)\n\nthe question");
    }

    #[test]
    fn parse_response_fenced_json_block() {
        let text = "Reasoning here.\n\n```json\n{\"probability\": 0.42, \"summary\": \"...\"}\n```\n\nDone.";
        let v = parse_response(text);
        assert_eq!(v["probability"], json!(0.42));
        assert_eq!(v["summary"], "...");
    }

    #[test]
    fn parse_response_raw_json() {
        let text = "{\"a\": 1}";
        let v = parse_response(text);
        assert_eq!(v["a"], json!(1));
    }

    #[test]
    fn parse_response_plain_text_wrapped() {
        let text = "I do not have an answer.";
        let v = parse_response(text);
        assert_eq!(v["text"], "I do not have an answer.");
    }

    #[test]
    fn extract_json_block_finds_fenced() {
        let text = "before\n```json\n{\"x\":1}\n```\nafter";
        assert_eq!(extract_json_block(text), Some("{\"x\":1}".to_string()));
    }

    #[test]
    fn extract_json_block_returns_none_when_absent() {
        assert!(extract_json_block("no json here").is_none());
    }

    #[test]
    fn extract_json_block_handles_unterminated() {
        // Unterminated fence (no closing ```) returns None rather than
        // gobbling the whole tail.
        let text = "```json\n{\"x\":1}\nno close fence";
        assert!(extract_json_block(text).is_none());
    }
}
