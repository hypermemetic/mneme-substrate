//! Forecast activation — BLF binary forecasting as a Plexus skill.
//!
//! See `activation.rs` for methods, `types.rs` for event and state types.

mod activation;
mod agent_loop;
mod types;

pub use activation::Forecast;
pub use agent_loop::{
    execute_action, parse_step, Action, Observation, ParseError, ParsedStep, SearchHit,
    TimeSeriesPoint,
};
pub use types::{
    CreateEvent, EvidenceItem, ForecastConfidence, ForecastState, PriorRef, ResolveEvent,
    TrialResponse, UpdateEvent, BELIEF_SCHEMA_VERSION,
};
