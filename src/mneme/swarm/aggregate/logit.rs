//! Logit-space Bayesian shrinkage — the BLF aggregation rule for probabilities.
//!
//! Given N trial probabilities `p_i ∈ (0, 1)` and a prior `p_0 ∈ (0, 1)`:
//!
//! 1. Convert each to log-odds: `l_i = log(p_i / (1 - p_i))`.
//! 2. Compute the trial mean in logit space: `l̄ = (1/N) Σ l_i`.
//! 3. Shrink toward the prior: `l_aggregated = (1 - λ) · l̄ + λ · l_0`.
//! 4. Convert back to probability: `p_aggregated = sigmoid(l_aggregated)`.
//!
//! `λ = 0` ⇒ pure trial mean; `λ = 1` ⇒ pure prior.
//!
//! Inputs are clamped to `[1e-6, 1 − 1e-6]` to avoid logit overflow at the
//! endpoints. Matches the spirit of MNEME-5's `LogitShrinkage` ticket.

use serde_json::{json, Value};

use super::AggregateError;

const CLAMP_EPS: f64 = 1e-6;

fn clamp(p: f64) -> f64 {
    p.max(CLAMP_EPS).min(1.0 - CLAMP_EPS)
}

fn logit(p: f64) -> f64 {
    let p = clamp(p);
    (p / (1.0 - p)).ln()
}

fn sigmoid(l: f64) -> f64 {
    1.0 / (1.0 + (-l).exp())
}

