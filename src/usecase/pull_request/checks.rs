use std::io::{self, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Map, Value, json};

use crate::error::{ErrorKind, Exit, Result, RuntimeError};
use crate::github;
use crate::model::{CheckDiagnosticsOptions, Target};

const LOG_LINE_LIMIT: usize = 200;
const LOG_BYTE_LIMIT: usize = 64 * 1024;
const UTF8_BOUNDARY_BYTES: usize = 4;
const ZERO_TIME: &str = "0001-01-01T00:00:00Z";
const CHECK_RUN_MARKER: &str = "__checkRun";
const CHECK_RUN_ID: &str = "__checkRunId";

pub fn execute(target: &Target, required: bool, options: CheckDiagnosticsOptions) -> Result<Value> {
    let diagnostics_requested = options.failed_diagnostics || options.include_failed_logs;
    let timeout_message = format!(
        "failed check diagnostics timed out after {} seconds",
        options.timeout_seconds
    );
    let deadline = diagnostic_deadline(options.timeout_seconds).ok_or_else(|| Exit {
        message: Some(format!(
            "argument --timeout: {} seconds cannot be represented as a diagnostic deadline",
            options.timeout_seconds
        )),
        code: 2,
    })?;
    let checks = if diagnostics_requested {
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
        for check in &mut checks {
            check.remove(CHECK_RUN_MARKER);
            check.remove(CHECK_RUN_ID);
        }
        checks
    } else {
        let response = github::pull_request::checks(target, required)?;
        let values = response
            .as_array()
            .ok_or_else(|| invalid_response("GitHub returned an invalid checks response"))?;
        let mut checks = values
            .iter()
            .map(validate_check)
            .collect::<Result<Vec<_>>>()?;
        checks.sort_by(compare_checks);
        checks
    };

    Ok(json!({ "checks": checks }))
}

fn collect_diagnostics(
    target: &Target,
    checks: &mut [Map<String, Value>],
    options: CheckDiagnosticsOptions,
    head_oid: &str,
    deadline: Instant,
    timeout_message: &str,
) -> Result<()> {
    let failed = checks
        .iter()
        .enumerate()
        .filter_map(|(index, check)| {
            matches!(string_field(check, "bucket"), "fail" | "cancel").then_some(index)
        })
        .collect::<Vec<_>>();
    if failed.is_empty() {
        return Ok(());
    }

    let progress = Progress::start(failed.len(), options.quiet);
    ensure_before_deadline(deadline, timeout_message)?;
    for index in failed {
        ensure_before_deadline(deadline, timeout_message)?;
        let check = &mut checks[index];
        let is_check_run = check.get(CHECK_RUN_MARKER) == Some(&Value::Bool(true));
        let check_run_id = if is_check_run {
            Some(
                check
                    .get(CHECK_RUN_ID)
                    .and_then(Value::as_u64)
                    .ok_or_else(|| invalid_response("GitHub returned an invalid Check Run ID"))?,
            )
        } else {
            None
        };
        let annotations = match check_run_id {
            Some(id) => validate_annotations(&github::pull_request::annotations(
                target,
                id,
                deadline,
                timeout_message,
            )?)?,
            None => Vec::new(),
        };
        check.insert("annotations".to_owned(), Value::Array(annotations));

        if options.include_failed_logs {
            let log = if is_check_run {
                collect_actions_log(
                    target,
                    optional_string_field(check, "link"),
                    check_run_id,
                    head_oid,
                    LOG_BYTE_LIMIT,
                    LOG_LINE_LIMIT,
                    deadline,
                    timeout_message,
                )?
            } else {
                Value::Null
            };
            check.insert("log".to_owned(), log);
        }
        progress.complete_one();
    }
    ensure_before_deadline(deadline, timeout_message)?;
    Ok(())
}

