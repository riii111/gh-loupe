use serde_json::{Map, Value};

use crate::error::{Exit, Result};

use super::output::{Annotation, Check, CheckBucket};

const ZERO_TIME: &str = "0001-01-01T00:00:00Z";

pub(super) fn validate_check(value: &Value) -> Result<Check> {
    let object = value
        .as_object()
        .ok_or_else(|| Exit::invalid_response("GitHub returned an invalid check entry"))?;
    let bucket = CheckBucket::from_cli(required_cli_check_string(object, "bucket")?)?;
    Ok(Check {
        name: required_cli_check_string(object, "name")?.to_owned(),
        state: required_cli_check_string(object, "state")?.to_owned(),
        bucket,
        link: cli_check_metadata(object, "link")?,
        workflow: cli_check_metadata(object, "workflow")?,
        started_at: cli_check_metadata(object, "startedAt")?,
        completed_at: cli_check_metadata(object, "completedAt")?,
        check_run_id: None,
        annotations: None,
        log: None,
    })
}

pub(super) fn validate_check_contexts(values: &[Value], required: bool) -> Result<Vec<Check>> {
    let mut checks = Vec::new();

    for value in values {
        let object = value
            .as_object()
            .ok_or_else(|| Exit::invalid_response("GitHub returned an invalid check context"))?;
        let is_required = object
            .get("isRequired")
            .and_then(Value::as_bool)
            .ok_or_else(|| {
                Exit::invalid_response("GitHub returned an invalid required check marker")
            })?;
        if required && !is_required {
            continue;
        }
        let type_name = required_string(object, "__typename", "check context type")?;
        let details = match type_name {
            "CheckRun" => {
                let check_run_id = required_u64(object, "databaseId", "check run identifier")?;
                let name = required_string(object, "name", "check run name")?;
                let status = required_string(object, "status", "check run status")?;
                let conclusion = nullable_string(object, "conclusion", "check run conclusion")?;
                let state = graphql_check_run_state(status, conclusion)?;
                let link = nullable_string(object, "detailsUrl", "check run details URL")?;
                let started_at = nullable_string(object, "startedAt", "check run start time")?;
                let completed_at =
                    nullable_string(object, "completedAt", "check run completion time")?;
                let workflow = check_run_workflow(object)?;
                CheckDetails {
                    name,
                    state,
                    link,
                    workflow,
                    started_at,
                    completed_at,
                    check_run_id: Some(check_run_id),
                }
            }
            "StatusContext" => {
                let name = required_string(object, "context", "commit status context")?;
                let state = required_string(object, "state", "commit status state")?.to_owned();
                let link = nullable_string(object, "targetUrl", "commit status target URL")?;
                CheckDetails {
                    name,
                    state,
                    link,
                    workflow: None,
                    started_at: None,
                    completed_at: None,
                    check_run_id: None,
                }
            }
            _ => {
                return Err(Exit::invalid_response(
                    "GitHub returned an unknown check context type",
                ));
            }
        };
        let bucket = CheckBucket::from_state(&details.state)?;
        checks.push(Check {
            name: details.name.to_owned(),
            state: details.state,
            bucket,
            link: details.link.map(str::to_owned),
            workflow: details.workflow.map(str::to_owned),
            started_at: details.started_at.map(str::to_owned),
            completed_at: details.completed_at.map(str::to_owned),
            check_run_id: details.check_run_id,
            annotations: None,
            log: None,
        });
    }
    Ok(checks)
}

struct CheckDetails<'a> {
    name: &'a str,
    state: String,
    link: Option<&'a str>,
    workflow: Option<&'a str>,
    started_at: Option<&'a str>,
    completed_at: Option<&'a str>,
    check_run_id: Option<u64>,
}

fn graphql_check_run_state(status: &str, conclusion: Option<&str>) -> Result<String> {
    let conclusion = match conclusion {
        None => None,
        Some(value) if is_known_check_run_conclusion(value) => Some(value),
        Some(_) => {
            return Err(Exit::invalid_response(
                "GitHub returned an unknown check run conclusion",
            ));
        }
    };
    if status == "COMPLETED" {
        return conclusion.map(str::to_owned).ok_or_else(|| {
            Exit::invalid_response("GitHub returned a completed check run without a conclusion")
        });
    }
    if conclusion.is_some() {
        return Err(Exit::invalid_response(
            "GitHub returned a non-completed check run with a conclusion",
        ));
    }
    match status {
        "IN_PROGRESS" | "PENDING" | "QUEUED" | "REQUESTED" | "WAITING" => Ok(status.to_owned()),
        _ => Err(Exit::invalid_response(
            "GitHub returned an unknown check run status",
        )),
    }
}

