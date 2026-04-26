//! Substrate-side glue between mneme core types and the existing substrate.
//!
//! The modules in `mneme/` (program, swarm, respond, calibration) are pure
//! data + math. This `runtime/` module bridges them to the substrate runtime:
//! it owns the integration points where program lifecycle hooks into dispatch,
//! where the per-program tool registry lives in the loopback MCP, where
//! claudecode sessions get attributed to programs, and where the swarm
//! orchestration actually drives forks + chats.
//!
//! ## Status (as of this commit)
//!
//! These modules are scaffolds. Each has a clearly-typed public API and an
//! integration plan in its doc comment. The actual hooks into the substrate's
//! dispatch path, loopback MCP, and claudecode activation require touching
//! substrate-internal code paths that should land with the user present, not
//! during an autonomous build session.
//!
//! What IS implemented here:
//! - Type signatures for each runtime component (so consumers can be
//!   written and tested against them)
//! - Doc comments describing exactly which substrate file each piece must
//!   modify and what the change looks like
//!
//! What is NOT implemented here:
//! - The dispatch interception itself
//! - Loopback MCP enhancements
//! - Direct calls into the claudecode activation
//!
//! See `mneme/plans/MNEME/MNEME-3.md` (respond), `MNEME-4.md` (swarm.trial),
//! `MNEME-7.md` (recording integration) for the full contracts.

pub mod program_runtime;
pub mod session_attribution;
pub mod swarm_runtime;
pub mod tool_registry;