fn validate_check(value: &Value) -> Result<Map<String, Value>> {
    let object = value
        .as_object()
        .ok_or_else(|| invalid_response("GitHub returned an invalid check entry"))?;
    let check = Map::from_iter([
        (
            "name".to_owned(),
            Value::String(required_cli_check_string(object, "name")?.to_owned()),
        ),
        (
            "state".to_owned(),
            Value::String(required_cli_check_string(object, "state")?.to_owned()),
        ),
        (
            "bucket".to_owned(),
            Value::String(required_cli_check_string(object, "bucket")?.to_owned()),
        ),
        (
            "link".to_owned(),
            cli_nullable_check_metadata(object, "link")?,
        ),
        (
            "workflow".to_owned(),
            cli_nullable_check_metadata(object, "workflow")?,
        ),
        (
            "startedAt".to_owned(),
            cli_nullable_check_metadata(object, "startedAt")?,
        ),
        (
            "completedAt".to_owned(),
            cli_nullable_check_metadata(object, "completedAt")?,
        ),
    ]);
    match string_field(&check, "bucket") {
        "pass" | "fail" | "pending" | "skipping" | "cancel" => {}
        _ => return Err(invalid_response("GitHub returned an unknown check bucket")),
    }
    Ok(check)
}

fn validate_check_contexts(values: &[Value], required: bool) -> Result<Vec<Map<String, Value>>> {
    let mut checks = Vec::new();

    for value in values {
        let object = value
            .as_object()
            .ok_or_else(|| invalid_response("GitHub returned an invalid check context"))?;
        let is_required = object
            .get("isRequired")
            .and_then(Value::as_bool)
            .ok_or_else(|| invalid_response("GitHub returned an invalid required check marker"))?;
        if required && !is_required {
            continue;
        }
        let type_name = required_string(object, "__typename", "check context type")?;
        let (name, state, link, workflow, started_at, completed_at, check_run_id) = match type_name
        {
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
                (
                    name,
                    state,
                    link,
                    workflow,
                    started_at,
                    completed_at,
                    Some(check_run_id),
                )
            }
            "StatusContext" => {
                let name = required_string(object, "context", "commit status context")?;
                let state = required_string(object, "state", "commit status state")?.to_owned();
                let link = nullable_string(object, "targetUrl", "commit status target URL")?;
                (name, state, link, None, None, None, None)
            }
            _ => {
                return Err(invalid_response(
                    "GitHub returned an unknown check context type",
                ));
            }
        };
        let bucket = check_bucket(&state)?;
        checks.push(Map::from_iter([
            ("name".to_owned(), Value::String(name.to_owned())),
            ("state".to_owned(), Value::String(state)),
            ("bucket".to_owned(), Value::String(bucket.to_owned())),
            (
                "link".to_owned(),
                link.map_or(Value::Null, |value| Value::String(value.to_owned())),
            ),
            (
                "workflow".to_owned(),
                workflow.map_or(Value::Null, |value| Value::String(value.to_owned())),
            ),
            (
                "startedAt".to_owned(),
                started_at.map_or(Value::Null, |value| Value::String(value.to_owned())),
            ),
            (
                "completedAt".to_owned(),
                completed_at.map_or(Value::Null, |value| Value::String(value.to_owned())),
            ),
            (
                CHECK_RUN_MARKER.to_owned(),
                Value::Bool(check_run_id.is_some()),
            ),
            (
                CHECK_RUN_ID.to_owned(),
                check_run_id.map_or(Value::Null, Value::from),
            ),
        ]));
    }
    Ok(checks)
}

