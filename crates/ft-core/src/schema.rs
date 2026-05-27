//! JSON Schema generation and runtime validation.
//!
//! The Rust types are the source of truth; the schema is derived from them
//! via `schemars`. Validation runs through the `jsonschema` crate. Exporting
//! the schema as a JSON document (for external PR-review bots or importers)
//! is the responsibility of a downstream tool that calls
//! [`record_schema_json`].

use jsonschema::Validator;
use schemars::schema_for;
use serde_json::Value;

use crate::error::CoreError;
use crate::record::Record;

/// Generate the JSON Schema for [`Record`] as a `serde_json::Value`.
#[must_use]
pub fn record_schema() -> Value {
    let schema = schema_for!(Record);
    serde_json::to_value(schema).expect("schemars output always converts to Value")
}

/// Generate the JSON Schema as a pretty-printed JSON string.
#[must_use]
pub fn record_schema_json() -> String {
    serde_json::to_string_pretty(&record_schema()).expect("schema value always serializes")
}

/// Validate an arbitrary JSON value against the [`Record`] schema.
///
/// # Errors
///
/// Returns [`CoreError::SchemaValidation`] with a comma-separated list of
/// validator errors if validation fails.
pub fn validate_record_json(value: &Value) -> Result<(), CoreError> {
    let schema = record_schema();
    let validator = Validator::new(&schema)
        .map_err(|e| CoreError::SchemaValidation(format!("invalid schema: {e}")))?;
    let errors: Vec<String> = validator
        .iter_errors(value)
        .map(|e| format!("{e} at {}", e.instance_path))
        .collect();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(CoreError::SchemaValidation(errors.join("; ")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builder::RecordBuilder;
    use crate::id::RecordKind;
    use crate::identity::Identity;

    fn alice() -> Identity {
        Identity::new("alice@example.com").unwrap()
    }

    #[test]
    fn schema_generates_non_empty() {
        let s = record_schema();
        assert!(s.is_object(), "schema must be an object");
    }

    #[test]
    fn schema_json_is_pretty() {
        let s = record_schema_json();
        assert!(s.contains('\n'));
    }

    #[test]
    fn valid_record_passes_validation() {
        let r = RecordBuilder::new(RecordKind::Task, "demo", alice())
            .build()
            .unwrap();
        let v = serde_json::to_value(&r).unwrap();
        validate_record_json(&v).expect("valid record must validate");
    }

    #[test]
    fn malformed_record_is_rejected() {
        // Missing required fields entirely.
        let bad = serde_json::json!({"foo": "bar"});
        assert!(validate_record_json(&bad).is_err());
    }

    #[test]
    fn wrong_type_field_is_rejected() {
        let r = RecordBuilder::new(RecordKind::Task, "demo", alice())
            .build()
            .unwrap();
        let mut v = serde_json::to_value(&r).unwrap();
        // Replace title with a number.
        v.get_mut("envelope")
            .and_then(|e| e.as_object_mut())
            .unwrap()
            .insert("title".into(), serde_json::json!(42));
        assert!(validate_record_json(&v).is_err());
    }
}
