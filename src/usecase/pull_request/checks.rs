mod actions_log;
mod diagnostics;
mod validation;

use std::cmp::Ordering;

use serde_json::{Map, Value};

use crate::error::{Exit, Result};
use crate::github;
use crate::model::{CheckDiagnosticsOptions, CheckSelection, Target};

use self::diagnostics::{collect_diagnostics, diagnostic_deadline};
use self::validation::{validate_check, validate_check_contexts};

struct Check {
    name: String,
    state: String,
    bucket: CheckBucket,
    link: Option<String>,
    workflow: Option<String>,
    started_at: Option<String>,
    completed_at: Option<String>,
    check_run_id: Option<u64>,
    annotations: Option<Vec<Annotation>>,
    log: Option<Value>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CheckBucket {
    Pass,
    Fail,
    Pending,
    Skipping,
    Cancel,
}

impl CheckBucket {
    fn from_cli(value: &str) -> Result<Self> {
        match value {
            "pass" => Ok(Self::Pass),
            "fail" => Ok(Self::Fail),
            "pending" => Ok(Self::Pending),
            "skipping" => Ok(Self::Skipping),
            "cancel" => Ok(Self::Cancel),
            _ => Err(Exit::invalid_response(
                "GitHub returned an unknown check bucket",
            )),
        }
    }

