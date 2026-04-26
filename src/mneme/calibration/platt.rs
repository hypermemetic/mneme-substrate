//! Platt scaling — fit and apply.
//!
//! Platt scaling fits two parameters `(a, b)` such that
//!   `p_calibrated = sigmoid(a · logit(p_raw) + b)`
//! minimizes log-loss on a set of (predicted, actual) pairs.
//!
//! We use a simple Newton-Raphson loop (the standard textbook approach) with
//! a small ridge term for numerical stability. Phase 1 implementation; Phase 3
//! (MNEME-15) will revisit if the dataset grows large enough to need it.

use serde::{Deserialize, Serialize};

const CLAMP_EPS: f64 = 1e-6;
const MAX_ITERATIONS: usize = 100;
const CONVERGENCE_TOLERANCE: f64 = 1e-6;
const RIDGE: f64 = 1e-3;

/// Errors from Platt fitting.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum PlattError {
    #[error("not enough observations: have {have}, need {need}")]
    NotEnoughObservations { have: usize, need: usize },
    #[error("observations contain only one class — cannot fit")]
    SingleClass,
    #[error("Newton iteration failed to converge after {0} steps")]
    NoConvergence(usize),
    #[error("predicted probability {0} is not in [0, 1]")]
    BadPredicted(f64),
}

/// Fitted parameters. `apply(p) = sigmoid(a · logit(p) + b)`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PlattParams {
    pub a: f64,
    pub b: f64,
}

impl PlattParams {
    /// Identity transform — pass `a=1, b=0` so `apply(p) = p` (modulo clamping).
    pub fn identity() -> Self {
        Self { a: 1.0, b: 0.0 }
    }
}

fn clamp(p: f64) -> f64 {
    p.max(CLAMP_EPS).min(1.0 - CLAMP_EPS)
}

fn logit(p: f64) -> f64 {
    let p = clamp(p);
    (p / (1.0 - p)).ln()
}

fn sigmoid(x: f64) -> f64 {
    1.0 / (1.0 + (-x).exp())
}

/// Apply fitted parameters to a raw probability.
pub fn platt_apply(params: PlattParams, p_raw: f64) -> Result<f64, PlattError> {
    if !(0.0..=1.0).contains(&p_raw) || !p_raw.is_finite() {
        return Err(PlattError::BadPredicted(p_raw));
    }
    Ok(sigmoid(params.a * logit(p_raw) + params.b))
}

