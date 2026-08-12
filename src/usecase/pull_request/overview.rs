use std::thread;

use serde_json::{Map, Value};

use crate::error::{Exit, Result};
use crate::github::{checks, graphql};
use crate::model::Target;

pub fn execute(target: &Target) -> Result<Value> {
    let (graphql_result, required_result, all_result) = thread::scope(|scope| {
        let graphql = scope.spawn(|| graphql::pull_request_overview(target));
        let required = scope.spawn(|| checks::required_check_buckets(target));
        let all = scope.spawn(|| checks::all_check_buckets(target));

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
    let mut checks = Map::new();
    checks.insert("required".to_owned(), required["required"].clone());
    checks.insert("passed".to_owned(), required["passed"].clone());
    checks.insert("pending".to_owned(), required["pending"].clone());
    checks.insert("failed".to_owned(), required["failed"].clone());
    checks.insert("all".to_owned(), all);

    let mut review_threads = Map::new();
    review_threads.insert("unresolved".to_owned(), Value::from(unresolved));

    let mut result = Map::new();
    result.insert("pullRequest".to_owned(), pull_request);
    result.insert("checks".to_owned(), Value::Object(checks));
    result.insert("reviewThreads".to_owned(), Value::Object(review_threads));
    Ok(Value::Object(result))
}

fn summarize_required_checks(checks: &Value) -> Result<Value> {
    let checks = summarize_buckets(checks, "required")?;
    let mut result = Map::new();
    result.insert("required".to_owned(), checks["total"].clone());
    result.insert("passed".to_owned(), checks["passed"].clone());
    result.insert("pending".to_owned(), checks["pending"].clone());
    result.insert("failed".to_owned(), checks["failed"].clone());
    Ok(Value::Object(result))
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
    let mut result = Map::new();
    result.insert("total".to_owned(), Value::from(checks.len()));
    result.insert("passed".to_owned(), Value::from(passed));
    result.insert("pending".to_owned(), Value::from(pending));
    result.insert("failed".to_owned(), Value::from(failed));
    Ok(Value::Object(result))
}

fn invalid_checks_response(kind: &str) -> Exit {
    Exit::invalid_response(format!("GitHub returned an invalid {kind} checks response"))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

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

        assert!(error.stderr_line().contains(r#""kind":"invalidResponse""#));
    }
}
