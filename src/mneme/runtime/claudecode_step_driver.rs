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

use chrono::Utc;
use uuid::Uuid;

use crate::activations::claudecode::{ChatEvent, ChatUsage, ClaudeCode, CreateResult, Model};
use crate::activations::forecast::{
    apply_search_query_date_filter, is_url_blocked, Action, EnvContext, Observation, ParseError,
    SearchHit, StepContext, StepDriver,
};
use crate::mneme::capabilities::CapabilityRegistry;
use crate::mneme::swarm::TrialUsage;

/// Per-step driver bound to one claudecode session.
pub(crate) struct ClaudecodeStepDriver<P: HubContext + 'static> {
    pub(crate) claudecode: Arc<ClaudeCode<P>>,
    pub(crate) session_name: String,
    pub(crate) allowed_tools: Option<Vec<String>>,
    pub(crate) working_dir: String,
    pub(crate) usage_accum: TrialUsage,
    /// Capability registry. The driver consults this for the
    /// `json_cleanup` capability when `parse_step` fails on the agent's
    /// output (MNEME-29).
    pub(crate) capabilities: CapabilityRegistry,
    /// Lazily-created sibling session used to execute `WebSearch` /
    /// `LookupUrl` actions in isolation from the main reasoning session.
    /// Keeping the search worker separate prevents search-tool clutter
    /// from polluting the reasoning session's history.
    search_session: Option<String>,
    /// BLFX-9 date-leakage defenses. Default = no enforcement (production
    /// forecasting); ForecastBench / held-out runs construct this with a
    /// non-None `cutoff_date` and per-question `blocked_urls`.
    pub(crate) env: EnvContext,
}

impl<P: HubContext + 'static> ClaudecodeStepDriver<P> {
    pub(crate) fn new(
        claudecode: Arc<ClaudeCode<P>>,
        session_name: String,
        allowed_tools: Option<Vec<String>>,
        working_dir: String,
    ) -> Self {
        Self::new_with_env(
            claudecode,
            session_name,
            allowed_tools,
            working_dir,
            EnvContext::default(),
        )
    }

    pub(crate) fn new_with_env(
        claudecode: Arc<ClaudeCode<P>>,
        session_name: String,
        allowed_tools: Option<Vec<String>>,
        working_dir: String,
        mut env: EnvContext,
    ) -> Self {
        let capabilities = CapabilityRegistry::default_substrate();
        // BLFX-9 layer 2: auto-attach the default Haiku-backed leak
        // classifier when a cutoff is set but the operator didn't
        // supply a classifier. ForecastBench / held-out runs always
        // fall through this path; production forecasting (no cutoff)
        // still pays nothing.
        if env.cutoff_date.is_some() && env.leak_classifier.is_none() {
            env.leak_classifier = Some(std::sync::Arc::new(
                crate::mneme::runtime::haiku_leak_classifier::HaikuLeakClassifier::new(
                    capabilities.clone(),
                    claudecode.clone(),
                    working_dir.clone(),
                ),
            ));
        }
        Self {
            claudecode,
            session_name,
            allowed_tools,
            working_dir,
            usage_accum: TrialUsage::default(),
            capabilities,
            search_session: None,
            env,
        }
    }

    /// Move out the accumulated usage at the end of a trial.
    pub(crate) fn into_usage(self) -> TrialUsage {
        self.usage_accum
    }

    /// Lazily create (or return) the search-worker session. Allows
    /// cross-action reuse so we're not repeatedly paying session-spawn
    /// costs for back-to-back searches.
    async fn ensure_search_session(&mut self) -> Result<String, String> {
        if let Some(name) = &self.search_session {
            return Ok(name.clone());
        }
        let name = format!("search-worker-{}", Uuid::new_v4());
        let create = self
            .claudecode
            .create(
                name.clone(),
                self.working_dir.clone(),
                Model::Sonnet,
                Some(SEARCH_WORKER_SYSTEM_PROMPT.to_string()),
                None,
                None,
            )
            .await;
        let mut create = Box::pin(create);
        match create.next().await {
            Some(CreateResult::Ok { .. }) => {
                self.search_session = Some(name.clone());
                Ok(name)
            }
            Some(CreateResult::Err { message }) => {
                Err(format!("create search session: {}", message))
            }
            None => Err("create search session returned no result".into()),
        }
    }

    /// Send a chat to the search worker, drain events, return the assistant
    /// text. Accumulates usage into the trial's running total.
    async fn search_chat(&mut self, prompt: String, allowed_tools: Vec<String>) -> Result<String, String> {
        let session = self.ensure_search_session().await?;
        let stream = self
            .claudecode
            .chat(session, prompt, None, Some(allowed_tools))
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
                    return Err(format!("search chat error: {}", message));
                }
                _ => {}
            }
        }
        if buffer.is_empty() {
            Err("search chat ended without Complete event".into())
        } else {
            Ok(buffer)
        }
    }
}

