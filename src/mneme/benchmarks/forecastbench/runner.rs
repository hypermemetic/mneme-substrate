//! Backtest runner — loops resolved market questions through a forecaster
//! function, collects predictions, and scores them.
//!
//! The forecaster is passed as a closure so this module is decoupled from
//! the `forecast.update` Plexus method. Tests can inject a deterministic
//! mock; production binds to a closure that calls `forecast.update` against
//! a live substrate (separate runner binary, not in this module).

use std::pin::Pin;
use std::future::Future;

use chrono::{DateTime, Utc};

use super::types::MarketQuestionWithResolution;
use crate::mneme::benchmarks::score::{
    bootstrap_ci_brier, brier_index, mean_brier, BootstrapInterval,
};

/// One forecaster's call: given the question + freeze metadata, produce a
/// probability in `[0, 1]`. Errors propagate as `Err(reason)` and become
/// excluded predictions in the result.
pub type ForecasterFn = Box<
    dyn for<'a> Fn(&'a MarketQuestionWithResolution) -> Pin<Box<dyn Future<Output = Result<f64, String>> + Send + 'a>>
        + Send
        + Sync,
>;

/// Result of running a backtest over N resolved market questions.
#[derive(Debug, Clone)]
pub struct BacktestResult {
    pub predictions: Vec<PredictionRecord>,
    pub failures: Vec<FailureRecord>,
    pub mean_brier: Option<f64>,
    pub brier_index: Option<f64>,
    pub ci_95: Option<BootstrapInterval>,
}

#[derive(Debug, Clone)]
pub struct PredictionRecord {
    pub question_id: String,
    pub source: String,
    pub predicted: f64,
    pub actual: f64,
    pub resolution_date: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct FailureRecord {
    pub question_id: String,
    pub source: String,
    pub error: String,
}

impl PredictionRecord {
    pub fn brier(&self) -> f64 {
        (self.predicted - self.actual).powi(2)
    }
}

/// Run the forecaster against the first `take_n` joined questions.
/// `take_n = None` means run them all.
///
/// Returns predictions in the order they were processed (= resolution-date
/// ascending, since `join_market_questions` sorts that way).
pub async fn run_backtest(
    joined: &[MarketQuestionWithResolution],
    take_n: Option<usize>,
    forecaster: ForecasterFn,
) -> BacktestResult {
    let n = take_n.map(|t| t.min(joined.len())).unwrap_or(joined.len());
    let mut predictions = Vec::with_capacity(n);
    let mut failures = Vec::new();

    for q in joined.iter().take(n) {
        let qid = q
            .question
            .id
            .as_single()
            .map(|s| s.to_string())
            .unwrap_or_default();
        match forecaster(q).await {
            Ok(p) => {
                let p_clamped = p.clamp(0.0, 1.0);
                predictions.push(PredictionRecord {
                    question_id: qid,
                    source: q.question.source.clone(),
                    predicted: p_clamped,
                    actual: q.resolution.resolved_to,
                    resolution_date: q.resolution_datetime_utc,
                });
            }
            Err(reason) => {
                failures.push(FailureRecord {
                    question_id: qid,
                    source: q.question.source.clone(),
                    error: reason,
                });
            }
        }
    }

    let pa: Vec<(f64, f64)> = predictions.iter().map(|p| (p.predicted, p.actual)).collect();
    let mean_b = if pa.is_empty() { None } else { Some(mean_brier(&pa)) };
    let bi = brier_index(&pa);
    let ci = if pa.len() >= 2 {
        bootstrap_ci_brier(&pa, 1000, 0.95, Some(0xC0FFEE))
    } else {
        None
    };
    BacktestResult {
        predictions,
        failures,
        mean_brier: mean_b,
        brier_index: bi,
        ci_95: ci,
    }
}

/// Convenience: bind a closure that always returns the same probability.
/// Useful for sanity-checking the runner (e.g., always-0.5 baseline).
pub fn constant_forecaster(p: f64) -> ForecasterFn {
    Box::new(move |_q| {
        let p = p;
        Box::pin(async move { Ok(p) })
    })
}

/// Forecaster that uses the question's `freeze_datetime_value` directly as
/// the prediction (parsed as f64). For market questions this is the
/// market-implied probability at the cutoff — the natural "crowd baseline".
/// Unparseable freeze values become failures.
pub fn freeze_value_forecaster() -> ForecasterFn {
    Box::new(|q| {
        let v = q.question.freeze_datetime_value.clone();
        Box::pin(async move {
            v.parse::<f64>()
                .map_err(|e| format!("freeze_datetime_value `{}` not a number: {}", v, e))
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mneme::benchmarks::forecastbench::types::{FBQuestion, FBQuestionId, FBResolution};

    fn fixture_question(id: &str, freeze: &str) -> MarketQuestionWithResolution {
        MarketQuestionWithResolution {
            question: FBQuestion {
                id: FBQuestionId::Single(id.to_string()),
                source: "manifold".to_string(),
                question: format!("Q {}", id),
                resolution_criteria: "...".to_string(),
                background: None,
                url: "https://e.x".to_string(),
                freeze_datetime: "2024-07-12T00:00:00+00:00".to_string(),
                freeze_datetime_value: freeze.to_string(),
                freeze_datetime_value_explanation: "...".to_string(),
                market_info_resolution_datetime: None,
                forecast_horizons: vec![],
            },
            resolution: FBResolution {
                id: FBQuestionId::Single(id.to_string()),
                source: "manifold".to_string(),
                direction: None,
                resolution_date: "2024-12-31".to_string(),
                resolved_to: 1.0,
                resolved: true,
            },
            resolution_datetime_utc: chrono::DateTime::<Utc>::from_naive_utc_and_offset(
                chrono::NaiveDate::from_ymd_opt(2024, 12, 31)
                    .unwrap()
                    .and_hms_opt(0, 0, 0)
                    .unwrap(),
                Utc,
            ),
        }
    }

    #[tokio::test]
    async fn constant_forecaster_zero_five_brier_quarter() {
        let qs = vec![
            fixture_question("a", "0.5"),
            fixture_question("b", "0.5"),
            fixture_question("c", "0.5"),
            fixture_question("d", "0.5"),
        ];
        let r = run_backtest(&qs, None, constant_forecaster(0.5)).await;
        assert_eq!(r.predictions.len(), 4);
        assert_eq!(r.failures.len(), 0);
        assert!((r.mean_brier.unwrap() - 0.25).abs() < 1e-9);
        assert!(r.brier_index.unwrap().abs() < 1e-9);
    }

    #[tokio::test]
    async fn freeze_value_forecaster_uses_cutoff_market_price() {
        let qs = vec![fixture_question("a", "0.937")];
        let r = run_backtest(&qs, None, freeze_value_forecaster()).await;
        assert_eq!(r.predictions.len(), 1);
        assert!((r.predictions[0].predicted - 0.937).abs() < 1e-9);
    }

    #[tokio::test]
    async fn freeze_value_forecaster_fails_on_garbage() {
        let qs = vec![fixture_question("a", "not a number")];
        let r = run_backtest(&qs, None, freeze_value_forecaster()).await;
        assert_eq!(r.predictions.len(), 0);
        assert_eq!(r.failures.len(), 1);
    }

    #[tokio::test]
    async fn take_n_caps_iteration() {
        let qs = vec![
            fixture_question("a", "0.5"),
            fixture_question("b", "0.5"),
            fixture_question("c", "0.5"),
        ];
        let r = run_backtest(&qs, Some(2), constant_forecaster(0.5)).await;
        assert_eq!(r.predictions.len(), 2);
    }
}