fn graphql_check_run_state(status: &str, conclusion: Option<&str>) -> Result<String> {
    let conclusion = match conclusion {
        None => None,
        Some(value) if is_known_check_run_conclusion(value) => Some(value),
        Some(_) => {
            return Err(invalid_response(
                "GitHub returned an unknown check run conclusion",
            ));
        }
    };
    if status == "COMPLETED" {
        return conclusion.map(str::to_owned).ok_or_else(|| {
            invalid_response("GitHub returned a completed check run without a conclusion")
        });
    }
    if conclusion.is_some() {
        return Err(invalid_response(
            "GitHub returned a non-completed check run with a conclusion",
        ));
    }
    match status {
        "IN_PROGRESS" | "PENDING" | "QUEUED" | "REQUESTED" | "WAITING" => Ok(status.to_owned()),
        _ => Err(invalid_response(
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
        .ok_or_else(|| invalid_response("GitHub returned an invalid check suite"))?;
    let Some(run) = suite.get("workflowRun") else {
        return Err(invalid_response("GitHub returned an invalid workflow run"));
    };
    if run.is_null() {
        return Ok(None);
    }
    let run = run
        .as_object()
        .ok_or_else(|| invalid_response("GitHub returned an invalid workflow run"))?;
    let workflow = run
        .get("workflow")
        .and_then(Value::as_object)
        .ok_or_else(|| invalid_response("GitHub returned an invalid workflow"))?;
    Ok(Some(required_string(workflow, "name", "workflow name")?))
}

fn check_bucket(state: &str) -> Result<&'static str> {
    match state {
        "SUCCESS" => Ok("pass"),
        "SKIPPED" | "NEUTRAL" => Ok("skipping"),
        "ERROR" | "FAILURE" | "TIMED_OUT" | "ACTION_REQUIRED" => Ok("fail"),
        "CANCELLED" => Ok("cancel"),
        "EXPECTED" | "REQUESTED" | "WAITING" | "QUEUED" | "PENDING" | "IN_PROGRESS" | "STALE"
        | "STARTUP_FAILURE" => Ok("pending"),
        _ => Err(invalid_response("GitHub returned an unknown check state")),
    }
}

fn validate_annotations(response: &Value) -> Result<Vec<Value>> {
    let pages = response
        .as_array()
        .ok_or_else(|| invalid_response("GitHub returned an invalid annotation response"))?;
    let mut annotations = Vec::new();
    for page in pages {
        let values = page
            .as_array()
            .ok_or_else(|| invalid_response("GitHub returned an invalid annotation page"))?;
        for value in values {
            let object = value
                .as_object()
                .ok_or_else(|| invalid_response("GitHub returned an invalid annotation"))?;
            let path = required_string(object, "path", "annotation path")?;
            let start_line = required_u64(object, "start_line", "annotation start line")?;
            let end_line = required_u64(object, "end_line", "annotation end line")?;
            let annotation_level = required_string(object, "annotation_level", "annotation level")?;
            let message = required_string(object, "message", "annotation message")?;
            let title = nullable_string(object, "title", "annotation title")?;
            annotations.push(json!({
                "path": path,
                "startLine": start_line,
                "endLine": end_line,
                "annotationLevel": annotation_level,
                "title": title,
                "message": message,
            }));
        }
    }
    annotations.sort_by(|left, right| {
        annotation_string(left, "path")
            .cmp(annotation_string(right, "path"))
            .then_with(|| {
                annotation_u64(left, "startLine").cmp(&annotation_u64(right, "startLine"))
            })
            .then_with(|| annotation_u64(left, "endLine").cmp(&annotation_u64(right, "endLine")))
            .then_with(|| {
                annotation_string(left, "message").cmp(annotation_string(right, "message"))
            })
            .then_with(|| {
                annotation_optional_string(left, "title")
                    .cmp(&annotation_optional_string(right, "title"))
            })
    });
    Ok(annotations)
}

fn required_string<'a>(
    object: &'a Map<String, Value>,
    field: &str,
    label: &str,
) -> Result<&'a str> {
    object
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_response(&format!("GitHub returned an invalid {label}")))
}

fn required_cli_check_string<'a>(object: &'a Map<String, Value>, field: &str) -> Result<&'a str> {
    object.get(field).and_then(Value::as_str).ok_or_else(|| {
        invalid_response(&format!("GitHub check field {field} is missing or invalid"))
    })
}

fn cli_nullable_check_metadata(object: &Map<String, Value>, field: &str) -> Result<Value> {
    let value = required_cli_check_string(object, field)?;
    let absent =
        value.is_empty() || matches!(field, "startedAt" | "completedAt") && value == ZERO_TIME;
    Ok(if absent {
        Value::Null
    } else {
        Value::String(value.to_owned())
    })
}

fn required_u64(object: &Map<String, Value>, field: &str, label: &str) -> Result<u64> {
    object
        .get(field)
        .and_then(Value::as_u64)
        .ok_or_else(|| invalid_response(&format!("GitHub returned an invalid {label}")))
}