/// Fit Platt parameters from (predicted, actual) pairs. `actual` is `bool`
/// representing whether the predicted event happened.
///
/// Requires both classes (at least one true and one false) to be present.
pub fn fit_platt(observations: &[(f64, bool)]) -> Result<PlattParams, PlattError> {
    if observations.is_empty() {
        return Err(PlattError::NotEnoughObservations {
            have: 0,
            need: 2,
        });
    }
    let pos_count = observations.iter().filter(|(_, y)| *y).count();
    let neg_count = observations.len() - pos_count;
    if pos_count == 0 || neg_count == 0 {
        return Err(PlattError::SingleClass);
    }

    // Pre-compute per-observation logits and targets.
    // For Platt's original formulation with smoothing:
    //   t_i = (N+ + 1) / (N+ + 2) if y_i = 1
    //   t_i = 1 / (N- + 2)        if y_i = 0
    let n_pos = pos_count as f64;
    let n_neg = neg_count as f64;
    let t_pos = (n_pos + 1.0) / (n_pos + 2.0);
    let t_neg = 1.0 / (n_neg + 2.0);

    let data: Vec<(f64, f64)> = observations
        .iter()
        .map(|(p, y)| (logit(*p), if *y { t_pos } else { t_neg }))
        .collect();

    // Newton-Raphson on negative log-likelihood. Variables: a, b.
    let mut a = 1.0_f64;
    let mut b = 0.0_f64;

    for _ in 0..MAX_ITERATIONS {
        let mut grad_a = 0.0;
        let mut grad_b = 0.0;
        let mut h_aa = 0.0;
        let mut h_ab = 0.0;
        let mut h_bb = 0.0;

        for &(li, ti) in &data {
            let z = a * li + b;
            let pi = sigmoid(z);
            let err = pi - ti;
            grad_a += err * li;
            grad_b += err;

            let w = pi * (1.0 - pi);
            h_aa += w * li * li;
            h_ab += w * li;
            h_bb += w;
        }

        // Add ridge for stability.
        h_aa += RIDGE;
        h_bb += RIDGE;

        // Solve 2x2 system H · delta = grad.
        let det = h_aa * h_bb - h_ab * h_ab;
        if det.abs() < 1e-12 {
            return Err(PlattError::NoConvergence(MAX_ITERATIONS));
        }
        let delta_a = (h_bb * grad_a - h_ab * grad_b) / det;
        let delta_b = (-h_ab * grad_a + h_aa * grad_b) / det;

        a -= delta_a;
        b -= delta_b;

        if delta_a.abs() < CONVERGENCE_TOLERANCE && delta_b.abs() < CONVERGENCE_TOLERANCE {
            return Ok(PlattParams { a, b });
        }
    }

    Err(PlattError::NoConvergence(MAX_ITERATIONS))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_is_passthrough_modulo_clamp() {
        let p = platt_apply(PlattParams::identity(), 0.4).unwrap();
        assert!((p - 0.4).abs() < 1e-9);
    }

    #[test]
    fn apply_rejects_out_of_range() {
        assert!(platt_apply(PlattParams::identity(), -0.1).is_err());
        assert!(platt_apply(PlattParams::identity(), 1.1).is_err());
    }

    #[test]
    fn apply_rejects_nan() {
        assert!(platt_apply(PlattParams::identity(), f64::NAN).is_err());
    }

    #[test]
    fn fit_requires_both_classes() {
        let obs = vec![(0.5, true), (0.6, true), (0.7, true)];
        assert_eq!(fit_platt(&obs), Err(PlattError::SingleClass));

        let obs = vec![(0.5, false), (0.6, false)];
        assert_eq!(fit_platt(&obs), Err(PlattError::SingleClass));
    }

    #[test]
    fn fit_empty_errors() {
        let obs: Vec<(f64, bool)> = vec![];
        assert!(matches!(
            fit_platt(&obs),
            Err(PlattError::NotEnoughObservations { .. })
        ));
    }

    #[test]
    fn fit_well_calibrated_data_returns_near_identity() {
        // If raw probs already match outcomes, fit should give a≈1, b≈0.
        // Generate symmetric data: 100 obs at 0.7 with ~70% true, etc.
        let mut obs = Vec::new();
        let levels = [(0.2, 0.2), (0.5, 0.5), (0.8, 0.8)];
        for (p, true_rate) in levels {
            for i in 0..50 {
                let y = (i as f64) < (50.0 * true_rate);
                obs.push((p, y));
            }
        }
        let params = fit_platt(&obs).unwrap();
        // Allow slack — Platt smoothing biases slightly toward 0.5 for small N.
        assert!(params.a > 0.5 && params.a < 1.5, "a = {}", params.a);
        assert!(params.b.abs() < 0.5, "b = {}", params.b);
    }

    #[test]
    fn fit_overconfident_data_shrinks_toward_05() {
        // Overconfident (but moderate): predict 0.8 but only true 55% of the time;
        // predict 0.2 but true 45% of the time. Calibrated probs should be closer
        // to 0.5 than the raw predictions are.
        //
        // Extreme probabilities (0.95 / 0.05) produce huge logits that can blow
        // up plain Newton-Raphson without line search. Logged as LOG-12 in
        // mneme/ISSUES.md; will revisit in MNEME-15 calibration loop closure.
        let mut obs = Vec::new();
        for i in 0..100 {
            let y = (i % 20) < 11; // 55% true at p=0.8
            obs.push((0.8, y));
        }
        for i in 0..100 {
            let y = (i % 20) < 9; // 45% true at p=0.2
            obs.push((0.2, y));
        }
        let params = fit_platt(&obs).unwrap();
        let calibrated_high = platt_apply(params, 0.8).unwrap();
        // Should pull toward the true rate (0.55), so well below the raw 0.8.
        assert!(
            calibrated_high < 0.7,
            "calibrated 0.8 → {} (should shrink toward true rate 0.55)",
            calibrated_high
        );
        let calibrated_low = platt_apply(params, 0.2).unwrap();
        // Symmetrically, should pull above the raw 0.2 toward 0.45.
        assert!(
            calibrated_low > 0.3,
            "calibrated 0.2 → {} (should pull up toward true rate 0.45)",
            calibrated_low
        );
    }

    #[test]
    fn fit_then_apply_round_trip() {
        let obs = vec![
            (0.1, false),
            (0.2, false),
            (0.3, false),
            (0.4, false),
            (0.5, true),
            (0.6, true),
            (0.7, true),
            (0.8, true),
            (0.9, true),
        ];
        let params = fit_platt(&obs).unwrap();
        // All applied probabilities should be in (0, 1).
        for p in [0.1, 0.5, 0.9] {
            let calibrated = platt_apply(params, p).unwrap();
            assert!(calibrated > 0.0 && calibrated < 1.0);
        }
    }
}
