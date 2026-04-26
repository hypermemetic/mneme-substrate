//! Program identifier — UUID v4 newtype.
//!
//! Programs are identified by an opaque UUID. Treating it as a newtype rather
//! than a bare string carries the evidence that it WAS validated as a program
//! id (see strong-typing skill — types are sufficient statistics).

use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

/// Globally unique identifier for a program (one skill invocation + its tree).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProgramId(Uuid);

impl ProgramId {
    /// Generate a fresh program id.
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Construct from an existing UUID (e.g., parsed from a manifest).
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    /// Underlying UUID.
    pub fn as_uuid(&self) -> &Uuid {
        &self.0
    }

    /// Hyphenated string form (e.g., for filesystem paths).
    pub fn as_str(&self) -> String {
        self.0.to_string()
    }
}

impl Default for ProgramId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for ProgramId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<Uuid> for ProgramId {
    fn from(uuid: Uuid) -> Self {
        Self(uuid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_ids_are_distinct() {
        let a = ProgramId::new();
        let b = ProgramId::new();
        assert_ne!(a, b);
    }

    #[test]
    fn serializes_as_bare_string() {
        let id = ProgramId::new();
        let json = serde_json::to_string(&id).unwrap();
        // No object wrapper {"0": "..."}; it's a raw string.
        assert!(json.starts_with('"'));
        assert!(json.ends_with('"'));
        assert_eq!(json.len(), 38); // 36 chars + 2 quotes
    }

    #[test]
    fn round_trips_through_serde() {
        let id = ProgramId::new();
        let json = serde_json::to_string(&id).unwrap();
        let back: ProgramId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, back);
    }

    #[test]
    fn display_matches_uuid_to_string() {
        let id = ProgramId::new();
        assert_eq!(format!("{}", id), id.as_uuid().to_string());
    }
}
