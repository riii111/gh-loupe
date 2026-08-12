use serde_json::Value;

use crate::error::{Exit, Result};

pub mod checks;
pub mod comments;
pub mod overview;
pub mod review_thread;
pub mod review_threads;
pub mod reviews;

pub(super) fn required_field<'a>(value: &'a Value, field: &str) -> Result<&'a Value> {
    value
        .get(field)
        .ok_or_else(|| Exit::invalid_response(format!("GitHub response omitted {field}")))
}

pub(super) fn string_field<'a>(value: &'a Value, field: &str) -> Result<&'a str> {
    value
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| Exit::invalid_response(format!("GitHub field {field} must be a string")))
}

pub(super) fn bool_field(value: &Value, field: &str) -> Result<bool> {
    value
        .get(field)
        .and_then(Value::as_bool)
        .ok_or_else(|| Exit::invalid_response(format!("GitHub field {field} must be a boolean")))
}

pub(super) fn nullable_location(value: &Value, field: &str) -> Result<Value> {
    let value = required_field(value, field)?;
    let valid = match field {
        "path" | "diffSide" => value.is_null() || value.is_string(),
        "line" | "originalLine" | "startLine" => value.is_null() || value.as_i64().is_some(),
        _ => false,
    };
    if !valid {
        return Err(Exit::invalid_response(format!(
            "GitHub field {field} has an invalid value"
        )));
    }
    Ok(value.clone())
}

pub(super) fn string_value<'a>(value: &'a Value, field: &str) -> &'a str {
    value[field]
        .as_str()
        .expect("projected comment string was validated")
}
