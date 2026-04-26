//! Program lifecycle integration into the dispatch path.
//!
//! ## What this needs to do
//!
//! Wrap every external method call that arrives via Plexus RPC with a Program:
//!
//! ```text
//!   incoming RPC method call
//!         │
//!         ▼
//!   Program::open(programs_root, method_name, params, versions)
//!         │
//!         ▼
//!   dispatch to activation method (with program in context)
//!         │
//!         ├── activation calls swarm.trial / aggregate / etc.
//!         │   each call records a TraceEntry to the program directory
//!         │
//!         ▼
//!   on success: program.close_completed(returned value, schema_version)
//!   on error:   program.close_failed(kind, message, stage)
//! ```
//!
//! ## Where this hooks in
//!
//! The substrate's dispatch path lives in `src/plexus/` (specifically the
//! [`DynamicHub`] dispatch and the jsonrpsee bridge). The cleanest insertion
//! point is most likely a middleware layer at the jsonrpsee `RpcModule` level,
//! but two alternatives are worth checking before committing:
//!
//! 1. **jsonrpsee middleware** — wrap the RpcModule with a tower-style
//!    middleware that opens/closes a program around each method call. Pros:
//!    no changes to activation code; fully external. Cons: jsonrpsee's
//!    middleware story is less mature than tower's.
//! 2. **Wrapper trait in plexus-core** — extend the [`Activation`] trait with
//!    a hook the dispatcher calls. Pros: matches existing extension patterns.
//!    Cons: requires upstream changes.
//! 3. **DynamicHub wrapper** — intercept inside [`DynamicHub::call`] (or
//!    whatever the macro-generated dispatch entry point is). Pros: minimal;
//!    one site. Cons: may need access to method name and params before they
//!    are deserialized.
//!
//! Recommend (1) if jsonrpsee exposes a hook; fall back to (3). Avoid (2)
//! until other options are exhausted.
//!
//! ## Context threading
//!
//! Once a Program is open, the activation method needs to reach it. Three
//! options:
//!
//! - **tokio task-local** — convenient, spooky-action-at-a-distance
//! - **explicit `&Program` parameter** on the activation method — explicit,
//!   breaks every signature
//! - **jsonrpsee `Extensions`** — already exists, may be the natural fit
//!
//! Recommend `Extensions` if it works; the macro can be taught to thread the
//! program context through.
//!
//! ## Status
//!
//! Stub. The `ProgramRuntime` type below is the API; its methods will become
//! real once dispatch interception is wired in.

use std::path::PathBuf;
use std::sync::Arc;

use crate::mneme::program::{Program, ProgramError};

/// Configuration for the program runtime.
#[derive(Debug, Clone)]
pub struct ProgramRuntimeConfig {
    /// Filesystem root where `programs/<id>/` directories live.
    pub programs_root: PathBuf,
    /// Substrate version, embedded in every manifest.
    pub substrate_version: String,
    /// mneme harness version, embedded in every manifest.
    pub mneme_version: String,
}

impl ProgramRuntimeConfig {
    pub fn new(programs_root: impl Into<PathBuf>) -> Self {
        Self {
            programs_root: programs_root.into(),
            substrate_version: env!("CARGO_PKG_VERSION").to_string(),
            mneme_version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

/// Owns the program lifecycle integration. Held by the substrate's main hub
/// and consulted by the dispatch interceptor.
#[derive(Debug, Clone)]
pub struct ProgramRuntime {
    config: Arc<ProgramRuntimeConfig>,
}

impl ProgramRuntime {
    pub fn new(config: ProgramRuntimeConfig) -> Self {
        Self {
            config: Arc::new(config),
        }
    }

    pub fn config(&self) -> &ProgramRuntimeConfig {
        &self.config
    }

    /// Open a program for an incoming top-level method call.
    /// The dispatch interceptor calls this before invoking the activation.
    pub fn open(
        &self,
        entry_skill: impl Into<String>,
        inputs: serde_json::Value,
    ) -> Result<Program, ProgramError> {
        Program::open(
            &self.config.programs_root,
            entry_skill,
            inputs,
            self.config.substrate_version.clone(),
            self.config.mneme_version.clone(),
        )
    }

    /// Open a child program for a loopback call.
    /// Bumps depth; refuses past the recursion cap.
    pub fn open_child(
        &self,
        parent: &Program,
        entry_skill: impl Into<String>,
        inputs: serde_json::Value,
    ) -> Result<Program, ProgramError> {
        Program::open_child(parent, entry_skill, inputs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    #[test]
    fn open_creates_program_under_root() {
        let root = TempDir::new().unwrap();
        let runtime = ProgramRuntime::new(ProgramRuntimeConfig::new(root.path()));
        let prog = runtime.open("forecast.update", json!({})).unwrap();
        assert!(prog.directory().root().starts_with(root.path()));
    }

    #[test]
    fn config_defaults_to_pkg_version() {
        let root = TempDir::new().unwrap();
        let cfg = ProgramRuntimeConfig::new(root.path());
        assert_eq!(cfg.substrate_version, env!("CARGO_PKG_VERSION"));
        assert_eq!(cfg.mneme_version, env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn open_child_under_open_parent() {
        let root = TempDir::new().unwrap();
        let runtime = ProgramRuntime::new(ProgramRuntimeConfig::new(root.path()));
        let parent = runtime.open("outer", json!({})).unwrap();
        let child = runtime.open_child(&parent, "inner", json!({})).unwrap();
        assert_eq!(child.depth(), 1);
        assert!(child
            .directory()
            .root()
            .to_string_lossy()
            .contains("/skills/"));
    }
}
