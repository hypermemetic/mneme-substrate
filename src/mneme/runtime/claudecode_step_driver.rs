//! `ClaudecodeStepDriver` — production [`StepDriver`] backed by a long-running
//! claudecode session. One driver instance per trial; constructed after the
//! session has been forked from the skill parent.
//!
//! The driver owns the session id and an accumulator for per-trial token
//! usage. Each `next_step` call sends a single `chat` to the session,
//! drains the event stream until `Complete`, captures `usage`, and returns
//! the assistant text. Because claudecode retains conversation history
//! server-side, the driver only needs to render the *new* content per step
//! (the question on step 0, the latest observation on subsequent steps).

use std::sync::Arc;

use async_trait::async_trait;
use futures::StreamExt;
use plexus_core::plexus::HubContext;

use crate::activations::claudecode::{ChatEvent, ChatUsage, ClaudeCode};
use crate::activations::forecast::{Observation, StepContext, StepDriver};
use crate::mneme::swarm::TrialUsage;

/// Per-step driver bound to one claudecode session.
pub(crate) struct ClaudecodeStepDriver<P: HubContext + 'static> {
    pub(crate) claudecode: Arc<ClaudeCode<P>>,
    pub(crate) session_name: String,
    pub(crate) allowed_tools: Option<Vec<String>>,
    pub(crate) usage_accum: TrialUsage,
}

impl<P: HubContext + 'static> ClaudecodeStepDriver<P> {
    pub(crate) fn new(
        claudecode: Arc<ClaudeCode<P>>,
        session_name: String,
        allowed_tools: Option<Vec<String>>,
    ) -> Self {
        Self {
            claudecode,
            session_name,
            allowed_tools,
            usage_accum: TrialUsage::default(),
        }
    }

    /// Move out the accumulated usage at the end of a trial.
    pub(crate) fn into_usage(self) -> TrialUsage {
        self.usage_accum
    }
}

#[async_trait]
impl<P: HubContext + 'static> StepDriver for ClaudecodeStepDriver<P> {
    async fn next_step(&mut self, ctx: StepContext<'_>) -> Result<String, String> {
        let prompt = build_session_prompt(&ctx);

        let stream = self
            .claudecode
            .chat(
                self.session_name.clone(),
                prompt,
                None,
                self.allowed_tools.clone(),
            )
            .await;
        let mut stream = Box::pin(stream);
        let mut buffer = String::new();
        while let Some(event) = stream.next().await {
            match event {
                ChatEvent::Content { text } => buffer.push_str(&text),
                ChatEvent::Complete { usage, .. } => {
                    if let Some(u) = usage {
                        self.usage_accum = self.usage_accum.merge(&chat_usage_to_trial(u));
                    }
                    return Ok(buffer);
                }
                ChatEvent::Err { message } => {
                    return Err(format!("chat error: {}", message));
                }
                _ => {}
            }
        }
        if buffer.is_empty() {
            Err("chat ended without Complete event".into())
        } else {
            Ok(buffer)
        }
    }
}

/// Compose the per-step prompt the driver sends. Because claudecode retains
/// session history server-side, we only emit the *new* content each step:
/// the question on step 0, the latest observation thereafter. The
/// per-call framing reminds the model of the JSON output contract so a
/// single-step regression doesn't propagate.
fn build_session_prompt(ctx: &StepContext<'_>) -> String {
    let mut out = String::new();
    if ctx.step_idx == 0 {
        out.push_str(&format!(
            "You are running an iterative forecast loop. Step {}/{}.\n\n",
            ctx.step_idx + 1,
            ctx.max_steps
        ));
        out.push_str(&format!("Question: {}\n\n", ctx.initial_question));
        out.push_str(ITERATIVE_FORMAT_CONTRACT);
        out.push_str(&format!(
            "\nThis is step 1 of up to {}. You have no observations yet, so \
             pick `web_search` (or `submit` only if you genuinely already know \
             the answer with high confidence).\n",
            ctx.max_steps
        ));
    } else {
        // Step > 0: the previous turn just executed an action. Hand back the
        // observation and ask for the next step.
        let last = ctx
            .history
            .last()
            .map(|h| &h.observation);
        out.push_str(&format!(
            "Step {}/{}. Last action's result:\n\n",
            ctx.step_idx + 1,
            ctx.max_steps
        ));
        match last {
            Some(observation) => {
                out.push_str(&render_observation(observation));
            }
            None => {
                out.push_str("(no observation available)\n");
            }
        }
        out.push_str("\n");
        out.push_str(ITERATIVE_FORMAT_CONTRACT);
        if ctx.step_idx + 1 == ctx.max_steps {
            out.push_str(
                "\nThis is your LAST step — your action MUST be `submit` with a final \
                 probability.\n",
            );
        }
    }
    out
}

