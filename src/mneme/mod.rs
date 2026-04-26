//! mneme — skill orchestration on Plexus RPC.
//!
//! This module is the substrate's mneme-specific machinery. It sits alongside
//! the inherited substrate (claudecode, arbor, etc.) and adds:
//!
//! - [`program`] — program lifecycle types and on-disk layout
//! - [`swarm`] — orchestration primitives (trial, aggregate, sequential, race)
//! - [`respond`] — structured-output protocol via per-program loopback tools
//! - [`calibration`] — Platt-scaling and the calibration store
//!
//! Plexus is the **boundary protocol** for outside callers. Inside the binary,
//! these modules compose via normal Rust function calls. See the architecture
//! diagram in the sibling `mneme/` repo's README.

pub mod calibration;
pub mod program;
pub mod respond;
pub mod runtime;
pub mod swarm;

pub use program::{Program, ProgramDirectory, ProgramError, ProgramId, ProgramStatus};
