//! Brier score, Brier Index (ForecastBench / Murphy 2026 convention), and a
//! percentile-bootstrap confidence interval. Pure math; no I/O.
//!
//! ## Brier
//!
//! For a prediction `p ∈ [0, 1]` and an outcome `a ∈ [0, 1]` (allowed to be
//! probabilistic — ForecastBench resolution_to is in `[0,1]` because some
//! markets resolve fractionally on ambiguous outcomes):
//!
//!   `brier(p, a) = (p - a)²`
//!
//! Lower is better. Always-50/50 baseline scores `0.25`.
//!
//! ## Brier Index (BI)
//!
//! ForecastBench reports a Brier Index where higher is better, scaled to the
//! always-50/50 baseline:
//!
//!   `BI = 100 · (1 - mean_brier / 0.25)`
//!
//! BI = 0 means "no better than coin flip"; BI = 100 means "perfect"; BI > 0
//! means "better than 50/50"; BI < 0 means "worse than 50/50". Murphy 2026's
//! reported numbers are in this units.
//!
//! ## Bootstrap CI
//!
//! Percentile bootstrap: resample predictions with replacement N times,
//! recompute mean Brier each time, return the desired-percentile interval.
//! Default 1,000 resamples + 95% CI.

use rand::{Rng, SeedableRng};
use rand::rngs::StdRng;

/// Single-question Brier score: `(p - a)²`.
pub fn brier_score(p: f64, actual: f64) -> f64 {
    (p - actual).powi(2)
}

/// Mean Brier score across predictions. Empty input is `0.0` (caller's choice
/// to handle; we don't error). Use [`mean_brier_or_none`] if you want
/// `Option`-shaped semantics.
pub fn mean_brier(predictions: &[(f64, f64)]) -> f64 {
    if predictions.is_empty() {
        return 0.0;
    }
    let sum: f64 = predictions.iter().map(|(p, a)| brier_score(*p, *a)).sum();
    sum / predictions.len() as f64
}

/// Mean Brier wrapper that returns `None` when input is empty.
pub fn mean_brier_or_none(predictions: &[(f64, f64)]) -> Option<f64> {
    if predictions.is_empty() {
        None
    } else {
        Some(mean_brier(predictions))
    }
}

/// ForecastBench Brier Index — `100 · (1 - mean_brier / 0.25)`.
/// The 0.25 baseline is the Brier of always-predicting-0.5.
pub fn brier_index(predictions: &[(f64, f64)]) -> Option<f64> {
    mean_brier_or_none(predictions).map(|mb| 100.0 * (1.0 - mb / 0.25))
}

