//! LLM capability registry — maps semantic capability names to model
//! configurations.
//!
//! The substrate's components (forecast, swarm runtime, future skills)
//! ask for *capabilities* by name (e.g. "json_cleanup", "reasoning_strong")
//! rather than picking a model directly. This decouples "what kind of
//! thinking is needed" from "which model do we currently use for it" —
//! upgrading from Haiku-3 to Haiku-4 (or swapping in a future cheaper
//! model) is a registry config change, not a code change scattered
//! across the codebase.
//!
//! See `mneme/plans/MNEME/MNEME-29.md` for the design rationale + the
//! motivating use case (JSON-cleanup recovery on parse failures).
//!
//! ## Default capabilities (substrate ships with these)
//!
//! | name | model | tier | use |
//! |---|---|---|---|
//! | `reasoning_default` | Sonnet | Standard | iterative loop's main reasoner |
//! | `reasoning_strong` | Opus | Premium | future BLFX-14 final-step commitment |
//! | `json_cleanup` | Haiku | Cheap | recover malformed JSON output |
//! | `summarize_short` | Haiku | Cheap | future search-worker summarization |
//! | `leak_classifier` | Haiku | Cheap | BLFX-9 layer 2 — drop search hits that look post-cutoff |

use std::collections::HashMap;
use std::sync::Arc;

use futures::StreamExt;
use plexus_core::plexus::HubContext;
use thiserror::Error;
use uuid::Uuid;

use crate::activations::claudecode::{ChatEvent, ClaudeCode, CreateResult, Model};

/// Cost tier for a capability — used by callers that want to pick the
/// cheapest model that meets a quality bar, or by future budget-tracking
/// code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CostTier {
    /// Haiku-tier — fastest, cheapest. Suitable for structural tasks
    /// (JSON cleanup, reformatting, short summaries) but not deep reasoning.
    Cheap,
    /// Sonnet-tier — balanced. The default for reasoning tasks.
    Standard,
    /// Opus-tier — most capable, most expensive. Reserve for tasks where
    /// the marginal quality improvement is decision-relevant.
    Premium,
}

/// One capability entry — semantic name, the model that implements it,
/// and metadata.
#[derive(Debug, Clone)]
pub struct Capability {
    pub name: String,
    pub model: Model,
    pub cost_tier: CostTier,
    /// Optional max-token hint for callers that want to enforce a budget
    /// per invocation. Not currently enforced; future hook.
    pub max_tokens: u32,
}

/// Errors raised by capability invocations.
#[derive(Debug, Error)]
pub enum CapabilityError {
    #[error("unknown capability: {0}")]
    UnknownCapability(String),
    #[error("session create failed: {0}")]
    SessionCreate(String),
    #[error("chat error: {0}")]
    Chat(String),
    #[error("chat ended without Complete event and no buffered text")]
    EmptyResponse,
}

/// Registry mapping capability name → [`Capability`]. Held by the
/// substrate; passed to runtime code that needs to invoke capabilities.
#[derive(Debug, Clone, Default)]
pub struct CapabilityRegistry {
    by_name: HashMap<String, Capability>,
}

impl CapabilityRegistry {
    /// Empty registry (no capabilities). Mostly for tests.
    pub fn empty() -> Self {
        Self {
            by_name: HashMap::new(),
        }
    }

    /// Default substrate registry — the four canonical capabilities
    /// described at the top of this module.
    pub fn default_substrate() -> Self {
        let mut r = Self::empty();
        r.register(Capability {
            name: "reasoning_default".to_string(),
            model: Model::Sonnet,
            cost_tier: CostTier::Standard,
            max_tokens: 4096,
        });
        r.register(Capability {
            name: "reasoning_strong".to_string(),
            model: Model::Opus,
            cost_tier: CostTier::Premium,
            max_tokens: 4096,
        });
        r.register(Capability {
            name: "json_cleanup".to_string(),
            model: Model::Haiku,
            cost_tier: CostTier::Cheap,
            max_tokens: 1024,
        });
        r.register(Capability {
            name: "summarize_short".to_string(),
            model: Model::Haiku,
            cost_tier: CostTier::Cheap,
            max_tokens: 1024,
        });
        r.register(Capability {
            name: "leak_classifier".to_string(),
            model: Model::Haiku,
            cost_tier: CostTier::Cheap,
            max_tokens: 32, // single-token YES/NO answer; ample headroom
        });
        r
    }