const SEARCH_WORKER_SYSTEM_PROMPT: &str = "You are a search worker. Your only \
job is to execute web searches or URL fetches and return raw results as JSON. \
You never reason, opine, or summarize beyond what was retrieved. You always \
return a fenced ```json block matching the requested shape.";

/// Schema description used by the json_cleanup capability when a parse_step
/// fails. Mirrors `ITERATIVE_FORMAT_CONTRACT` above but worded as a schema
/// for a cleanup-only model (Haiku) rather than as a contract for the
/// reasoning agent.
const ITERATIVE_STEP_SCHEMA_DESCRIPTION: &str = r#"{
  "action": {
    "type": "web_search" | "lookup_url" | "summarize_results"
            | "fetch_time_series" | "fetch_wikipedia_section" | "submit",
    // Plus type-specific required fields:
    //   web_search:    "query" (string), "k" (int 1..20)
    //   lookup_url:    "url" (https://... string)
    //   summarize_results: "result_ids" (non-empty list of strings)
    //   submit:        "probability" (float 0..1)
  },
  "belief": {
    "probability": float in [0, 1],          // required
    "summary": string,                        // required, may be ""
    "evidence_for": [{"claim": string, "weight": float}, ...] | [string, ...],
    "evidence_against": [{"claim": string, "weight": float}, ...] | [string, ...],
    "open_questions": [string, ...]
  }
}"#;

impl<P: HubContext + 'static> ClaudecodeStepDriver<P> {
    /// Run a real WebSearch via the search-worker session, parse the
    /// response into `SearchHit`s.
    async fn run_web_search(&mut self, query: &str, k: u8) -> Result<Vec<SearchHit>, String> {
        // BLFX-9 layer 1: substrate-level date filter on the search query.
        let filtered_query = apply_search_query_date_filter(query, self.env.cutoff_date);
        let prompt = format!(
            "Use the WebSearch tool exactly once to search for: {}\n\n\
             Return up to {} top results as a fenced ```json block whose contents \
             match this shape EXACTLY:\n\n\
             ```json\n[\n  {{\"url\": \"https://...\", \"title\": \"...\", \"snippet\": \"...\"}}\n]\n```\n\
             Only the JSON array — no prose around it.",
            filtered_query, k
        );
        let text = self.search_chat(prompt, vec!["WebSearch".to_string()]).await?;
        let raw = extract_json_block(&text)
            .ok_or_else(|| "search worker returned no JSON block".to_string())?;
        let parsed: serde_json::Value = serde_json::from_str(&raw)
            .map_err(|e| format!("parse search JSON: {}", e))?;
        let arr = parsed
            .as_array()
            .ok_or_else(|| "search JSON is not an array".to_string())?;
        let mut hits = Vec::with_capacity(arr.len());
        for (i, item) in arr.iter().enumerate() {
            let url = item.get("url").and_then(|v| v.as_str()).unwrap_or("").to_string();
            // BLFX-9 layer 4: drop blocked URLs from search results before
            // they ever reach the reasoning model.
            if is_url_blocked(&url, &self.env.blocked_urls) {
                tracing::debug!(url = %url, "BLFX-9: dropping search hit on blocklist");
                continue;
            }
            let title = item
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let snippet = item
                .get("snippet")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let hit = SearchHit {
                id: format!("r{}", i + 1),
                url,
                title,
                snippet,
                published_at: None,
            };
            // BLFX-9 layer 2: per-hit LLM leak classifier. Classifier
            // call only runs if a cutoff is set AND a classifier is
            // configured (production has neither; benches set both).
            if let (Some(cutoff), Some(classifier)) =
                (self.env.cutoff_date, self.env.leak_classifier.as_ref())
            {
                if classifier.classify(&hit, cutoff).await {
                    tracing::debug!(url = %hit.url, "BLFX-9: dropping hit flagged as leaked");
                    continue;
                }
            }
            hits.push(hit);
        }
        Ok(hits)
    }

    /// Fetch the content of a URL via the search worker (Read or WebFetch tool).
    async fn run_lookup_url(&mut self, url: &str) -> Result<String, String> {
        // BLFX-9 layer 4: refuse to fetch blocked URLs. Substrate-level
        // enforcement, not "we asked the LLM nicely."
        if is_url_blocked(url, &self.env.blocked_urls) {
            return Err(format!(
                "BLFX-9: URL `{}` is on the per-question blocklist; refusing fetch",
                url
            ));
        }
        let prompt = format!(
            "Fetch the content at this URL using the WebFetch tool: {}\n\n\
             Return the page's main text content as a fenced ```json block:\n\n\
             ```json\n{{\"content\": \"... the page text ...\"}}\n```\n\
             Truncate to ~3000 chars if longer. Only the JSON — no prose.",
            url
        );
        let text = self.search_chat(prompt, vec!["WebFetch".to_string()]).await?;
        let raw = extract_json_block(&text)
            .ok_or_else(|| "lookup worker returned no JSON block".to_string())?;
        let parsed: serde_json::Value = serde_json::from_str(&raw)
            .map_err(|e| format!("parse lookup JSON: {}", e))?;
        parsed
            .get("content")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| "lookup JSON missing `content` field".to_string())
    }
}