fn nullable_string<'a>(
    object: &'a Map<String, Value>,
    field: &str,
    label: &str,
) -> Result<Option<&'a str>> {
    match object.get(field) {
        Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value)),
        _ => Err(invalid_response(&format!(
            "GitHub returned an invalid {label}"
        ))),
    }
}

fn annotation_string<'a>(annotation: &'a Value, field: &str) -> &'a str {
    annotation[field]
        .as_str()
        .expect("validated annotation field is a string")
}

fn annotation_optional_string<'a>(annotation: &'a Value, field: &str) -> Option<&'a str> {
    annotation[field].as_str()
}

fn annotation_u64(annotation: &Value, field: &str) -> u64 {
    annotation[field]
        .as_u64()
        .expect("validated annotation field is an integer")
}

fn actions_job_id(target: &Target, link: &str) -> Option<u64> {
    let path = strip_ascii_case_prefix(link, "https://github.com/")?;
    let path = path.split_once('?').map_or(path, |(path, _)| path);
    let mut segments = path.split('/');
    let owner = segments.next()?;
    let repository = segments.next()?;
    if !repository_matches(target, owner, repository)
        || segments.next()? != "actions"
        || segments.next()? != "runs"
    {
        return None;
    }
    let run_id = segments.next()?;
    if segments.next()? != "job" {
        return None;
    }
    let job_id = segments.next()?;
    if segments.next().is_some() {
        return None;
    }
    run_id.parse::<u64>().ok()?;
    job_id.parse().ok()
}

fn collect_actions_log(
    target: &Target,
    link: Option<&str>,
    check_run_id: Option<u64>,
    head_oid: &str,
    max_bytes: usize,
    max_lines: usize,
    deadline: Instant,
    timeout_message: &str,
) -> Result<Value> {
    let Some(job_id) = link.and_then(|link| actions_job_id(target, link)) else {
        return Ok(Value::Null);
    };
    let job = github::pull_request::job(target, job_id, deadline, timeout_message)?;
    let job = job
        .as_object()
        .ok_or_else(|| invalid_response("GitHub returned an invalid Actions job response"))?;
    let returned_id = required_u64(job, "id", "Actions job identifier")?;
    if returned_id != job_id {
        return Err(invalid_response(
            "GitHub returned a mismatched Actions job identifier",
        ));
    }
    let check_run_url = required_string(job, "check_run_url", "Actions job check run URL")?;
    let job_head_oid = required_string(job, "head_sha", "Actions job head SHA")?;
    let job_check_run_id = actions_check_run_id(target, check_run_url);
    if job_check_run_id != check_run_id || job_head_oid != head_oid {
        return Ok(Value::Null);
    }

    let bytes = github::pull_request::job_log(
        target,
        job_id,
        max_bytes,
        max_lines,
        deadline,
        timeout_message,
    )?;
    truncate_log(bytes)
}

fn actions_check_run_id(target: &Target, url: &str) -> Option<u64> {
    let path = strip_ascii_case_prefix(url, "https://api.github.com/repos/")?;
    let path = path.split_once('?').map_or(path, |(path, _)| path);
    let mut segments = path.split('/');
    let owner = segments.next()?;
    let repository = segments.next()?;
    if !repository_matches(target, owner, repository) || segments.next()? != "check-runs" {
        return None;
    }
    let check_run_id = segments.next()?;
    if segments.next().is_some() {
        return None;
    }
    check_run_id.parse().ok()
}

fn repository_matches(target: &Target, owner: &str, repository: &str) -> bool {
    target
        .repository
        .split_once('/')
        .is_some_and(|(target_owner, target_repository)| {
            target_owner.eq_ignore_ascii_case(owner)
                && target_repository.eq_ignore_ascii_case(repository)
        })
}

fn strip_ascii_case_prefix<'a>(value: &'a str, prefix: &str) -> Option<&'a str> {
    let candidate = value.get(..prefix.len())?;
    candidate
        .eq_ignore_ascii_case(prefix)
        .then(|| &value[prefix.len()..])
}

