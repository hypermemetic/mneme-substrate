//! Calibration — Platt scaling and the calibration store.
//!
//! Skills that produce binary probabilities (forecast, future security_review
//! exploit-likelihood) accumulate (predicted, actual) pairs as they resolve.
//! Once enough data exists, Platt scaling fits a sigmoid correction:
//!
//!   `p_calibrated = sigmoid(a · logit(p_raw) + b)`
//!
//! The substrate hosts a single calibration store at
//! `programs/_calibration/`. Skills opt in by reading/writing through the
//! store API rather than touching the filesystem directly.
//!
//! **Cold-start policy:** until at least [`COLD_START_THRESHOLD`] resolved
//! observations exist, calibration is identity-with-eps shrinkage toward 0.5.
//! Pretending we can fit Platt parameters on 3 data points is precision
//! theater that's worse than admitting we don't know yet.

pub mod platt;
pub mod store;

pub use platt::{fit_platt, platt_apply, PlattParams, PlattError};
pub use store::{CalibrationStore, ResolvedObservation, StoreError};

/// Minimum resolved observations before Platt fitting kicks in. Below this
/// threshold, calibration is identity (or gentle shrinkage toward 0.5).
pub const COLD_START_THRESHOLD: usize = 10;