fn is_known_check_run_conclusion(conclusion: &str) -> bool {
    matches!(
        conclusion,
        "ACTION_REQUIRED"
            | "CANCELLED"
            | "FAILURE"
            | "NEUTRAL"
            | "SKIPPED"
            | "STALE"
            | "STARTUP_FAILURE"
            | "SUCCESS"
            | "TIMED_OUT"
    )
}

fn check_run_workflow(object: &Map<String, Value>) -> Result<Option<&str>> {
    let suite = object
        .get("checkSuite")
        .and_then(Value::as_object)
        .ok_or_else(|| Exit::invalid_response("GitHub returned an invalid check suite"))?;
    let Some(run) = suite.get("workflowRun") else {
        return Err(Exit::invalid_response(
            "GitHub returned an invalid workflow run",
        ));
    };
    if run.is_null() {
        return Ok(None);
    }
    let run = run
        .as_object()
        .ok_or_else(|| Exit::invalid_response("GitHub returned an invalid workflow run"))?;
    let workflow = run
        .get("workflow")
        .and_then(Value::as_object)
        .ok_or_else(|| Exit::invalid_response("GitHub returned an invalid workflow"))?;
    Ok(Some(required_string(workflow, "name", "workflow name")?))
}

pub(super) fn validate_annotations(response: &Value) -> Result<Vec<Annotation>> {
    let pages = response
        .as_array()
        .ok_or_else(|| Exit::invalid_response("GitHub returned an invalid annotation response"))?;
    let mut annotations = Vec::new();
    for page in pages {
        let values = page
            .as_array()
            .ok_or_else(|| Exit::invalid_response("GitHub returned an invalid annotation page"))?;
        for value in values {
            let object = value
                .as_object()
                .ok_or_else(|| Exit::invalid_response("GitHub returned an invalid annotation"))?;
            annotations.push(annotation_from_object(object)?);
        }
    }
    annotations.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.start_line.cmp(&right.start_line))
            .then_with(|| left.end_line.cmp(&right.end_line))
            .then_with(|| left.message.cmp(&right.message))
            .then_with(|| left.title.cmp(&right.title))
    });
    Ok(annotations)
}

fn annotation_from_object(object: &Map<String, Value>) -> Result<Annotation> {
    let path = required_string(object, "path", "annotation path")?.to_owned();
    let start_line = required_u64(object, "start_line", "annotation start line")?;
    let end_line = required_u64(object, "end_line", "annotation end line")?;
    let annotation_level =
        required_string(object, "annotation_level", "annotation level")?.to_owned();
    let message = required_string(object, "message", "annotation message")?.to_owned();
    let title = nullable_string(object, "title", "annotation title")?.map(str::to_owned);
    Ok(Annotation {
        path,
        start_line,
        end_line,
        annotation_level,
        title,
        message,
    })
}

pub(super) fn required_string<'a>(
    object: &'a Map<String, Value>,
    field: &str,
    label: &str,
) -> Result<&'a str> {
    object
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| Exit::invalid_response(format!("GitHub returned an invalid {label}")))
}

fn required_cli_check_string<'a>(object: &'a Map<String, Value>, field: &str) -> Result<&'a str> {
    object.get(field).and_then(Value::as_str).ok_or_else(|| {
        Exit::invalid_response(format!("GitHub check field {field} is missing or invalid"))
    })
}

fn cli_check_metadata(object: &Map<String, Value>, field: &str) -> Result<Option<String>> {
    let value = required_cli_check_string(object, field)?;
    let absent =
        value.is_empty() || matches!(field, "startedAt" | "completedAt") && value == ZERO_TIME;
    Ok(if absent { None } else { Some(value.to_owned()) })
}

pub(super) fn required_u64(object: &Map<String, Value>, field: &str, label: &str) -> Result<u64> {
    object
        .get(field)
        .and_then(Value::as_u64)
        .ok_or_else(|| Exit::invalid_response(format!("GitHub returned an invalid {label}")))
}