fn truncate_log(log: github::pull_request::BoundedBytes) -> Result<Value> {
    let github::pull_request::BoundedBytes {
        bytes,
        total_bytes,
        total_newlines,
        valid_utf8,
    } = log;
    if !valid_utf8 {
        return Err(invalid_response("GitHub returned a non-UTF-8 job log"));
    }
    let byte_start = bytes.len().saturating_sub(LOG_BYTE_LIMIT);
    let line_start = line_start(&bytes, total_newlines);
    let mut start = byte_start.max(line_start);
    let mut text = std::str::from_utf8(&bytes[start..]);
    while let Err(error) = text {
        if line_start >= byte_start
            || error.valid_up_to() != 0
            || start >= byte_start.saturating_add(UTF8_BOUNDARY_BYTES)
            || start >= bytes.len()
        {
            return Err(invalid_response("GitHub returned a non-UTF-8 job log"));
        }
        start += 1;
        text = std::str::from_utf8(&bytes[start..]);
    }
    let text = text.expect("valid UTF-8 log after boundary adjustment");
    let omitted_bytes = total_bytes.saturating_sub(bytes.len() as u64) + start as u64;
    let omitted_lines = total_newlines.saturating_sub(newline_count(&bytes[start..]));
    Ok(json!({
        "text": text,
        "truncated": omitted_bytes > 0,
        "omittedLines": omitted_lines,
        "omittedBytes": omitted_bytes,
    }))
}

fn line_start(bytes: &[u8], total_newlines: u64) -> usize {
    let Some(last_byte) = bytes.last() else {
        return 0;
    };
    let total_lines = if *last_byte == b'\n' {
        total_newlines
    } else {
        total_newlines.saturating_add(1)
    };
    let omitted_lines = total_lines.saturating_sub(LOG_LINE_LIMIT as u64);
    if omitted_lines == 0 {
        return 0;
    }

    let retained_newlines = newline_count(bytes);
    let newlines_before = total_newlines.saturating_sub(retained_newlines);
    if omitted_lines <= newlines_before {
        return 0;
    }
    let target = omitted_lines - newlines_before;
    let mut seen = 0_u64;
    for (index, byte) in bytes.iter().enumerate() {
        if *byte == b'\n' {
            seen += 1;
            if seen == target {
                return index + 1;
            }
        }
    }
    bytes.len()
}

fn newline_count(bytes: &[u8]) -> u64 {
    bytes
        .iter()
        .fold(0_u64, |count, byte| count + u64::from(*byte == b'\n'))
}

fn string_field<'a>(object: &'a Map<String, Value>, field: &str) -> &'a str {
    object
        .get(field)
        .and_then(Value::as_str)
        .expect("validated check fields are strings")
}

fn optional_string_field<'a>(object: &'a Map<String, Value>, field: &str) -> Option<&'a str> {
    object.get(field).and_then(Value::as_str)
}