/// Hard-baked JSON-shape contract that goes on every step. Repeating the full
/// shape each turn is cheap insurance against the model drifting out of the
/// `{"type": "..."}` internally-tagged Action format — the Rust parser
/// rejects bare-string actions like `"action": "submit"`.
const ITERATIVE_FORMAT_CONTRACT: &str = r#"
Reply with reasoning followed by a single fenced ```json block whose contents
match this shape EXACTLY (the `action.type` field is required — bare-string
actions are rejected):

```json
{
  "action": {
    "type": "web_search",
    "query": "concrete query string",
    "k": 5
  },
  "belief": {
    "probability": 0.5,
    "summary": "",
    "evidence_for": [],
    "evidence_against": [],
    "open_questions": []
  }
}
```

Valid `action` shapes:
- `{"type": "web_search", "query": "...", "k": 1..20}` — issue a search.
- `{"type": "lookup_url", "url": "https://..."}` — fetch a URL.
- `{"type": "summarize_results", "result_ids": ["...","..."]}` — only after a prior `web_search`.
- `{"type": "submit", "probability": 0.0..1.0}` — terminate with final p.

`belief.probability` is required and must be a number in [0, 1]. The other
belief fields default to empty arrays / strings if you have nothing to add.
"#;

fn render_observation(obs: &Observation) -> String {
    match obs {
        Observation::SearchResults { results } => {
            let mut s = format!("Search returned {} results:\n", results.len());
            for hit in results {
                s.push_str(&format!(
                    "- [{}] {} — {}\n  {}\n",
                    hit.id, hit.title, hit.url, hit.snippet
                ));
            }
            s
        }
        Observation::Summary { text } => format!("Summary:\n{}\n", text),
        Observation::PageContent { url, content, .. } => {
            format!("Page {}:\n{}\n", url, content)
        }
        Observation::TimeSeries { source, key, points } => format!(
            "Time series {}/{} ({} points): {:?}\n",
            source,
            key,
            points.len(),
            points
        ),
        Observation::WikipediaSection {
            article,
            section,
            content,
        } => format!(
            "Wikipedia {}#{}:\n{}\n",
            article, section, content
        ),
        Observation::Submitted { probability } => {
            format!("Submitted probability: {}\n", probability)
        }
        Observation::Error { message } => format!("Action failed: {}\n", message),
    }
}

fn chat_usage_to_trial(u: ChatUsage) -> TrialUsage {
    TrialUsage {
        input_tokens: u.input_tokens,
        output_tokens: u.output_tokens,
        cost_usd: u.cost_usd,
        num_turns: u.num_turns,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::activations::forecast::{Action, HistoryEntry, Observation};
    use crate::activations::forecast::TrialResponse;

    fn empty_belief() -> TrialResponse {
        serde_json::from_value(serde_json::json!({
            "probability": 0.5,
            "summary": "",
            "evidence_for": [],
            "evidence_against": [],
            "open_questions": []
        }))
        .unwrap()
    }

    #[test]
    fn first_step_prompt_includes_question_and_format_contract() {
        let history: Vec<HistoryEntry> = vec![];
        let ctx = StepContext {
            initial_question: "Will X happen?",
            step_idx: 0,
            max_steps: 10,
            history: &history,
        };
        let p = build_session_prompt(&ctx);
        assert!(p.contains("Step 1/10"));
        assert!(p.contains("Will X happen?"));
        assert!(p.contains("```json"));
        assert!(p.contains("\"action\""));
        assert!(p.contains("\"belief\""));
        assert!(p.contains("\"type\""), "must remind model of internally-tagged Action shape");
    }

    #[test]
    fn subsequent_step_prompt_renders_last_observation() {
        let history = vec![HistoryEntry {
            action: Action::WebSearch {
                query: "BTC".into(),
                k: 3,
            },
            observation: Observation::Error {
                message: "rate limited".into(),
            },
            belief: empty_belief(),
        }];
        let ctx = StepContext {
            initial_question: "Will X happen?",
            step_idx: 1,
            max_steps: 10,
            history: &history,
        };
        let p = build_session_prompt(&ctx);
        assert!(p.contains("Step 2/10"));
        assert!(p.contains("rate limited"));
        assert!(!p.contains("This is your LAST step"));
    }

    #[test]
    fn last_step_prompt_demands_submit() {
        let history = vec![HistoryEntry {
            action: Action::WebSearch {
                query: "BTC".into(),
                k: 3,
            },
            observation: Observation::Summary {
                text: "fine".into(),
            },
            belief: empty_belief(),
        }];
        let ctx = StepContext {
            initial_question: "Will X happen?",
            step_idx: 9,
            max_steps: 10,
            history: &history,
        };
        let p = build_session_prompt(&ctx);
        assert!(p.contains("Step 10/10"));
        assert!(p.contains("LAST step"));
        assert!(p.contains("submit"));
    }
}