/// Percentile bootstrap on the mean Brier score.
/// Returns `(lower_bound, point_estimate, upper_bound)` at the requested
/// confidence level (e.g. `0.95` for 95% CI). Uses a deterministic seed so
/// results are reproducible across runs; if you want fresh randomness pass
/// `seed = None` and we'll mix in time.
///
/// Errors if `predictions` is empty or `confidence_level ∉ (0, 1)`.
pub fn bootstrap_ci_brier(
    predictions: &[(f64, f64)],
    n_resamples: usize,
    confidence_level: f64,
    seed: Option<u64>,
) -> Option<BootstrapInterval> {
    if predictions.is_empty() || !(0.0..1.0).contains(&confidence_level) || confidence_level <= 0.0 {
        return None;
    }
    let n = predictions.len();
    let seed = seed.unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0xC0FFEE)
    });
    let mut rng = StdRng::seed_from_u64(seed);

    let mut means = Vec::with_capacity(n_resamples);
    for _ in 0..n_resamples {
        let mut sum = 0.0;
        for _ in 0..n {
            let idx = rng.gen_range(0..n);
            let (p, a) = predictions[idx];
            sum += brier_score(p, a);
        }
        means.push(sum / n as f64);
    }
    means.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let alpha = 1.0 - confidence_level;
    let lo_idx = ((alpha / 2.0) * n_resamples as f64).floor() as usize;
    let hi_idx = ((1.0 - alpha / 2.0) * n_resamples as f64).ceil() as usize - 1;
    let lo = means[lo_idx.min(means.len() - 1)];
    let hi = means[hi_idx.min(means.len() - 1)];
    Some(BootstrapInterval {
        lower: lo,
        point_estimate: mean_brier(predictions),
        upper: hi,
    })
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BootstrapInterval {
    pub lower: f64,
    pub point_estimate: f64,
    pub upper: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brier_perfect_yes_is_zero() {
        assert_eq!(brier_score(1.0, 1.0), 0.0);
    }

    #[test]
    fn brier_perfect_no_is_zero() {
        assert_eq!(brier_score(0.0, 0.0), 0.0);
    }

    #[test]
    fn brier_coin_flip_is_quarter() {
        assert!((brier_score(0.5, 1.0) - 0.25).abs() < 1e-9);
        assert!((brier_score(0.5, 0.0) - 0.25).abs() < 1e-9);
    }

    #[test]
    fn brier_worst_case_is_one() {
        assert!((brier_score(1.0, 0.0) - 1.0).abs() < 1e-9);
        assert!((brier_score(0.0, 1.0) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn mean_brier_empty_returns_zero() {
        assert_eq!(mean_brier(&[]), 0.0);
    }

    #[test]
    fn mean_brier_or_none_empty_returns_none() {
        assert_eq!(mean_brier_or_none(&[]), None);
    }

    #[test]
    fn brier_index_coin_flip_baseline_is_zero() {
        let preds = vec![(0.5, 1.0), (0.5, 0.0), (0.5, 1.0), (0.5, 0.0)];
        let bi = brier_index(&preds).unwrap();
        assert!(bi.abs() < 1e-9, "BI should be 0 for always-50/50, got {}", bi);
    }

    #[test]
    fn brier_index_perfect_predictor_is_one_hundred() {
        let preds = vec![(1.0, 1.0), (0.0, 0.0), (1.0, 1.0), (0.0, 0.0)];
        let bi = brier_index(&preds).unwrap();
        assert!((bi - 100.0).abs() < 1e-9, "BI should be 100 for perfect, got {}", bi);
    }

    #[test]
    fn brier_index_anti_predictor_is_minus_three_hundred() {
        // worst case: predict 1.0 when actual=0 and 0.0 when actual=1 ⇒ Brier=1.0 each
        // BI = 100 * (1 - 1.0 / 0.25) = -300
        let preds = vec![(1.0, 0.0), (0.0, 1.0)];
        let bi = brier_index(&preds).unwrap();
        assert!((bi - -300.0).abs() < 1e-9, "BI should be -300 for anti, got {}", bi);
    }

    #[test]
    fn bootstrap_ci_perfect_predictor_has_tight_zero_interval() {
        // Every prediction perfect ⇒ every resample also perfect ⇒ mean=0 always.
        let preds: Vec<(f64, f64)> = (0..100)
            .map(|i| if i % 2 == 0 { (1.0, 1.0) } else { (0.0, 0.0) })
            .collect();
        let ci = bootstrap_ci_brier(&preds, 200, 0.95, Some(42)).unwrap();
        assert_eq!(ci.point_estimate, 0.0);
        assert_eq!(ci.lower, 0.0);
        assert_eq!(ci.upper, 0.0);
    }

    #[test]
    fn bootstrap_ci_returns_none_on_empty_input() {
        assert!(bootstrap_ci_brier(&[], 100, 0.95, Some(1)).is_none());
    }

    #[test]
    fn bootstrap_ci_invalid_confidence_returns_none() {
        let preds = vec![(0.5, 1.0)];
        assert!(bootstrap_ci_brier(&preds, 100, 0.0, Some(1)).is_none());
        assert!(bootstrap_ci_brier(&preds, 100, 1.0, Some(1)).is_none());
        assert!(bootstrap_ci_brier(&preds, 100, -0.5, Some(1)).is_none());
    }

    #[test]
    fn bootstrap_ci_brackets_point_estimate() {
        // Mixed correct/incorrect predictions; CI should bracket the mean.
        let preds: Vec<(f64, f64)> = (0..100)
            .map(|i| (0.7, if i < 70 { 1.0 } else { 0.0 }))
            .collect();
        let ci = bootstrap_ci_brier(&preds, 1000, 0.95, Some(42)).unwrap();
        assert!(ci.lower <= ci.point_estimate);
        assert!(ci.point_estimate <= ci.upper);
        assert!(ci.upper - ci.lower > 0.0); // some width
    }
}
