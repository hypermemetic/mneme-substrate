//! Forecast activation — BLF binary forecasting as a Plexus skill.
//!
//! See `activation.rs` for methods, `types.rs` for event and state types.

mod activation;
mod agent_loop;
mod iterative_loop;
mod types;

pub use activation::Forecast;
pub use agent_loop::{
    apply_search_query_date_filter, execute_action, is_url_blocked, parse_step, Action,
    EnvContext, LeakClassifier, Observation, ParseError, ParsedStep, SearchHit, TimeSeriesPoint,
};
pub use iterative_loop::{
    build_step_prompt, iterative_trial, HistoryEntry, LoopError, QueueStepDriver, StepContext,
    StepDriver,
};
pub use types::{
    CreateEvent, EvidenceItem, ForecastConfidence, ForecastState, PriorRef, ResolveEvent,
    TrialResponse, UpdateEvent, BELIEF_SCHEMA_VERSION,
};
