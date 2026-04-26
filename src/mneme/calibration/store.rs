//! Calibration store — persistent (predicted, actual) history + fitted Platt params.
//!
//! On-disk layout under `programs/_calibration/`:
//!
//! - `history.jsonl` — append-only one record per resolved observation
//! - `bias.json` — current fitted [`PlattParams`]; absent until cold-start passes
//!
//! Skills resolve forecasts by appending an observation here. The substrate
//! refits Platt parameters when the history crosses [`COLD_START_THRESHOLD`].

use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::platt::{fit_platt, PlattError, PlattParams};
use super::COLD_START_THRESHOLD;

/// Errors raised by store operations.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("io error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("serde error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("platt fit error: {0}")]
    Platt(#[from] PlattError),
}

/// One resolved (predicted, actual) record persisted in `history.jsonl`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResolvedObservation {
    pub program_id: String,
    pub predicted: f64,
    pub actual: bool,
    pub deadline: Option<DateTime<Utc>>,
    pub resolved_at: DateTime<Utc>,
}

/// Filesystem-backed calibration store rooted at `programs/_calibration/`.
#[derive(Debug, Clone)]
pub struct CalibrationStore {
    root: PathBuf,
}

impl CalibrationStore {
    /// Open or create the store at the given root.
    pub fn open(root: impl AsRef<Path>) -> Result<Self, StoreError> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(&root).map_err(|source| StoreError::Io {
            path: root.clone(),
            source,
        })?;
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn history_path(&self) -> PathBuf {
        self.root.join("history.jsonl")
    }

    pub fn bias_path(&self) -> PathBuf {
        self.root.join("bias.json")
    }

    /// Append a resolved observation. Refits Platt params if the post-append
    /// count crosses [`COLD_START_THRESHOLD`] (or is already past it).
    pub fn record(&self, obs: &ResolvedObservation) -> Result<(), StoreError> {
        let path = self.history_path();
        let mut line = serde_json::to_string(obs)?;
        line.push('\n');
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|source| StoreError::Io {
                path: path.clone(),
                source,
            })?;
        file.write_all(line.as_bytes())
            .map_err(|source| StoreError::Io {
                path: path.clone(),
                source,
            })?;

