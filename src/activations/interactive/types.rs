//! Event types for the interactive activation

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Events emitted by the wizard method
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum WizardEvent {
    /// Wizard has started
    Started,

    /// Name has been collected from user
    NameCollected { name: String },

    /// Template has been selected
    TemplateSelected { template: String },

    /// Project was created successfully
    Created { name: String, template: String },

    /// User cancelled the wizard
    Cancelled,

    /// An error occurred
    Error { message: String },

    /// Wizard completed
    Done,
}

/// Events emitted by the delete method
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum DeleteEvent {
    /// File was deleted
    Deleted { path: String },

    /// User cancelled the operation
    Cancelled,

    /// All files processed
    Done,
}

/// Events emitted by the confirm method
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum ConfirmEvent {
    /// User confirmed
    Confirmed,

    /// User declined
    Declined,

    /// Error occurred (e.g., bidirectional not supported)
    Error { message: String },
}
