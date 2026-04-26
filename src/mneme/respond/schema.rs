//! JSON Schema validation for `respond` payloads.
//!
//! Phase 1 ships a deliberately minimal validator: type, required-fields,
//! integer min/max. This is enough for forecast and the other phase-2 skills.
//! The full JSON Schema spec is too big to implement by hand and the existing
//! plexus toolchain doesn't currently expose a validator we can reuse.
//!
//! Phase 3 will swap this for `jsonschema` crate or similar; the API here is
//! built to be drop-in-replaceable.

use serde_json::Value;

/// Errors raised by [`validate`].
#[derive(Debug, thiserror::Error)]
pub enum SchemaError {
    #[error("payload is not an object (schema requires `type: object`)")]
    NotObject,
    #[error("required field `{0}` is missing")]
    MissingRequired(String),
    #[error("field `{field}`: expected {expected}, got {actual}")]
    WrongType {
        field: String,
        expected: String,
        actual: String,
    },
    #[error("field `{field}`: value {value} out of range [{min}, {max}]")]
    OutOfRange {
        field: String,
        value: f64,
        min: f64,
        max: f64,
    },
    #[error("schema is malformed: {0}")]
    BadSchema(String),
}

/// Validate `payload` against `schema`. Returns Ok on success.
pub fn validate(payload: &Value, schema: &Value) -> Result<(), SchemaError> {
    let schema_obj = schema
        .as_object()
        .ok_or_else(|| SchemaError::BadSchema("schema must be an object".to_string()))?;

    let schema_type = schema_obj
        .get("type")
        .and_then(|v| v.as_str())
        .ok_or_else(|| SchemaError::BadSchema("schema must declare `type`".to_string()))?;

    if schema_type != "object" {
        return Err(SchemaError::BadSchema(format!(
            "Phase 1 only validates `type: object` schemas; got `{}`",
            schema_type
        )));
    }

    let payload_obj = payload.as_object().ok_or(SchemaError::NotObject)?;

    // Required fields.
    if let Some(required) = schema_obj.get("required").and_then(|v| v.as_array()) {
        for r in required {
            let name = r.as_str().ok_or_else(|| {
                SchemaError::BadSchema("required entries must be strings".to_string())
            })?;
            if !payload_obj.contains_key(name) {
                return Err(SchemaError::MissingRequired(name.to_string()));
            }
        }
    }

    // Per-property type / range checks.
    if let Some(props) = schema_obj.get("properties").and_then(|v| v.as_object()) {
        for (field, prop_schema) in props {
            let Some(value) = payload_obj.get(field) else {
                continue; // not required (already checked); ok if absent
            };
            validate_property(field, value, prop_schema)?;
        }
    }

    Ok(())
}

fn validate_property(field: &str, value: &Value, prop_schema: &Value) -> Result<(), SchemaError> {
    let ps = prop_schema
        .as_object()
        .ok_or_else(|| SchemaError::BadSchema(format!("property `{}` schema not an object", field)))?;

    let Some(ty) = ps.get("type").and_then(|v| v.as_str()) else {
        return Ok(()); // untyped property; skip
    };

    let actual_type = json_type_name(value);
    let type_ok = match ty {
        "string" => value.is_string(),
        "integer" => value.is_i64() || value.is_u64(),
        "number" => value.is_number(),
        "boolean" => value.is_boolean(),
        "object" => value.is_object(),
        "array" => value.is_array(),
        "null" => value.is_null(),
        _ => return Err(SchemaError::BadSchema(format!("unknown type `{}`", ty))),
    };
    if !type_ok {
        return Err(SchemaError::WrongType {
            field: field.to_string(),
            expected: ty.to_string(),
            actual: actual_type.to_string(),
        });
    }

    // Range checks for numeric types.
    if ty == "integer" || ty == "number" {
        let v = value.as_f64().expect("type-checked above");
        let min = ps
            .get("minimum")
            .and_then(|x| x.as_f64())
            .unwrap_or(f64::NEG_INFINITY);
        let max = ps
            .get("maximum")
            .and_then(|x| x.as_f64())
            .unwrap_or(f64::INFINITY);
        if v < min || v > max {
            return Err(SchemaError::OutOfRange {
                field: field.to_string(),
                value: v,
                min,
                max,
            });
        }
    }

    Ok(())
}

fn json_type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(n) => {
            if n.is_i64() || n.is_u64() {
                "integer"
            } else {
                "number"
            }
        }
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn forecast_schema() -> Value {
        json!({
            "type": "object",
            "properties": {
                "probability": {"type": "number", "minimum": 0.0, "maximum": 1.0},
                "summary": {"type": "string"}
            },
            "required": ["probability", "summary"]
        })
    }

    #[test]
    fn valid_payload_passes() {
        let schema = forecast_schema();
        let payload = json!({"probability": 0.42, "summary": "..."});
        validate(&payload, &schema).unwrap();
    }

    #[test]
    fn missing_required_field_fails() {
        let schema = forecast_schema();
        let payload = json!({"probability": 0.5});
        let err = validate(&payload, &schema).unwrap_err();
        match err {
            SchemaError::MissingRequired(field) => assert_eq!(field, "summary"),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn wrong_type_fails() {
        let schema = forecast_schema();
        let payload = json!({"probability": "not a number", "summary": "ok"});
        let err = validate(&payload, &schema).unwrap_err();
        match err {
            SchemaError::WrongType { field, expected, .. } => {
                assert_eq!(field, "probability");
                assert_eq!(expected, "number");
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn out_of_range_fails() {
        let schema = forecast_schema();
        let payload = json!({"probability": 1.5, "summary": "..."});
        let err = validate(&payload, &schema).unwrap_err();
        match err {
            SchemaError::OutOfRange { field, value, max, .. } => {
                assert_eq!(field, "probability");
                assert_eq!(value, 1.5);
                assert_eq!(max, 1.0);
            }
            _ => panic!(),
        }
    }

    #[test]
    fn integer_type_accepts_i64() {
        let schema = json!({
            "type": "object",
            "properties": {"v": {"type": "integer", "minimum": 0, "maximum": 10}},
            "required": ["v"]
        });
        validate(&json!({"v": 7}), &schema).unwrap();
    }

    #[test]
    fn integer_type_rejects_float() {
        let schema = json!({
            "type": "object",
            "properties": {"v": {"type": "integer"}},
            "required": ["v"]
        });
        let err = validate(&json!({"v": 7.5}), &schema).unwrap_err();
        assert!(matches!(err, SchemaError::WrongType { .. }));
    }

    #[test]
    fn non_object_payload_fails() {
        let schema = forecast_schema();
        let err = validate(&json!("just a string"), &schema).unwrap_err();
        assert!(matches!(err, SchemaError::NotObject));
    }

    #[test]
    fn bad_schema_errors() {
        let payload = json!({});
        let err = validate(&payload, &json!("not an object")).unwrap_err();
        assert!(matches!(err, SchemaError::BadSchema(_)));
    }

    #[test]
    fn untyped_property_passes() {
        let schema = json!({
            "type": "object",
            "properties": {"x": {}},
            "required": ["x"]
        });
        validate(&json!({"x": 42}), &schema).unwrap();
        validate(&json!({"x": "anything"}), &schema).unwrap();
    }

    #[test]
    fn extra_properties_allowed() {
        let schema = forecast_schema();
        let payload = json!({"probability": 0.5, "summary": "ok", "extra": "fine"});
        validate(&payload, &schema).unwrap();
    }
}