    /// Register a capability. Replaces an existing entry with the same name.
    pub fn register(&mut self, cap: Capability) {
        self.by_name.insert(cap.name.clone(), cap);
    }

    pub fn get(&self, name: &str) -> Option<&Capability> {
        self.by_name.get(name)
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.by_name.keys().map(String::as_str)
    }

    /// Invoke a capability one-shot: create an ephemeral session at the
    /// capability's model, send `prompt`, return the assistant text.
    /// Session is left in claudecode's storage (not auto-deleted) so the
    /// substrate's existing session inspection works for forensic
    /// debugging; a future cleanup ticket may add automatic disposal.
    pub async fn invoke<P: HubContext + 'static>(
        &self,
        claudecode: Arc<ClaudeCode<P>>,
        capability_name: &str,
        prompt: String,
        working_dir: String,
    ) -> Result<String, CapabilityError> {
        let cap = self
            .get(capability_name)
            .ok_or_else(|| CapabilityError::UnknownCapability(capability_name.to_string()))?;

        let session_name = format!("cap-{}-{}", cap.name, Uuid::new_v4());
        let create_stream = claudecode
            .create(
                session_name.clone(),
                working_dir,
                cap.model,
                None,
                None,
                None,
            )
            .await;
        let mut create_stream = Box::pin(create_stream);
        match create_stream.next().await {
            Some(CreateResult::Ok { .. }) => {}
            Some(CreateResult::Err { message }) => {
                return Err(CapabilityError::SessionCreate(message));
            }
            None => return Err(CapabilityError::SessionCreate("no result".into())),
        }

        let stream = claudecode.chat(session_name, prompt, None, Some(vec![])).await;
        let mut stream = Box::pin(stream);
        let mut buffer = String::new();
        while let Some(event) = stream.next().await {
            match event {
                ChatEvent::Content { text } => buffer.push_str(&text),
                ChatEvent::Complete { .. } => return Ok(buffer),
                ChatEvent::Err { message } => return Err(CapabilityError::Chat(message)),
                _ => {}
            }
        }
        if buffer.is_empty() {
            Err(CapabilityError::EmptyResponse)
        } else {
            Ok(buffer)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_registry_has_no_capabilities() {
        let r = CapabilityRegistry::empty();
        assert!(r.get("anything").is_none());
        assert_eq!(r.names().count(), 0);
    }

    #[test]
    fn default_substrate_has_required_capabilities() {
        let r = CapabilityRegistry::default_substrate();
        assert!(r.get("reasoning_default").is_some());
        assert!(r.get("reasoning_strong").is_some());
        assert!(r.get("json_cleanup").is_some());
        assert!(r.get("summarize_short").is_some());
    }

    #[test]
    fn json_cleanup_uses_haiku() {
        let r = CapabilityRegistry::default_substrate();
        let cap = r.get("json_cleanup").unwrap();
        assert_eq!(cap.model, Model::Haiku);
        assert_eq!(cap.cost_tier, CostTier::Cheap);
    }

    #[test]
    fn reasoning_strong_uses_opus() {
        let r = CapabilityRegistry::default_substrate();
        let cap = r.get("reasoning_strong").unwrap();
        assert_eq!(cap.model, Model::Opus);
        assert_eq!(cap.cost_tier, CostTier::Premium);
    }

    #[test]
    fn leak_classifier_uses_haiku() {
        let r = CapabilityRegistry::default_substrate();
        let cap = r.get("leak_classifier").unwrap();
        assert_eq!(cap.model, Model::Haiku);
        assert_eq!(cap.cost_tier, CostTier::Cheap);
    }

    #[test]
    fn register_overrides_existing() {
        let mut r = CapabilityRegistry::default_substrate();
        r.register(Capability {
            name: "json_cleanup".to_string(),
            model: Model::Sonnet, // override to Sonnet
            cost_tier: CostTier::Standard,
            max_tokens: 8192,
        });
        assert_eq!(r.get("json_cleanup").unwrap().model, Model::Sonnet);
    }
}