/// Apply logit-space shrinkage. Returns
/// `{aggregated: f64, raw_mean: f64, n: usize}`.
pub fn logit_shrinkage(
    trials: &[Value],
    field: &str,
    prior: f64,
    lambda: f64,
) -> Result<Value, AggregateError> {
    if !(0.0..=1.0).contains(&lambda) {
        return Err(AggregateError::InvalidParam(format!(
            "lambda must be in [0, 1], got {}",
            lambda
        )));
    }
    if !(0.0..=1.0).contains(&prior) {
        return Err(AggregateError::InvalidParam(format!(
            "prior must be in [0, 1], got {}",
            prior
        )));
    }

    let mut logits = Vec::with_capacity(trials.len());
    let mut raw_sum = 0.0;
    for (i, trial) in trials.iter().enumerate() {
        let v = trial
            .get(field)
            .ok_or_else(|| AggregateError::MissingField {
                trial_index: i,
                field: field.to_string(),
            })?;
        let p = v
            .as_f64()
            .ok_or_else(|| AggregateError::WrongType {
                trial_index: i,
                field: field.to_string(),
                detail: format!("expected number, got {:?}", v),
            })?;
        if !p.is_finite() {
            return Err(AggregateError::WrongType {
                trial_index: i,
                field: field.to_string(),
                detail: "non-finite probability".to_string(),
            });
        }
        logits.push(logit(p));
        raw_sum += clamp(p);
    }

    let n = logits.len() as f64;
    let trial_mean_logit = logits.iter().sum::<f64>() / n;
    let prior_logit = logit(prior);
    let agg_logit = (1.0 - lambda) * trial_mean_logit + lambda * prior_logit;
    let aggregated = sigmoid(agg_logit);
    let raw_mean = raw_sum / n;

    Ok(json!({
        "aggregated": aggregated,
        "raw_mean": raw_mean,
        "n": trials.len(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f64, b: f64, eps: f64) {
        assert!((a - b).abs() < eps, "{} vs {} (eps {})", a, b, eps);
    }

    #[test]
    fn no_prior_pull_returns_trial_mean() {
        // λ=0 → aggregated == sigmoid(mean(logit(p_i))).
        // For all-equal trials, that equals p itself.
        let trials = vec![json!({"p": 0.7}), json!({"p": 0.7}), json!({"p": 0.7})];
        let r = logit_shrinkage(&trials, "p", 0.3, 0.0).unwrap();
        approx(r["aggregated"].as_f64().unwrap(), 0.7, 1e-9);
        approx(r["raw_mean"].as_f64().unwrap(), 0.7, 1e-9);
        assert_eq!(r["n"].as_u64().unwrap(), 3);
    }

    #[test]
    fn full_prior_pull_returns_prior() {
        let trials = vec![json!({"p": 0.9}), json!({"p": 0.95}), json!({"p": 0.99})];
        let r = logit_shrinkage(&trials, "p", 0.3, 1.0).unwrap();
        approx(r["aggregated"].as_f64().unwrap(), 0.3, 1e-9);
    }

    #[test]
    fn partial_pull_is_between() {
        let trials = vec![json!({"p": 0.8}), json!({"p": 0.8}), json!({"p": 0.8})];
        let r = logit_shrinkage(&trials, "p", 0.5, 0.5).unwrap();
        let agg = r["aggregated"].as_f64().unwrap();
        // λ=0.5: average of logit(0.8) and logit(0.5) in logit space.
        // logit(0.8) ≈ 1.3863, logit(0.5) = 0. Average ≈ 0.6931. sigmoid(0.6931) ≈ 0.6667.
        approx(agg, 0.6666666666, 1e-6);
    }

    #[test]
    fn endpoint_inputs_clamped_no_overflow() {
        let trials = vec![json!({"p": 0.0}), json!({"p": 1.0})];
        let r = logit_shrinkage(&trials, "p", 0.5, 0.0).unwrap();
        let agg = r["aggregated"].as_f64().unwrap();
        // clamp(0)=eps, clamp(1)=1-eps; logit symmetric so mean logit = 0; sigmoid(0)=0.5.
        approx(agg, 0.5, 1e-9);
    }

    #[test]
    fn missing_field_errors_with_index() {
        let trials = vec![json!({"p": 0.5}), json!({"x": 0.5})];
        let err = logit_shrinkage(&trials, "p", 0.5, 0.0).unwrap_err();
        match err {
            AggregateError::MissingField { trial_index, field } => {
                assert_eq!(trial_index, 1);
                assert_eq!(field, "p");
            }
            _ => panic!("wrong error variant: {:?}", err),
        }
    }

    #[test]
    fn wrong_type_errors_with_index() {
        let trials = vec![json!({"p": "not a number"})];
        let err = logit_shrinkage(&trials, "p", 0.5, 0.0).unwrap_err();
        match err {
            AggregateError::WrongType { trial_index, .. } => assert_eq!(trial_index, 0),
            _ => panic!("wrong error variant: {:?}", err),
        }
    }

    #[test]
    fn lambda_out_of_range_errors() {
        let trials = vec![json!({"p": 0.5})];
        assert!(logit_shrinkage(&trials, "p", 0.5, -0.1).is_err());
        assert!(logit_shrinkage(&trials, "p", 0.5, 1.1).is_err());
    }

    #[test]
    fn prior_out_of_range_errors() {
        let trials = vec![json!({"p": 0.5})];
        assert!(logit_shrinkage(&trials, "p", -0.1, 0.5).is_err());
        assert!(logit_shrinkage(&trials, "p", 1.1, 0.5).is_err());
    }

    #[test]
    fn nan_input_errors() {
        let trials = vec![json!({"p": f64::NAN})];
        let err = logit_shrinkage(&trials, "p", 0.5, 0.0).unwrap_err();
        // serde_json represents NaN as null; that fails as_f64 first → WrongType.
        // If a number type that's NaN somehow makes it through, we catch via is_finite.
        match err {
            AggregateError::WrongType { .. } => {}
            other => panic!("unexpected: {:?}", other),
        }
    }

    #[test]
    fn raw_mean_uses_clamped_inputs() {
        // raw_mean averages the (clamped) trial probabilities, not the trial mean
        // in probability space directly. Confirming the contract: it's a sanity
        // metric, not an aggregation result.
        let trials = vec![json!({"p": 0.2}), json!({"p": 0.8})];
        let r = logit_shrinkage(&trials, "p", 0.5, 0.0).unwrap();
        approx(r["raw_mean"].as_f64().unwrap(), 0.5, 1e-9);
    }
}
