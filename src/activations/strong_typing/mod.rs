//! Strong typing activation — domain newtype proposals as a Plexus skill.
//!
//! See `activation.rs` for methods, `types.rs` for event types.

mod activation;
mod types;

pub use activation::StrongTyping;
pub use types::ProposeEvent;
