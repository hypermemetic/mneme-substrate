//! Interactive activation module - demonstrates bidirectional communication
//!
//! This activation showcases how to use `StandardBidirChannel` for interactive
//! workflows with confirmations, prompts, and selection menus.
//!
//! # Examples
//!
//! ## Using the wizard method
//!
//! The wizard method demonstrates a multi-step interactive flow:
//!
//! ```text
//! Client                              Server (wizard method)
//!   |                                       |
//!   |<--- WizardEvent::Started ------------|
//!   |                                       |
//!   |<--- Request: prompt("name") ---------|
//!   |---- Response: "my-project" --------->|
//!   |                                       |
//!   |<--- WizardEvent::NameCollected ------|
//!   |                                       |
//!   |<--- Request: select("template") -----|
//!   |---- Response: ["minimal"] ---------->|
//!   |                                       |
//!   |<--- WizardEvent::TemplateSelected ---|
//!   |                                       |
//!   |<--- Request: confirm("Create?") -----|
//!   |---- Response: true ----------------->|
//!   |                                       |
//!   |<--- WizardEvent::Created ------------|
//!   |<--- WizardEvent::Done ---------------|
//! ```
//!
//! ## MCP Transport Flow
//!
//! Over MCP, bidirectional requests work as follows:
//!
//! 1. Server sends logging notification with type="request"
//! 2. Client receives the request data
//! 3. Client calls `_plexus_respond` tool with `request_id` and response
//! 4. Server continues execution with the response

mod activation;
mod types;

pub use activation::Interactive;
pub use types::{ConfirmEvent, DeleteEvent, WizardEvent};