fn nullable_string<'a>(
    object: &'a Map<String, Value>,
    field: &str,
    label: &str,
) -> Result<Option<&'a str>> {
    match object.get(field) {
        Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value)),
        _ => Err(Exit::invalid_response(format!(
            "GitHub returned an invalid {label}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn checks_with_the_same_name_are_ordered_by_latest_started_at() {
        let mut older = valid_check_run_context();
        older["databaseId"] = json!(100);
        older["startedAt"] = json!("2026-08-11T10:00:00Z");
        let mut newer = valid_check_run_context();
        newer["databaseId"] = json!(101);
        newer["startedAt"] = json!("2026-08-11T11:00:00Z");

        let mut checks = validate_check_contexts(&[older, newer], false)
            .unwrap_or_else(|_| panic!("valid check run contexts"));
        checks.sort_by(super::super::output::compare_checks);

        assert_eq!(
            checks[0].started_at.as_deref(),
            Some("2026-08-11T11:00:00Z")
        );
        assert_eq!(checks[0].check_run_id, Some(101));
        assert_eq!(checks[1].check_run_id, Some(100));
    }

    #[test]
    fn check_run_ids_are_preserved_for_duplicate_check_runs() {
        let mut first = valid_check_run_context();
        first["databaseId"] = json!(100);
        let mut second = valid_check_run_context();
        second["databaseId"] = json!(101);

        let checks = validate_check_contexts(&[first, second], false)
            .unwrap_or_else(|_| panic!("valid duplicate check run contexts"));

        assert_eq!(checks.len(), 2);
        assert_eq!(checks[0].check_run_id, Some(100));
        assert_eq!(checks[1].check_run_id, Some(101));
        assert!(checks.iter().all(|check| check.check_run_id.is_some()));
    }

    fn valid_check_run_context() -> Value {
        json!({
            "__typename": "CheckRun",
            "databaseId": 100,
            "name": "build",
            "isRequired": true,
            "status": "COMPLETED",
            "conclusion": "FAILURE",
            "startedAt": "2026-08-11T10:00:00Z",
            "completedAt": "2026-08-11T10:05:00Z",
            "detailsUrl": "https://example.test/build",
            "checkSuite": {
                "workflowRun": {
                    "workflow": {"name": "CI"}
                }
            }
        })
    }

    #[test]
    fn unknown_check_run_status_fails_closed() {
        let mut context = valid_check_run_context();
        context["status"] = json!("BROKEN");

        let Err(error) = validate_check_contexts(&[context], false) else {
            panic!("unknown status must fail closed");
        };

        assert!(error.stderr_line().contains("\"kind\":\"invalidResponse\""));
    }

    #[test]
    fn missing_check_run_status_fails_closed() {
        let mut context = valid_check_run_context();
        context
            .as_object_mut()
            .expect("context object")
            .remove("status");

        let Err(error) = validate_check_contexts(&[context], false) else {
            panic!("missing status must fail closed");
        };

        assert!(error.stderr_line().contains("\"kind\":\"invalidResponse\""));
    }

    #[test]
    fn missing_completed_check_run_conclusion_fails_closed() {
        let mut context = valid_check_run_context();
        context
            .as_object_mut()
            .expect("context object")
            .remove("conclusion");

        let Err(error) = validate_check_contexts(&[context], false) else {
            panic!("missing conclusion must fail closed");
        };

        assert!(error.stderr_line().contains("\"kind\":\"invalidResponse\""));
    }

    #[test]
    fn unknown_check_run_conclusion_fails_closed() {
        let mut context = valid_check_run_context();
        context["conclusion"] = json!("UNKNOWN");

        let Err(error) = validate_check_contexts(&[context], false) else {
            panic!("unknown conclusion must fail closed");
        };

        assert!(error.stderr_line().contains("\"kind\":\"invalidResponse\""));
    }

    #[test]
    fn unknown_non_completed_check_run_conclusion_fails_closed() {
        let mut context = valid_check_run_context();
        context["status"] = json!("IN_PROGRESS");
        context["conclusion"] = json!("UNKNOWN");

        let Err(error) = validate_check_contexts(&[context], false) else {
            panic!("unknown pending conclusion must fail closed");
        };

        assert!(error.stderr_line().contains("\"kind\":\"invalidResponse\""));
    }

    #[test]
    fn malformed_workflow_shape_fails_closed() {
        let mut context = valid_check_run_context();
        context["checkSuite"] = json!({"workflowRun": {"workflow": null}});

        let Err(error) = validate_check_contexts(&[context], false) else {
            panic!("malformed workflow must fail closed");
        };

        assert!(error.stderr_line().contains("\"kind\":\"invalidResponse\""));
    }

    #[test]
    fn malformed_annotation_fails_closed() {
        let error = validate_annotations(&json!([[{"path": "partial.rs"}]]))
            .expect_err("malformed annotation must fail closed");

        assert!(error.stderr_line().contains("\"kind\":\"invalidResponse\""));
    }

    #[test]
    fn annotation_message_is_validated_before_title() {
        let error = validate_annotations(&json!([[
            {
                "path": "partial.rs",
                "start_line": 1,
                "end_line": 1,
                "annotation_level": "failure"
            }
        ]]))
        .expect_err("missing annotation message must fail closed");

        assert!(
            error
                .stderr_line()
                .contains("GitHub returned an invalid annotation message")
        );
    }
}
