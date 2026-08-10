use serde_json::{Value, json};

use crate::error::{Exit, Result, RuntimeError};
use crate::github::{graphql, rest};
use crate::model::Target;
use crate::output;

pub fn execute(target: &Target) -> Result<Value> {
    let (pull_request, unresolved) = graphql::pull_request_overview(target)?;
    let checks = summarize_required_checks(&rest::required_check_buckets(target)?)?;
    Ok(output::success(json!({
        "pullRequest": pull_request,
        "checks": checks,
        "reviewThreads": {
            "unresolved": unresolved,
        },
    })))
}

fn summarize_required_checks(checks: &Value) -> Result<Value> {
    let checks = checks.as_array().ok_or_else(invalid_checks_response)?;
    let mut passed = 0;
    let mut pending = 0;
    let mut failed = 0;
    for check in checks {
        match check.get("bucket").and_then(Value::as_str) {
            Some("pass" | "skipping") => passed += 1,
            Some("pending") => pending += 1,
            Some("fail" | "cancel") => failed += 1,
            _ => return Err(invalid_checks_response()),
        }
    }
    Ok(json!({
        "required": checks.len(),
        "passed": passed,
        "pending": pending,
        "failed": failed,
    }))
}

fn invalid_checks_response() -> Exit {
    Exit::runtime(
        &RuntimeError::invalid_response("GitHub returned an invalid required checks response"),
        1,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn required_check_buckets_are_exclusively_aggregated() {
        let checks = json!([
            {"bucket": "pass"},
            {"bucket": "skipping"},
            {"bucket": "pending"},
            {"bucket": "fail"},
            {"bucket": "cancel"},
        ]);

        let summary =
            summarize_required_checks(&checks).unwrap_or_else(|_| panic!("valid check buckets"));

        assert_eq!(
            summary,
            json!({
                "required": 5,
                "passed": 2,
                "pending": 1,
                "failed": 2,
            })
        );
    }

    #[test]
    fn no_required_checks_produces_zero_counts() {
        let summary =
            summarize_required_checks(&json!([])).unwrap_or_else(|_| panic!("empty check list"));

        assert_eq!(
            summary,
            json!({
                "required": 0,
                "passed": 0,
                "pending": 0,
                "failed": 0,
            })
        );
    }

    #[test]
    fn unknown_bucket_is_an_invalid_response() {
        let error = summarize_required_checks(&json!([{"bucket": "new-state"}]))
            .expect_err("unknown bucket must fail");

        assert!(
            error
                .stderr_line()
                .is_some_and(|line| line.contains(r#""kind":"invalidResponse""#))
        );
    }
}