    fn from_state(state: &str) -> Result<Self> {
        match state {
            "SUCCESS" => Ok(Self::Pass),
            "SKIPPED" | "NEUTRAL" => Ok(Self::Skipping),
            "ERROR" | "FAILURE" | "TIMED_OUT" | "ACTION_REQUIRED" => Ok(Self::Fail),
            "CANCELLED" => Ok(Self::Cancel),
            "EXPECTED" | "REQUESTED" | "WAITING" | "QUEUED" | "PENDING" | "IN_PROGRESS"
            | "STALE" | "STARTUP_FAILURE" => Ok(Self::Pending),
            _ => Err(Exit::invalid_response(
                "GitHub returned an unknown check state",
            )),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Fail => "fail",
            Self::Pending => "pending",
            Self::Skipping => "skipping",
            Self::Cancel => "cancel",
        }
    }
}

#[derive(Debug)]
struct Annotation {
    path: String,
    start_line: u64,
    end_line: u64,
    annotation_level: String,
    title: Option<String>,
    message: String,
}

impl Annotation {
    fn into_value(self) -> Value {
        Value::Object(Map::from_iter([
            ("path".to_owned(), Value::String(self.path)),
            ("startLine".to_owned(), Value::from(self.start_line)),
            ("endLine".to_owned(), Value::from(self.end_line)),
            (
                "annotationLevel".to_owned(),
                Value::String(self.annotation_level),
            ),
            (
                "title".to_owned(),
                self.title.map_or(Value::Null, Value::String),
            ),
            ("message".to_owned(), Value::String(self.message)),
        ]))
    }
}

impl Check {
    fn into_value(self) -> Value {
        let mut value = Map::from_iter([
            ("name".to_owned(), Value::String(self.name)),
            ("state".to_owned(), Value::String(self.state)),
            (
                "bucket".to_owned(),
                Value::String(self.bucket.as_str().to_owned()),
            ),
            (
                "link".to_owned(),
                self.link.map_or(Value::Null, Value::String),
            ),
            (
                "workflow".to_owned(),
                self.workflow.map_or(Value::Null, Value::String),
            ),
            (
                "startedAt".to_owned(),
                self.started_at.map_or(Value::Null, Value::String),
            ),
            (
                "completedAt".to_owned(),
                self.completed_at.map_or(Value::Null, Value::String),
            ),
        ]);
        if let Some(annotations) = self.annotations {
            value.insert(
                "annotations".to_owned(),
                Value::Array(
                    annotations
                        .into_iter()
                        .map(Annotation::into_value)
                        .collect(),
                ),
            );
        }
        if let Some(log) = self.log {
            value.insert("log".to_owned(), log);
        }
        Value::Object(value)
    }
}

fn compare_checks(left: &Check, right: &Check) -> Ordering {
    left.name
        .cmp(&right.name)
        .then_with(|| left.link.cmp(&right.link))
        .then_with(|| right.started_at.cmp(&left.started_at))
        .then_with(|| left.check_run_id.cmp(&right.check_run_id))
}

pub fn execute(target: &Target, required: bool, options: CheckDiagnosticsOptions) -> Result<Value> {
    let diagnostics_requested = options.failed_diagnostics || options.include_failed_logs;
    let timeout_message = format!(
        "failed check diagnostics timed out after {} seconds",
        options.timeout_seconds
    );
    let deadline = diagnostic_deadline(options.timeout_seconds).ok_or_else(|| Exit {
        message: format!(
            "argument --timeout: {} seconds cannot be represented as a diagnostic deadline",
            options.timeout_seconds
        ),
        code: 2,
    })?;
    let mut checks = if diagnostics_requested {
        let (head_oid, contexts) =
            github::graphql::pull_request_check_contexts(target, deadline, &timeout_message)?;
        let mut checks = validate_check_contexts(&contexts, required)?;
        checks.sort_by(compare_checks);
        collect_diagnostics(
            target,
            &mut checks,
            options,
            &head_oid,
            deadline,
            &timeout_message,
        )?;
        checks
    } else {
        let response = github::checks::checks(target, required)?;
        let values = response
            .as_array()
            .ok_or_else(|| Exit::invalid_response("GitHub returned an invalid checks response"))?;
        let mut checks = values
            .iter()
            .map(validate_check)
            .collect::<Result<Vec<_>>>()?;
        checks.sort_by(compare_checks);
        checks
    };

    let failed_only = matches!(options.selection, CheckSelection::FailedOnly);
    let summary = failed_only.then(|| summarize_checks(&checks));
    if failed_only {
        checks.retain(|check| matches!(check.bucket, CheckBucket::Fail | CheckBucket::Cancel));
    }

    let mut result = Map::new();
    if let Some(summary) = summary {
        result.insert("summary".to_owned(), summary);
    }
    result.insert(
        "checks".to_owned(),
        Value::Array(checks.into_iter().map(Check::into_value).collect()),
    );
    Ok(Value::Object(result))
}

fn summarize_checks(checks: &[Check]) -> Value {
    let mut passed = 0;
    let mut pending = 0;
    let mut failed = 0;
    for check in checks {
        match check.bucket {
            CheckBucket::Pass | CheckBucket::Skipping => passed += 1,
            CheckBucket::Pending => pending += 1,
            CheckBucket::Fail | CheckBucket::Cancel => failed += 1,
        }
    }
    Value::Object(Map::from_iter([
        ("total".to_owned(), Value::from(checks.len())),
        ("passed".to_owned(), Value::from(passed)),
        ("pending".to_owned(), Value::from(pending)),
        ("failed".to_owned(), Value::from(failed)),
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_bucket_maps_all_known_states() {
        assert_eq!(bucket("SUCCESS"), CheckBucket::Pass);
        assert_eq!(bucket("SKIPPED"), CheckBucket::Skipping);
        assert_eq!(bucket("NEUTRAL"), CheckBucket::Skipping);
        assert_eq!(bucket("ERROR"), CheckBucket::Fail);
        assert_eq!(bucket("FAILURE"), CheckBucket::Fail);
        assert_eq!(bucket("TIMED_OUT"), CheckBucket::Fail);
        assert_eq!(bucket("ACTION_REQUIRED"), CheckBucket::Fail);
        assert_eq!(bucket("CANCELLED"), CheckBucket::Cancel);
        assert_eq!(bucket("EXPECTED"), CheckBucket::Pending);
        assert_eq!(bucket("REQUESTED"), CheckBucket::Pending);
        assert_eq!(bucket("WAITING"), CheckBucket::Pending);
        assert_eq!(bucket("QUEUED"), CheckBucket::Pending);
        assert_eq!(bucket("PENDING"), CheckBucket::Pending);
        assert_eq!(bucket("IN_PROGRESS"), CheckBucket::Pending);
        assert_eq!(bucket("STALE"), CheckBucket::Pending);
        assert_eq!(bucket("STARTUP_FAILURE"), CheckBucket::Pending);
        assert!(CheckBucket::from_state("UNKNOWN").is_err());
    }

    #[test]
    fn failed_only_summary_counts_skipping_as_passed_and_cancel_as_failed() {
        let checks = vec![
            check_with_bucket(CheckBucket::Pass),
            check_with_bucket(CheckBucket::Skipping),
            check_with_bucket(CheckBucket::Pending),
            check_with_bucket(CheckBucket::Fail),
            check_with_bucket(CheckBucket::Cancel),
        ];

        assert_eq!(
            summarize_checks(&checks),
            serde_json::json!({
                "total": 5,
                "passed": 2,
                "pending": 1,
                "failed": 2,
            })
        );
    }

    fn check_with_bucket(bucket: CheckBucket) -> Check {
        Check {
            name: "check".to_owned(),
            state: "SUCCESS".to_owned(),
            bucket,
            link: None,
            workflow: None,
            started_at: None,
            completed_at: None,
            check_run_id: None,
            annotations: None,
            log: None,
        }
    }

    fn bucket(state: &str) -> CheckBucket {
        CheckBucket::from_state(state).unwrap_or_else(|_| panic!("known check state: {state}"))
    }
}
