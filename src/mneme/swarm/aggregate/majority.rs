//! Majority-enum — most-common discrete string value across trials.
//!
//! Ties broken lexicographically (smaller string wins), so behavior is
//! deterministic. Returned shape: `{aggregated: <winner>, votes: {value: count}}`.

use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

use super::AggregateError;

pub fn majority_enum(trials: &[Value], field: &str) -> Result<Value, AggregateError> {
    let mut votes: BTreeMap<String, u32> = BTreeMap::new();
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
        *votes.entry(s.to_string()).or_insert(0) += 1;
    }

    // Pick the winner: max count, ties broken by lexicographic order (smaller
    // wins). BTreeMap iteration is already lexicographic; we just track best.
    let winner = votes
        .iter()
        .max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.cmp(a.0)))
        .map(|(k, _)| k.clone())
        .expect("non-empty checked by aggregate()");

    let mut votes_obj = Map::new();
    for (k, v) in votes {
        votes_obj.insert(k, json!(v));
    }
    Ok(json!({"aggregated": winner, "votes": Value::Object(votes_obj)}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn clear_majority() {
        let trials = vec![
            json!({"verdict": "yes"}),
            json!({"verdict": "yes"}),
            json!({"verdict": "no"}),
        ];
        let r = majority_enum(&trials, "verdict").unwrap();
        assert_eq!(r["aggregated"], "yes");
        assert_eq!(r["votes"]["yes"], 2);
        assert_eq!(r["votes"]["no"], 1);
    }

    #[test]
    fn tie_resolved_lexicographically_smaller_wins() {
        let trials = vec![json!({"v": "b"}), json!({"v": "a"})];
        let r = majority_enum(&trials, "v").unwrap();
        // Both have count 1; "a" < "b", so "a" wins.
        assert_eq!(r["aggregated"], "a");
    }

    #[test]
    fn unanimous_returns_the_value() {
        let trials = vec![json!({"v": "x"}), json!({"v": "x"}), json!({"v": "x"})];
        let r = majority_enum(&trials, "v").unwrap();
        assert_eq!(r["aggregated"], "x");
        assert_eq!(r["votes"]["x"], 3);
    }

    #[test]
    fn missing_field_errors() {
        let trials = vec![json!({"x": "yes"}), json!({"verdict": "no"})];
        let err = majority_enum(&trials, "verdict").unwrap_err();
        match err {
            AggregateError::MissingField { trial_index, .. } => assert_eq!(trial_index, 0),
            _ => panic!(),
        }
    }
}
