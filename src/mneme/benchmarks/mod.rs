//! Benchmark suite — datasets, scoring, and runners for evaluating mneme's
//! forecasting performance against published baselines.
//!
//! Modules:
//! - [`forecastbench`] — Karger et al. ICLR 2025 ForecastBench
//!   (https://github.com/forecastingresearch/forecastbench-datasets, CC BY-SA 4.0).
//!   Data ingestion + the runner that loops questions through `forecast.update`.
//! - [`score`] — Brier score and Brier Index (paper convention) plus a
//!   bootstrap confidence interval. Pure math; no I/O.
//!
//! Bench data lives under `programs/_benchmarks/forecastbench/` (gitignored).
//! Vendor a snapshot via the `forecastbench-datasets` repo's question_sets/
//! and resolution_sets/ directories.

pub mod forecastbench;
pub mod score;
