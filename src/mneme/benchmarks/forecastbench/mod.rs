//! ForecastBench (Karger et al. ICLR 2025) ingestion + runner glue.
//!
//! Data lives at https://github.com/forecastingresearch/forecastbench-datasets
//! under `datasets/question_sets/<DATE>-llm.json` and
//! `datasets/resolution_sets/<DATE>_resolution_set.json`. License: CC BY-SA 4.0.
//!
//! Dataset shape (from BLFX-S01 spike, 2026-04-26):
//! - One question_set per release (~biweekly since 2024-07).
//! - Each question_set contains `questions[]`. Sources split into:
//!   - **market sources** (manifold, metaculus, polymarket, infer): one
//!     question → one resolution at the market's `market_info_resolution_datetime`.
//!   - **dataset sources** (acled, dbnomics, fred, wikipedia, yfinance): one
//!     question → multiple resolutions, one per horizon
//!     (7d, 30d, 90d, 180d, 365d, 1825d, 3650d). Question text contains
//!     `{forecast_due_date}` / `{resolution_date}` f-string placeholders that
//!     must be rendered before sending to the model.
//! - Resolutions: `resolved_to ∈ [0, 1]` (probabilistic — markets can
//!   resolve fractionally on ambiguous outcomes). Use the raw float for
//!   Brier scoring; only threshold to bool when persisting to the
//!   `CalibrationStore` (which currently takes `actual: bool`).
//!
//! Phase 1 of BLFX-10 (this module) handles **market questions only** —
//! one-question-one-resolution shape, no f-string templating. Dataset
//! questions are deferred until BLFX-15 (source-specific tools).

pub mod loader;
pub mod runner;
pub mod types;

pub use loader::{join_market_questions, load_question_set, load_resolution_set, LoaderError};
pub use runner::{
    constant_forecaster, freeze_value_forecaster, run_backtest, BacktestResult, FailureRecord,
    ForecasterFn, PredictionRecord,
};
pub use types::{
    is_market_source, FBQuestion, FBQuestionId, FBQuestionSet, FBResolution, FBResolutionSet,
    MarketQuestionWithResolution,
};
