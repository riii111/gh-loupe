use std::thread;

use serde_json::{Value, json};

use crate::error::{Exit, Result, RuntimeError};
use crate::github::{graphql, rest};
use crate::model::Target;

pub fn execute(target: &Target) -> Result<Value> {
    let (graphql_result, required_result, all_result) = thread::scope(|scope| {
        let graphql = scope.spawn(|| graphql::pull_request_overview(target));
        let required = scope.spawn(|| rest::required_check_buckets(target));
        let all = scope.spawn(|| rest::all_check_buckets(target));

        (
            graphql
                .join()
                .expect("overview GraphQL worker must not panic"),
            required
                .join()
                .expect("required checks worker must not panic"),
            all.join().expect("all checks worker must not panic"),
        )
    });
    let (pull_request, unresolved) = graphql_result?;
    let required = summarize_required_checks(&required_result?)?;
    let all = summarize_all_checks(&all_result?)?;
    Ok(json!({
        "pullRequest": pull_request,
        "checks": {
            "required": required["required"],
            "passed": required["passed"],
            "pending": required["pending"],
            "failed": required["failed"],
            "all": all,
        },
        "reviewThreads": {
            "unresolved": unresolved,
        },
    }))
}

fn summarize_required_checks(checks: &Value) -> Result<Value> {
    let checks = summarize_buckets(checks, "required")?;
    Ok(json!({
        "required": checks["total"],
        "passed": checks["passed"],
        "pending": checks["pending"],
        "failed": checks["failed"],
    }))
}

fn summarize_all_checks(checks: &Value) -> Result<Value> {
    summarize_buckets(checks, "all")
}

fn summarize_buckets(checks: &Value, kind: &str) -> Result<Value> {
    let checks = checks
        .as_array()
        .ok_or_else(|| invalid_checks_response(kind))?;
    let mut passed = 0;
    let mut pending = 0;
    let mut failed = 0;
    for check in checks {
        match check.get("bucket").and_then(Value::as_str) {
            Some("pass" | "skipping") => passed += 1,
            Some("pending") => pending += 1,
            Some("fail" | "cancel") => failed += 1,
            _ => return Err(invalid_checks_response(kind)),
        }
    }
    Ok(json!({
        "total": checks.len(),
        "passed": passed,
        "pending": pending,
        "failed": failed,
    }))
}

fn invalid_checks_response(kind: &str) -> Exit {
    Exit::runtime(
        &RuntimeError::invalid_response(format!(
            "GitHub returned an invalid {kind} checks response"
        )),
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
    fn all_check_buckets_are_aggregated_separately_from_required() {
        let summary = summarize_all_checks(&json!([
            {"bucket": "pass"},
            {"bucket": "skipping"},
            {"bucket": "pending"},
            {"bucket": "fail"},
            {"bucket": "cancel"},
        ]))
        .unwrap_or_else(|_| panic!("valid check buckets"));

        assert_eq!(
            summary,
            json!({
                "total": 5,
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