fn compare_checks(left: &Map<String, Value>, right: &Map<String, Value>) -> std::cmp::Ordering {
    string_field(left, "name")
        .cmp(string_field(right, "name"))
        .then_with(|| {
            optional_string_field(left, "link").cmp(&optional_string_field(right, "link"))
        })
        .then_with(|| {
            optional_string_field(right, "startedAt").cmp(&optional_string_field(left, "startedAt"))
        })
        .then_with(|| {
            left.get(CHECK_RUN_ID)
                .and_then(Value::as_u64)
                .cmp(&right.get(CHECK_RUN_ID).and_then(Value::as_u64))
        })
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

fn ensure_before_deadline(deadline: Instant, timeout_message: &str) -> Result<()> {
    if Instant::now() < deadline {
        return Ok(());
    }
    Err(Exit::runtime(
        &RuntimeError {
            kind: ErrorKind::Timeout,
            message: timeout_message.to_owned(),
            retryable: true,
            retry_after_seconds: None,
        },
        1,
    ))
}

fn diagnostic_deadline(timeout_seconds: u64) -> Option<Instant> {
    Instant::now().checked_add(Duration::from_secs(timeout_seconds))
}

struct Progress {
    completed: Arc<AtomicUsize>,
    state: Arc<(Mutex<bool>, Condvar)>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Progress {
    fn start(total: usize, quiet: bool) -> Self {
        let completed = Arc::new(AtomicUsize::new(0));
        let state = Arc::new((Mutex::new(false), Condvar::new()));
        if quiet {
            return Self {
                completed,
                state,
                thread: None,
            };
        }

        writeln!(
            io::stderr(),
            "gh-loupe: collecting diagnostics for {total} failed checks"
        )
        .ok();
        let thread_completed = Arc::clone(&completed);
        let thread_state = Arc::clone(&state);
        let progress_thread = thread::spawn(move || {
            let started = Instant::now();
            let (lock, wake) = &*thread_state;
            let mut finished = lock.lock().expect("lock diagnostic progress");
            loop {
                let (next_finished, timeout) = wake
                    .wait_timeout(finished, Duration::from_secs(15))
                    .expect("wait for diagnostic progress");
                finished = next_finished;
                if *finished {
                    break;
                }
                if timeout.timed_out() {
                    let done = thread_completed.load(Ordering::Relaxed);
                    let elapsed = started.elapsed().as_secs();
                    writeln!(
                        io::stderr(),
                        "gh-loupe: diagnostics {done}/{total} complete; {elapsed}s elapsed"
                    )
                    .ok();
                }
            }
        });
        Self {
            completed,
            state,
            thread: Some(progress_thread),
        }
    }

    fn complete_one(&self) {
        self.completed.fetch_add(1, Ordering::Relaxed);
    }
}

impl Drop for Progress {
    fn drop(&mut self) {
        let (lock, wake) = &*self.state;
        *lock.lock().expect("lock diagnostic progress") = true;
        wake.notify_one();
        if let Some(progress_thread) = self.thread.take() {
            progress_thread.join().expect("join diagnostic progress");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_applies_line_limit_before_byte_limit() {
        let long_line = "x".repeat(LOG_BYTE_LIMIT);
        let mut input = (0..201).map(|_| "old\n").collect::<String>();
        input.push_str(&long_line);
        input.push_str("tail");
        let total_bytes = input.len() as u64;
        let total_newlines = newline_count(input.as_bytes());

        let log = truncate_log(github::pull_request::BoundedBytes {
            bytes: input.into_bytes(),
            total_bytes,
            total_newlines,
            valid_utf8: true,
        })
        .unwrap_or_else(|_| panic!("truncate log"));

        assert_eq!(
            log["text"].as_str().expect("log text").len(),
            LOG_BYTE_LIMIT
        );
        assert_eq!(log["omittedLines"], 201);
        assert_eq!(log["omittedBytes"], 808);
        assert_eq!(log["truncated"], true);
    }

    #[test]
    fn log_within_both_limits_reports_no_omissions() {
        let bytes = b"first\nsecond\n".to_vec();
        let log = truncate_log(github::pull_request::BoundedBytes {
            total_bytes: bytes.len() as u64,
            total_newlines: newline_count(&bytes),
            bytes,
            valid_utf8: true,
        })
        .unwrap_or_else(|_| panic!("retain log"));

        assert_eq!(log["text"], "first\nsecond\n");
        assert_eq!(log["truncated"], false);
        assert_eq!(log["omittedLines"], 0);
        assert_eq!(log["omittedBytes"], 0);
    }

    #[test]
    fn multibyte_character_crossing_byte_limit_is_not_split() {
        let mut input = vec![b'x'; LOG_BYTE_LIMIT - 1];
        input.extend_from_slice("あ".as_bytes());
        input.extend(std::iter::repeat_n(b'x', LOG_BYTE_LIMIT - 7));
        input.extend_from_slice(b"tail\n");
        let total_bytes = input.len() as u64;
        let total_newlines = newline_count(&input);
        let retained_start = input.len() - (LOG_BYTE_LIMIT + UTF8_BOUNDARY_BYTES);
        let bytes = input[retained_start..].to_vec();

        let log = truncate_log(github::pull_request::BoundedBytes {
            bytes,
            total_bytes,
            total_newlines,
            valid_utf8: true,
        })
        .unwrap_or_else(|_| panic!("truncate UTF-8 log"));

        assert!(
            log["text"]
                .as_str()
                .is_some_and(|text| text.ends_with("tail\n"))
        );
        assert_eq!(log["omittedBytes"], 65_538);
        assert_eq!(log["omittedLines"], 0);
    }

    #[test]
    fn actions_job_requires_the_fixed_repository_url_shape() {
        let target = Target {
            repository: "owner/repo".to_owned(),
            number: "1".to_owned(),
        };

        assert_eq!(
            actions_job_id(
                &target,
                "https://github.com/owner/repo/actions/runs/10/job/20?pr=1"
            ),
            Some(20)
        );
        assert_eq!(
            actions_job_id(&target, "https://example.test/actions/runs/10/job/20"),
            None
        );
    }

    #[test]
    fn actions_urls_match_repository_names_case_insensitively() {
        let target = Target {
            repository: "Owner/Repo".to_owned(),
            number: "1".to_owned(),
        };

        assert_eq!(
            actions_job_id(
                &target,
                "https://github.com/owner/repo/actions/runs/10/job/20"
            ),
            Some(20)
        );
        assert_eq!(
            actions_check_run_id(
                &target,
                "https://api.github.com/repos/owner/repo/check-runs/100"
            ),
            Some(100)
        );
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

        let error = validate_check_contexts(&[context], false)
            .expect_err("unknown status must fail closed");

        assert!(
            error
                .stderr_line()
                .is_some_and(|line| line.contains("\"kind\":\"invalidResponse\""))
        );
    }

    #[test]
    fn missing_check_run_status_fails_closed() {
        let mut context = valid_check_run_context();
        context
            .as_object_mut()
            .expect("context object")
            .remove("status");

        let error = validate_check_contexts(&[context], false)
            .expect_err("missing status must fail closed");

        assert!(
            error
                .stderr_line()
                .is_some_and(|line| line.contains("\"kind\":\"invalidResponse\""))
        );
    }

    #[test]
    fn missing_completed_check_run_conclusion_fails_closed() {
        let mut context = valid_check_run_context();
        context
            .as_object_mut()
            .expect("context object")
            .remove("conclusion");

        let error = validate_check_contexts(&[context], false)
            .expect_err("missing conclusion must fail closed");

        assert!(
            error
                .stderr_line()
                .is_some_and(|line| line.contains("\"kind\":\"invalidResponse\""))
        );
    }

    #[test]
    fn unknown_check_run_conclusion_fails_closed() {
        let mut context = valid_check_run_context();
        context["conclusion"] = json!("UNKNOWN");

        let error = validate_check_contexts(&[context], false)
            .expect_err("unknown conclusion must fail closed");

        assert!(
            error
                .stderr_line()
                .is_some_and(|line| line.contains("\"kind\":\"invalidResponse\""))
        );
    }

    #[test]
    fn unknown_non_completed_check_run_conclusion_fails_closed() {
        let mut context = valid_check_run_context();
        context["status"] = json!("IN_PROGRESS");
        context["conclusion"] = json!("UNKNOWN");

        let error = validate_check_contexts(&[context], false)
            .expect_err("unknown pending conclusion must fail closed");

        assert!(
            error
                .stderr_line()
                .is_some_and(|line| line.contains("\"kind\":\"invalidResponse\""))
        );
    }

    #[test]
    fn malformed_workflow_shape_fails_closed() {
        let mut context = valid_check_run_context();
        context["checkSuite"] = json!({"workflowRun": {"workflow": null}});

        let error = validate_check_contexts(&[context], false)
            .expect_err("malformed workflow must fail closed");

        assert!(
            error
                .stderr_line()
                .is_some_and(|line| line.contains("\"kind\":\"invalidResponse\""))
        );
    }

    #[test]
    fn malformed_annotation_fails_closed() {
        let error = validate_annotations(&json!([[{"path": "partial.rs"}]]))
            .expect_err("malformed annotation must fail closed");

        assert!(
            error
                .stderr_line()
                .is_some_and(|line| line.contains("\"kind\":\"invalidResponse\""))
        );
    }
}