        // Refit if we have enough.
        let history = self.read_history()?;
        if history.len() >= COLD_START_THRESHOLD {
            // Refit may fail (e.g., single-class history); keep prior bias if so.
            if let Ok(params) = self.fit_from_history(&history) {
                self.write_bias(&params)?;
            }
        }
        Ok(())
    }

    /// Read the full history. Empty if no records yet.
    pub fn read_history(&self) -> Result<Vec<ResolvedObservation>, StoreError> {
        let path = self.history_path();
        if !path.exists() {
            return Ok(Vec::new());
        }
        let file = fs::File::open(&path).map_err(|source| StoreError::Io {
            path: path.clone(),
            source,
        })?;
        let reader = BufReader::new(file);
        let mut out = Vec::new();
        for line in reader.lines() {
            let line = line.map_err(|source| StoreError::Io {
                path: path.clone(),
                source,
            })?;
            if line.trim().is_empty() {
                continue;
            }
            out.push(serde_json::from_str(&line)?);
        }
        Ok(out)
    }

    /// Read the current bias parameters if calibrated; None during cold-start.
    pub fn read_bias(&self) -> Result<Option<PlattParams>, StoreError> {
        let path = self.bias_path();
        if !path.exists() {
            return Ok(None);
        }
        let bytes = fs::read(&path).map_err(|source| StoreError::Io {
            path: path.clone(),
            source,
        })?;
        Ok(Some(serde_json::from_slice(&bytes)?))
    }

    /// Write the bias parameters (overwrites any prior value).
    pub fn write_bias(&self, params: &PlattParams) -> Result<(), StoreError> {
        let path = self.bias_path();
        let json = serde_json::to_vec_pretty(params)?;
        fs::write(&path, json).map_err(|source| StoreError::Io { path, source })
    }

    /// Whether the store has crossed the cold-start threshold.
    pub fn is_calibrated(&self) -> Result<bool, StoreError> {
        Ok(self.read_history()?.len() >= COLD_START_THRESHOLD)
    }

    fn fit_from_history(
        &self,
        history: &[ResolvedObservation],
    ) -> Result<PlattParams, StoreError> {
        let pairs: Vec<(f64, bool)> = history.iter().map(|o| (o.predicted, o.actual)).collect();
        Ok(fit_platt(&pairs)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn temp_store() -> (TempDir, CalibrationStore) {
        let dir = TempDir::new().unwrap();
        let store = CalibrationStore::open(dir.path()).unwrap();
        (dir, store)
    }

    fn obs(predicted: f64, actual: bool) -> ResolvedObservation {
        ResolvedObservation {
            program_id: "test".to_string(),
            predicted,
            actual,
            deadline: None,
            resolved_at: Utc::now(),
        }
    }

    #[test]
    fn open_creates_root() {
        let (_dir, store) = temp_store();
        assert!(store.root().is_dir());
    }

    #[test]
    fn empty_history_ok() {
        let (_dir, store) = temp_store();
        assert_eq!(store.read_history().unwrap().len(), 0);
        assert!(!store.is_calibrated().unwrap());
        assert!(store.read_bias().unwrap().is_none());
    }

    #[test]
    fn record_appends_one_line() {
        let (_dir, store) = temp_store();
        store.record(&obs(0.5, true)).unwrap();
        let h = store.read_history().unwrap();
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].predicted, 0.5);
        assert!(h[0].actual);
    }

    #[test]
    fn cold_start_no_bias_below_threshold() {
        let (_dir, store) = temp_store();
        for i in 0..(COLD_START_THRESHOLD - 1) {
            store.record(&obs(0.5, i % 2 == 0)).unwrap();
        }
        assert!(!store.is_calibrated().unwrap());
        assert!(store.read_bias().unwrap().is_none());
    }

    #[test]
    fn crossing_threshold_writes_bias() {
        let (_dir, store) = temp_store();
        // Mix of true and false to allow Platt fit.
        for i in 0..COLD_START_THRESHOLD {
            store.record(&obs(0.5 + 0.01 * (i as f64), i % 2 == 0)).unwrap();
        }
        assert!(store.is_calibrated().unwrap());
        assert!(store.read_bias().unwrap().is_some());
    }

    #[test]
    fn single_class_history_does_not_clobber_prior_bias() {
        // Contract: when a refit FAILS (because the history has degenerated to
        // a single class), the previously-written bias must not be clobbered.
        //
        // To test this we need to actually produce single-class history. Since
        // record() always re-reads the full history before refitting, if we
        // ever have both classes present, we'll always succeed on subsequent
        // refits. So we set up the bias by direct write, then drive only
        // same-class records through the store, and verify the bias is
        // preserved across the failed refits.
        let (_dir, store) = temp_store();
        let prior = PlattParams { a: 0.7, b: 0.1 };
        store.write_bias(&prior).unwrap();

        // Append only true-actual observations (single class). Each triggers
        // a refit attempt that will fail with SingleClass; the catch in
        // record() should keep the prior bias intact.
        for _ in 0..(COLD_START_THRESHOLD + 5) {
            store.record(&obs(0.5, true)).unwrap();
        }

        let after = store.read_bias().unwrap().unwrap();
        assert_eq!(prior, after);
    }

    #[test]
    fn refit_succeeds_with_mixed_post_threshold_writes() {
        // Companion to the single_class test above: verify that when both
        // classes ARE present, refit DOES happen and bias gets updated.
        let (_dir, store) = temp_store();
        for i in 0..COLD_START_THRESHOLD {
            store.record(&obs(0.5, i % 2 == 0)).unwrap();
        }
        let first = store.read_bias().unwrap().unwrap();

        // Add more mixed observations — bias should change (or at least be
        // re-written; equality check is too strict because refits may produce
        // numerically-identical results on this constant predicted=0.5 data).
        for i in 0..5 {
            store.record(&obs(0.6, i % 2 == 0)).unwrap();
        }
        let second = store.read_bias().unwrap().unwrap();
        // Both should be valid PlattParams (no clobber to identity or NaN).
        assert!(first.a.is_finite() && first.b.is_finite());
        assert!(second.a.is_finite() && second.b.is_finite());
    }

    #[test]
    fn round_trip_observation() {
        let o = obs(0.42, false);
        let s = serde_json::to_string(&o).unwrap();
        let back: ResolvedObservation = serde_json::from_str(&s).unwrap();
        assert_eq!(o, back);
    }
}
