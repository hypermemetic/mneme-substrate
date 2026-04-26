//! Ticketing activation — TDD ticket writing as a Plexus skill.
//!
//! See `activation.rs` for methods, `types.rs` for event types.

mod activation;
mod types;

pub use activation::Ticketing;
pub use types::WriteEvent;
