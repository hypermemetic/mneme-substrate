//! Programs activation — query + inspect program directories.

mod activation;
mod types;

pub use activation::Programs;
pub use types::{
    parse_status, status_string, InspectEvent, ListEvent, ProgramDetail, ProgramSummary,
    StatusEvent,
};
