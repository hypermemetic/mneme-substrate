//! Max-severity — pick the highest-severity value along an ordered ladder.
//!
//! For security-review-style aggregation: each trial contributes a severity
//! string; the result is the maximum along the configured ladder, plus the
//! count of trials that voted at that level.
//!
//! Trials whose value is not on the ladder produce an error (the caller's
//! schema should constrain the field; an off-ladder value is a contract
//! violation).

use serde_json::{json, Value};

use super::AggregateError;

pub fn max_severity(
    trials: &[Value],
    field: &str,
    ladder: &[String],
) -> Result<Value, AggregateError> {
    if ladder.is_empty() {
        return Err(AggregateError::InvalidParam(
            "severity ladder must be non-empty".to_string(),
        ));
    }

    let mut max_index: Option<usize> = None;
    let mut count_at_max: u32 = 0;

    for (i, trial) in trials.iter().enumerate() {
        let v = trial
            .get(field)
            .ok_or_else(|| AggregateError::MissingField {
                trial_index: i,
                field: field.to_string(),
            })?;
        let s = v.as_str().ok_or_else(|| AggregateError::WrongType {
            trial_index: i,
            field: field.to_string(),
            detail: format!("expected string, got {:?}", v),
        })?;
        let pos = ladder.iter().position(|l| l == s).ok_or_else(|| {
            AggregateError::WrongType {
                trial_index: i,
                field: field.to_string(),
                detail: format!("value `{}` not on ladder {:?}", s, ladder),
            }
        })?;
        match max_index {
            None => {
                max_index = Some(pos);
                count_at_max = 1;
            }
            Some(prev) if pos > prev => {
                max_index = Some(pos);
                count_at_max = 1;
            }
            Some(prev) if pos == prev => {
                count_at_max += 1;
            }
            _ => {}
        }
    }

    let winner_idx = max_index.expect("non-empty trials checked by aggregate()");
    Ok(json!({
        "aggregated": &ladder[winner_idx],
        "count": count_at_max,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ladder() -> Vec<String> {
        vec!["Low", "Medium", "High", "Critical"]
            .into_iter()
            .map(String::from)
            .collect()
    }

    #[test]
    fn picks_max_along_ladder() {
        let trials = vec![
            json!({"sev": "Low"}),
            json!({"sev": "High"}),
            json!({"sev": "Medium"}),
        ];
        let r = max_severity(&trials, "sev", &ladder()).unwrap();
        assert_eq!(r["aggregated"], "High");
        assert_eq!(r["count"], 1);
    }

    #[test]
    fn counts_trials_at_max_level() {
        let trials = vec![
            json!({"sev": "High"}),
            json!({"sev": "High"}),
            json!({"sev": "Medium"}),
        ];
        let r = max_severity(&trials, "sev", &ladder()).unwrap();
        assert_eq!(r["aggregated"], "High");
        assert_eq!(r["count"], 2);
    }

    #[test]
    fn off_ladder_value_errors() {
        let trials = vec![json!({"sev": "Spicy"})];
        let err = max_severity(&trials, "sev", &ladder()).unwrap_err();
        match err {
            AggregateError::WrongType { trial_index, .. } => assert_eq!(trial_index, 0),
            _ => panic!(),
        }
    }

    #[test]
    fn empty_ladder_errors() {
        let trials = vec![json!({"sev": "Low"})];
        let err = max_severity(&trials, "sev", &[]).unwrap_err();
        assert!(matches!(err, AggregateError::InvalidParam(_)));
    }

    #[test]
    fn missing_field_errors() {
        let trials = vec![json!({"x": "Low"})];
        let err = max_severity(&trials, "sev", &ladder()).unwrap_err();
        match err {
            AggregateError::MissingField { .. } => {}
            _ => panic!(),
        }
    }
}
