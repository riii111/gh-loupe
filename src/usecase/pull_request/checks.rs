use serde_json::{Map, Value};

use crate::error::{ErrorKind, Exit, Result, RuntimeError};
use crate::github;
use crate::model::Target;
use crate::output;

const FIELDS: [&str; 7] = [
    "name",
    "state",
    "bucket",
    "link",
    "workflow",
    "startedAt",
    "completedAt",
];

pub fn execute(target: &Target, required: bool) -> Result<Value> {
    let response = github::pull_request::checks(target, required)?;
    let values = response
        .as_array()
        .ok_or_else(|| invalid_response("GitHub returned an invalid checks response"))?;
    let mut checks = values
        .iter()
        .map(validate_check)
        .collect::<Result<Vec<_>>>()?;
    checks.sort_by(|left, right| {
        string_field(left, "name")
            .cmp(string_field(right, "name"))
            .then_with(|| string_field(left, "link").cmp(string_field(right, "link")))
    });
    Ok(output::success(serde_json::json!({ "checks": checks })))
}

fn validate_check(value: &Value) -> Result<Map<String, Value>> {
    let object = value
        .as_object()
        .ok_or_else(|| invalid_response("GitHub returned an invalid check entry"))?;
    let mut check = Map::new();
    for field in FIELDS {
        let value = object.get(field).and_then(Value::as_str).ok_or_else(|| {
            invalid_response(&format!("GitHub check field {field} is missing or invalid"))
        })?;
        check.insert(field.to_owned(), Value::String(value.to_owned()));
    }
    match string_field(&check, "bucket") {
        "pass" | "fail" | "pending" | "skipping" | "cancel" => {}
        _ => return Err(invalid_response("GitHub returned an unknown check bucket")),
    }
    Ok(check)
}

fn string_field<'a>(object: &'a Map<String, Value>, field: &str) -> &'a str {
    object
        .get(field)
        .and_then(Value::as_str)
        .expect("validated check fields are strings")
}

fn invalid_response(message: &str) -> Exit {
    Exit::runtime(
        &RuntimeError {
            kind: ErrorKind::InvalidResponse,
            message: message.to_owned(),
            retryable: false,
            retry_after_seconds: None,
        },
        1,
    )
}
