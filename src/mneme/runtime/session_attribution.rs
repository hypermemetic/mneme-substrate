//! Session attribution — maps claudecode session ids to the program that
//! owns them.
//!
//! ## What this needs to do
//!
//! When a skill activation (or the swarm runtime on its behalf) creates a
//! `claudecode` session, that session belongs to a particular program. The
//! attribution lets us:
//!
//! - Auto-export the session into `programs/<id>/sessions/` when the program
//!   closes
//! - Route loopback tool registrations correctly (the loopback receives a
//!   session id; we look up the program; we look up the program's `respond`
//!   tool in the registry)
//! - Forbid cross-program session sharing (a session belongs to exactly one
//!   program for its lifetime)
//!
//! ## Where this hooks in
//!
//! When swarm or a skill activation calls `claudecode.create` or `claudecode.fork`,
//! the substrate-side wrapper must attribute the resulting session id here
//! before returning. When a program closes, it iterates the attributions and
//! triggers `claudecode.sessions_export` for each.
//!
//! ## Status
//!
//! Stub. The data structure is here; the actual integration with the
//! claudecode activation lands in Phase 2 follow-up.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use crate::mneme::program::ProgramId;

/// Maps claudecode session ids to their owning program.
#[derive(Debug, Default, Clone)]
pub struct SessionAttribution {
    /// session_id -> program_id
    by_session: Arc<Mutex<HashMap<String, ProgramId>>>,
    /// program_id -> session_ids it owns
    by_program: Arc<Mutex<HashMap<ProgramId, HashSet<String>>>>,
}

impl SessionAttribution {
    pub fn new() -> Self {
        Self::default()
    }

    /// Attribute a session to a program. If the session was already attributed
    /// to a different program, returns the previous owner.
    pub fn attribute(&self, session_id: impl Into<String>, program_id: ProgramId) -> Option<ProgramId> {
        let session_id = session_id.into();
        let mut by_session = self.by_session.lock().expect("poisoned");
        let mut by_program = self.by_program.lock().expect("poisoned");

        let prev = by_session.insert(session_id.clone(), program_id.clone());
        if let Some(prev) = &prev {
            if let Some(set) = by_program.get_mut(prev) {
                set.remove(&session_id);
            }
        }
        by_program
            .entry(program_id)
            .or_insert_with(HashSet::new)
            .insert(session_id);
        prev
    }

    /// Look up which program owns a session.
    pub fn program_for(&self, session_id: &str) -> Option<ProgramId> {
        self.by_session.lock().expect("poisoned").get(session_id).cloned()
    }

    /// All session ids owned by a program.
    pub fn sessions_of(&self, program_id: &ProgramId) -> Vec<String> {
        self.by_program
            .lock()
            .expect("poisoned")
            .get(program_id)
            .map(|set| set.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Remove all attribution for a program (called on program close, after
    /// exports have been dispatched).
    pub fn clear_program(&self, program_id: &ProgramId) {
        let mut by_session = self.by_session.lock().expect("poisoned");
        let mut by_program = self.by_program.lock().expect("poisoned");
        if let Some(set) = by_program.remove(program_id) {
            for sid in set {
                by_session.remove(&sid);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attribute_then_lookup() {
        let attr = SessionAttribution::new();
        let pid = ProgramId::new();
        attr.attribute("sess-1", pid.clone());
        assert_eq!(attr.program_for("sess-1"), Some(pid.clone()));
        assert_eq!(attr.sessions_of(&pid), vec!["sess-1".to_string()]);
    }

    #[test]
    fn unknown_session_returns_none() {
        let attr = SessionAttribution::new();
        assert!(attr.program_for("nope").is_none());
    }

    #[test]
    fn reattribution_returns_prior_owner() {
        let attr = SessionAttribution::new();
        let p1 = ProgramId::new();
        let p2 = ProgramId::new();
        attr.attribute("s", p1.clone());
        let prev = attr.attribute("s", p2.clone());
        assert_eq!(prev, Some(p1.clone()));
        assert_eq!(attr.program_for("s"), Some(p2.clone()));
        // p1 no longer owns s.
        assert!(attr.sessions_of(&p1).is_empty());
        // p2 owns s.
        assert_eq!(attr.sessions_of(&p2), vec!["s".to_string()]);
    }

    #[test]
    fn program_can_own_multiple_sessions() {
        let attr = SessionAttribution::new();
        let pid = ProgramId::new();
        attr.attribute("a", pid.clone());
        attr.attribute("b", pid.clone());
        attr.attribute("c", pid.clone());
        let mut sessions = attr.sessions_of(&pid);
        sessions.sort();
        assert_eq!(sessions, vec!["a", "b", "c"]);
    }

    #[test]
    fn clear_program_removes_all() {
        let attr = SessionAttribution::new();
        let pid = ProgramId::new();
        attr.attribute("a", pid.clone());
        attr.attribute("b", pid.clone());
        attr.clear_program(&pid);
        assert!(attr.sessions_of(&pid).is_empty());
        assert!(attr.program_for("a").is_none());
        assert!(attr.program_for("b").is_none());
    }
}
