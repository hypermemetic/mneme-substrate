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

pub mod benchmarks;
pub mod calibration;
pub mod capabilities;
pub mod context;
pub mod program;
pub mod respond;
pub mod runtime;
pub mod storage;
pub mod swarm;

pub use context::MnemeContext;
pub use program::{Program, ProgramDirectory, ProgramError, ProgramId, ProgramStatus};
pub use storage::{MnemeStorage, ProgramRow, SharedStorage, StorageError};