/// Extract the contents of the LAST fenced ```json block. Mirrors the
/// helper in `iterative_loop` but kept separate to avoid a cross-module
/// pub dependency.
fn extract_json_block(text: &str) -> Option<String> {
    let marker = "```json";
    let start = text.rfind(marker)?;
    let after = &text[start + marker.len()..];
    let end = after.find("```")?;
    Some(after[..end].trim().to_string())
}

#[async_trait]
impl<P: HubContext + 'static> StepDriver for ClaudecodeStepDriver<P> {
    async fn recover_parse(&mut self, raw: &str, err: &ParseError) -> Option<String> {
        let prompt = format!(
            "The following text was supposed to be JSON matching the iterative \
             forecast step contract:\n\n{}\n\n\
             The parser returned this error:\n  {}\n\n\
             Output ONLY the corrected JSON object — no prose, no markdown fence, \
             no apology, no explanation. If you must wrap it in a fence, use \
             ```json ... ``` so the parser can find it. If the structure is \
             genuinely unrecoverable, output an empty fence with `{{}}`.\n\n\
             ---\n\n{}",
            ITERATIVE_STEP_SCHEMA_DESCRIPTION, err, raw
        );
        match self
            .capabilities
            .invoke(
                self.claudecode.clone(),
                "json_cleanup",
                prompt,
                self.working_dir.clone(),
            )
            .await
        {
            Ok(cleaned) => Some(cleaned),
            Err(e) => {
                tracing::warn!("json_cleanup capability failed: {}", e);
                None
            }
        }
    }

    async fn execute_action(&mut self, action: Action) -> Observation {
        match action {
            Action::Submit { probability } => Observation::Submitted { probability },
            Action::WebSearch { query, k } => match self.run_web_search(&query, k).await {
                Ok(results) => Observation::SearchResults { results },
                Err(message) => Observation::Error { message },
            },
            Action::LookupUrl { url } => match self.run_lookup_url(&url).await {
                Ok(content) => Observation::PageContent {
                    url,
                    content,
                    fetched_at: Utc::now(),
                },
                Err(message) => Observation::Error { message },
            },
            // SummarizeResults / FetchTimeSeries / FetchWikipediaSection
            // remain stubbed; BLFX-15 (source tools) covers the rest.
            other => Observation::Error {
                message: format!(
                    "action {:?} not yet implemented in ClaudecodeStepDriver — see BLFX-15",
                    other
                ),
            },
        }
    }

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
