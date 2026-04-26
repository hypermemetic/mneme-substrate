//! Per-program loopback tool registry — the substrate-side enhancement that
//! makes the `respond` protocol work.
//!
//! ## What this needs to do
//!
//! Today the substrate's loopback MCP (in `claudecode_loopback`) exposes a
//! fixed set of tools to running Claude sessions. The mneme architecture
//! needs:
//!
//! - **Per-program registration.** A skill activation registers a `respond`
//!   tool tagged with its program id. The tool is exposed only to claudecode
//!   sessions that belong to that program.
//! - **Schema-validated input.** The tool's input must match the registered
//!   JSON Schema. Invalid payloads bounce back to Claude with the validation
//!   error so it can self-correct.
//! - **RAII deregistration.** When the skill activation drops its handle,
//!   the tool is removed from the registry.
//!
//! ## Where this hooks in
//!
//! The loopback MCP server lives in (look around `src/activations/claudecode/`
//! and `src/mcp_*`). The change is to the MCP `tools/list` and `tools/call`
//! handlers: they need to consult this registry, filtered by the calling
//! session's program id.
//!
//! Session → program lookup is provided by [`super::session_attribution`].
//!
//! ## Status
//!
//! Stub. The data structures are in place; the actual MCP handler integration
//! lands when MNEME-S01 spike confirms the loopback supports schema-constrained
//! tool registration.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::mneme::program::ProgramId;
use crate::mneme::respond::RespondTool;

/// In-memory registry of `respond` tools, keyed by program id.
#[derive(Debug, Default, Clone)]
pub struct ToolRegistry {
    inner: Arc<Mutex<HashMap<ProgramId, RespondTool>>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a tool for a program. Overwrites any prior tool for the same
    /// program (each program registers at most one `respond` tool at a time).
    pub fn register(&self, tool: RespondTool) {
        let mut guard = self.inner.lock().expect("poisoned");
        guard.insert(tool.program_id.clone(), tool);
    }

    /// Look up the tool registered for a program, if any.
    pub fn get(&self, program_id: &ProgramId) -> Option<RespondTool> {
        self.inner.lock().expect("poisoned").get(program_id).cloned()
    }

    /// Remove the tool for a program. Called by RAII handle on drop.
    pub fn deregister(&self, program_id: &ProgramId) -> Option<RespondTool> {
        self.inner.lock().expect("poisoned").remove(program_id)
    }

    /// Number of tools currently registered.
    pub fn len(&self) -> usize {
        self.inner.lock().expect("poisoned").len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// RAII handle: registers on construction, deregisters on drop.
pub struct RegisteredTool {
    registry: ToolRegistry,
    program_id: ProgramId,
}

impl RegisteredTool {
    pub fn new(registry: ToolRegistry, tool: RespondTool) -> Self {
        let program_id = tool.program_id.clone();
        registry.register(tool);
        Self {
            registry,
            program_id,
        }
    }

    pub fn program_id(&self) -> &ProgramId {
        &self.program_id
    }
}

impl Drop for RegisteredTool {
    fn drop(&mut self) {
        self.registry.deregister(&self.program_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn schema() -> serde_json::Value {
        json!({"type": "object", "properties": {"v": {"type": "integer"}}})
    }

    #[test]
    fn register_then_get() {
        let registry = ToolRegistry::new();
        let id = ProgramId::new();
        let tool = RespondTool::new(id.clone(), schema());
        registry.register(tool.clone());
        let got = registry.get(&id).unwrap();
        assert_eq!(got.program_id, tool.program_id);
        assert_eq!(registry.len(), 1);
    }

    #[test]
    fn get_unknown_returns_none() {
        let registry = ToolRegistry::new();
        assert!(registry.get(&ProgramId::new()).is_none());
    }

    #[test]
    fn deregister_removes() {
        let registry = ToolRegistry::new();
        let id = ProgramId::new();
        registry.register(RespondTool::new(id.clone(), schema()));
        assert_eq!(registry.len(), 1);
        let removed = registry.deregister(&id);
        assert!(removed.is_some());
        assert_eq!(registry.len(), 0);
    }

    #[test]
    fn raii_handle_deregisters_on_drop() {
        let registry = ToolRegistry::new();
        let id = ProgramId::new();
        {
            let _handle = RegisteredTool::new(registry.clone(), RespondTool::new(id.clone(), schema()));
            assert_eq!(registry.len(), 1);
        }
        assert_eq!(registry.len(), 0);
        assert!(registry.get(&id).is_none());
    }

    #[test]
    fn register_overwrites() {
        let registry = ToolRegistry::new();
        let id = ProgramId::new();
        registry.register(RespondTool::new(id.clone(), schema()).with_max_attempts(3));
        registry.register(RespondTool::new(id.clone(), schema()).with_max_attempts(7));
        assert_eq!(registry.get(&id).unwrap().max_attempts, 7);
    }

    #[test]
    fn distinct_programs_isolated() {
        let registry = ToolRegistry::new();
        let id_a = ProgramId::new();
        let id_b = ProgramId::new();
        registry.register(RespondTool::new(id_a.clone(), schema()));
        registry.register(RespondTool::new(id_b.clone(), schema()));
        assert_eq!(registry.len(), 2);
        assert!(registry.get(&id_a).is_some());
        assert!(registry.get(&id_b).is_some());
    }
}
