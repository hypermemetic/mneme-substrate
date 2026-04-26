//! Concat-evidence — join per-trial string summaries with attribution.
//!
//! Each trial contributes a string field; output is the strings joined with
//! a separator, prefixed `[trial i]: ` so downstream readers can attribute.

use serde_json::{json, Value};

use super::AggregateError;

/// Concatenate the named string field across trials with attribution.
pub fn concat_evidence(
    trials: &[Value],
    field: &str,
    separator: &str,
) -> Result<Value, AggregateError> {
    let mut parts = Vec::with_capacity(trials.len());
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
        parts.push(format!("[trial {}]: {}", i + 1, s));
    }
    Ok(json!({"aggregated": parts.join(separator)}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn joins_with_separator_and_attribution() {
        let trials = vec![
            json!({"summary": "first thought"}),
            json!({"summary": "second thought"}),
            json!({"summary": "third thought"}),
        ];
        let r = concat_evidence(&trials, "summary", "\n\n").unwrap();
        let s = r["aggregated"].as_str().unwrap();
        assert_eq!(
            s,
            "[trial 1]: first thought\n\n[trial 2]: second thought\n\n[trial 3]: third thought"
        );
    }

    #[test]
    fn single_trial_has_no_separator() {
        let trials = vec![json!({"summary": "only"})];
        let r = concat_evidence(&trials, "summary", "\n\n").unwrap();
        assert_eq!(r["aggregated"], "[trial 1]: only");
    }

    #[test]
    fn missing_field_errors_with_index() {
        let trials = vec![json!({"x": "ok"}), json!({"summary": "fine"})];
        let err = concat_evidence(&trials, "summary", ", ").unwrap_err();
        match err {
            AggregateError::MissingField { trial_index, field } => {
                assert_eq!(trial_index, 0);
                assert_eq!(field, "summary");
            }
            _ => panic!("wrong error variant: {:?}", err),
        }
    }

    #[test]
    fn wrong_type_errors() {
        let trials = vec![json!({"summary": 42})];
        let err = concat_evidence(&trials, "summary", ", ").unwrap_err();
        match err {
            AggregateError::WrongType { trial_index, .. } => assert_eq!(trial_index, 0),
            _ => panic!(),
        }
    }
}
