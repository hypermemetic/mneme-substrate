//! Backtest runner — loops resolved market questions through a forecaster
//! function, collects predictions, and scores them.
//!
//! The forecaster is passed as a closure so this module is decoupled from
//! the `forecast.update` Plexus method. Tests can inject a deterministic
//! mock; production binds to a closure that calls `forecast.update` against
//! a live substrate (separate runner binary, not in this module).

use std::pin::Pin;
use std::future::Future;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use futures::stream::{self, StreamExt};

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
/// `concurrency` controls how many forecaster calls are in flight
/// simultaneously. `1` is sequential (legacy / deterministic order).
/// `>1` uses `buffer_unordered` so faster questions don't block on slower
/// ones; results are sorted by resolution date afterward to keep reports
/// reproducible.
pub async fn run_backtest(
    joined: &[MarketQuestionWithResolution],
    take_n: Option<usize>,
    concurrency: usize,
    forecaster: ForecasterFn,
) -> BacktestResult {
    let n = take_n.map(|t| t.min(joined.len())).unwrap_or(joined.len());
    let concurrency = concurrency.max(1);
    let forecaster = Arc::new(forecaster);

    let questions: Vec<&MarketQuestionWithResolution> = joined.iter().take(n).collect();

    let outcomes: Vec<(String, String, DateTime<Utc>, f64, Result<f64, String>)> =
        stream::iter(questions)
            .map(|q| {
                let forecaster = forecaster.clone();
                async move {
                    let qid = q
                        .question
                        .id
                        .as_single()
                        .map(|s| s.to_string())
                        .unwrap_or_default();
                    let res = forecaster(q).await;
                    (
                        qid,
                        q.question.source.clone(),
                        q.resolution_datetime_utc,
                        q.resolution.resolved_to,
                        res,
                    )
                }
            })
            .buffer_unordered(concurrency)
            .collect()
            .await;

    let mut predictions = Vec::with_capacity(n);
    let mut failures = Vec::new();
    for (qid, source, res_date, actual, res) in outcomes {
        match res {
            Ok(p) => {
                predictions.push(PredictionRecord {
                    question_id: qid,
                    source,
                    predicted: p.clamp(0.0, 1.0),
                    actual,
                    resolution_date: res_date,
                });
            }
            Err(reason) => {
                failures.push(FailureRecord {
                    question_id: qid,
                    source,
                    error: reason,
                });
            }
        }
    }
    // Restore deterministic ordering by resolution date.
    predictions.sort_by_key(|p| p.resolution_date);
    failures.sort_by(|a, b| a.question_id.cmp(&b.question_id));

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
        let r = run_backtest(&qs, None, 1, constant_forecaster(0.5)).await;
        assert_eq!(r.predictions.len(), 4);
        assert_eq!(r.failures.len(), 0);
        assert!((r.mean_brier.unwrap() - 0.25).abs() < 1e-9);
        assert!(r.brier_index.unwrap().abs() < 1e-9);
    }

    #[tokio::test]
    async fn freeze_value_forecaster_uses_cutoff_market_price() {
        let qs = vec![fixture_question("a", "0.937")];
        let r = run_backtest(&qs, None, 1, freeze_value_forecaster()).await;
        assert_eq!(r.predictions.len(), 1);
        assert!((r.predictions[0].predicted - 0.937).abs() < 1e-9);
    }

    #[tokio::test]
    async fn freeze_value_forecaster_fails_on_garbage() {
        let qs = vec![fixture_question("a", "not a number")];
        let r = run_backtest(&qs, None, 1, freeze_value_forecaster()).await;
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
        let r = run_backtest(&qs, Some(2), 1, constant_forecaster(0.5)).await;
        assert_eq!(r.predictions.len(), 2);
    }

    #[tokio::test]
    async fn concurrency_speeds_up_delayed_forecaster() {
        // Forecaster that always sleeps 200ms before returning. With 8 questions
        // and concurrency=4, total wall clock should be ≤ ~2× the per-call
        // delay (= 400ms), not 8× (1600ms).
        let qs: Vec<MarketQuestionWithResolution> =
            (0..8).map(|i| fixture_question(&format!("q{}", i), "0.5")).collect();
        let delayed: ForecasterFn = Box::new(|_q| {
            Box::pin(async {
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                Ok(0.5)
            })
        });
        let start = std::time::Instant::now();
        let r = run_backtest(&qs, None, 4, delayed).await;
        let elapsed = start.elapsed();
        assert_eq!(r.predictions.len(), 8);
        assert!(
            elapsed < std::time::Duration::from_millis(700),
            "concurrency=4 with 200ms delay should finish in ≤700ms (got {:?})",
            elapsed
        );
        assert!(
            elapsed >= std::time::Duration::from_millis(380),
            "should still take at least 2 batches (~400ms); got {:?}",
            elapsed
        );
    }

    #[tokio::test]
    async fn concurrency_one_runs_sequentially() {
        // Same setup, concurrency=1 should take ≥ 8×200ms.
        let qs: Vec<MarketQuestionWithResolution> =
            (0..4).map(|i| fixture_question(&format!("q{}", i), "0.5")).collect();
        let delayed: ForecasterFn = Box::new(|_q| {
            Box::pin(async {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                Ok(0.5)
            })
        });
        let start = std::time::Instant::now();
        let _ = run_backtest(&qs, None, 1, delayed).await;
        let elapsed = start.elapsed();
        assert!(
            elapsed >= std::time::Duration::from_millis(180),
            "sequential should take ≥4×50ms; got {:?}",
            elapsed
        );
    }
}
