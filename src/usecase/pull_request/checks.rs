use std::collections::HashSet;
use std::io::{self, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Map, Value, json};

use crate::error::{ErrorKind, Exit, Result, RuntimeError};
use crate::github;
use crate::model::{CheckDiagnosticsOptions, Target};
use crate::output;

const CHECK_FIELDS: [&str; 7] = [
    "name",
    "state",
    "bucket",
    "link",
    "workflow",
    "startedAt",
    "completedAt",
];
const LOG_LINE_LIMIT: usize = 200;
const LOG_BYTE_LIMIT: usize = 64 * 1024;

pub fn execute(target: &Target, required: bool, options: CheckDiagnosticsOptions) -> Result<Value> {
    let diagnostics_requested = options.failed_diagnostics || options.include_failed_logs;
    let timeout_message = format!(
        "failed check diagnostics timed out after {} seconds",
        options.timeout_seconds
    );
    let deadline = diagnostic_deadline(options.timeout_seconds);
    let response = if diagnostics_requested {
        github::pull_request::checks_with_deadline(target, required, deadline, &timeout_message)?
    } else {
        github::pull_request::checks(target, required)?
    };
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

    if diagnostics_requested {
        collect_diagnostics(target, &mut checks, options, deadline, &timeout_message)?;
    }
    Ok(output::success(json!({ "checks": checks })))
}

fn collect_diagnostics(
    target: &Target,
    checks: &mut [Map<String, Value>],
    options: CheckDiagnosticsOptions,
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
    let head = github::pull_request::head_oid(target, deadline, timeout_message)?;
    let head_oid = head
        .get("headRefOid")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_response("GitHub returned an invalid pull request head"))?;
    let check_runs = github::pull_request::check_runs(target, head_oid, deadline, timeout_message)?;
    let check_runs = validate_check_runs(&check_runs)?;
    let mut used_check_runs = HashSet::new();

    for index in failed {
        ensure_before_deadline(deadline, timeout_message)?;
        let check = &mut checks[index];
        let check_run_id = matching_check_run(check, &check_runs, &mut used_check_runs);
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
            let log = collect_actions_log(
                target,
                string_field(check, "link"),
                check_run_id,
                head_oid,
                deadline,
                timeout_message,
            )?;
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
    let mut check = Map::new();
    for field in CHECK_FIELDS {
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

#[derive(Clone)]
struct CheckRun {
    id: u64,
    name: String,
    details_url: Option<String>,
    state: String,
    started_at: Option<String>,
    completed_at: Option<String>,
}

fn validate_check_runs(response: &Value) -> Result<Vec<CheckRun>> {
    let pages = response
        .as_array()
        .ok_or_else(|| invalid_response("GitHub returned an invalid check run response"))?;
    let mut runs = Vec::new();
    for page in pages {
        let page_runs = page
            .get("check_runs")
            .and_then(Value::as_array)
            .ok_or_else(|| invalid_response("GitHub returned an invalid check run page"))?;
        for run in page_runs {
            let id = run.get("id").and_then(Value::as_u64).ok_or_else(|| {
                invalid_response("GitHub returned an invalid check run identifier")
            })?;
            let name = run
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| invalid_response("GitHub returned an invalid check run name"))?;
            let details_url = nullable_string(
                run.as_object()
                    .ok_or_else(|| invalid_response("GitHub returned an invalid check run"))?,
                "details_url",
                "check run details URL",
            )?
            .map(str::to_owned);
            let status = required_string(
                run.as_object()
                    .ok_or_else(|| invalid_response("GitHub returned an invalid check run"))?,
                "status",
                "check run status",
            )?;
            let conclusion = nullable_string(
                run.as_object()
                    .ok_or_else(|| invalid_response("GitHub returned an invalid check run"))?,
                "conclusion",
                "check run conclusion",
            )?;
            let state = check_run_state(status, conclusion)?;
            let started_at = nullable_string(
                run.as_object()
                    .ok_or_else(|| invalid_response("GitHub returned an invalid check run"))?,
                "started_at",
                "check run start time",
            )?
            .map(str::to_owned);
            let completed_at = nullable_string(
                run.as_object()
                    .ok_or_else(|| invalid_response("GitHub returned an invalid check run"))?,
                "completed_at",
                "check run completion time",
            )?
            .map(str::to_owned);
            runs.push(CheckRun {
                id,
                name: name.to_owned(),
                details_url,
                state,
                started_at,
                completed_at,
            });
        }
    }
    Ok(runs)
}

fn matching_check_run(
    check: &Map<String, Value>,
    runs: &[CheckRun],
    used: &mut HashSet<u64>,
) -> Option<u64> {
    let name = string_field(check, "name");
    let link = string_field(check, "link");
    let run = runs
        .iter()
        .filter(|run| !used.contains(&run.id))
        .find(|run| {
            run.name == name
                && run.details_url.as_deref().unwrap_or_default() == link
                && run.state.eq_ignore_ascii_case(string_field(check, "state"))
                && run.started_at.as_deref().unwrap_or_default() == string_field(check, "startedAt")
                && run.completed_at.as_deref().unwrap_or_default()
                    == string_field(check, "completedAt")
        })?;
    used.insert(run.id);
    Some(run.id)
}

fn check_run_state(status: &str, conclusion: Option<&str>) -> Result<String> {
    if status != "completed" {
        return match status {
            "queued" | "in_progress" | "pending" | "requested" | "waiting" => {
                Ok("PENDING".to_owned())
            }
            _ => Err(invalid_response(
                "GitHub returned an unknown check run status",
            )),
        };
    }
    let conclusion = conclusion.ok_or_else(|| {
        invalid_response("GitHub returned a completed check run without a conclusion")
    })?;
    Ok(conclusion.to_ascii_uppercase())
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
    link: &str,
    check_run_id: Option<u64>,
    head_oid: &str,
    deadline: Instant,
    timeout_message: &str,
) -> Result<Value> {
    let Some(job_id) = actions_job_id(target, link) else {
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

    let bytes = github::pull_request::job_log(target, job_id, deadline, timeout_message)?;
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

fn truncate_log(bytes: Vec<u8>) -> Result<Value> {
    let text = String::from_utf8(bytes)
        .map_err(|_| invalid_response("GitHub returned a non-UTF-8 job log"))?;
    let lines = text.split_inclusive('\n').collect::<Vec<_>>();
    let first_line = lines.len().saturating_sub(LOG_LINE_LIMIT);
    let omitted_line_bytes = lines[..first_line]
        .iter()
        .map(|line| line.len())
        .sum::<usize>();
    let mut omitted_lines = first_line;
    let line_limited = &text[omitted_line_bytes..];

    let mut byte_start = line_limited.len().saturating_sub(LOG_BYTE_LIMIT);
    while !line_limited.is_char_boundary(byte_start) {
        byte_start += 1;
    }
    omitted_lines += line_limited[..byte_start]
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count();
    let omitted_bytes = omitted_line_bytes + byte_start;
    Ok(json!({
        "text": &line_limited[byte_start..],
        "truncated": omitted_bytes > 0,
        "omittedLines": omitted_lines,
        "omittedBytes": omitted_bytes,
    }))
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

fn diagnostic_deadline(timeout_seconds: u64) -> Instant {
    let now = Instant::now();
    let mut seconds = timeout_seconds;
    loop {
        if let Some(deadline) = now.checked_add(Duration::from_secs(seconds)) {
            return deadline;
        }
        seconds /= 2;
    }
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
            "gh-read: collecting diagnostics for {total} failed checks"
        )
        .expect("write diagnostic progress");
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
                        "gh-read: diagnostics {done}/{total} complete; {elapsed}s elapsed"
                    )
                    .expect("write diagnostic progress");
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

        let log = truncate_log(input.into_bytes()).unwrap_or_else(|_| panic!("truncate log"));

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
        let log =
            truncate_log(b"first\nsecond\n".to_vec()).unwrap_or_else(|_| panic!("retain log"));

        assert_eq!(log["text"], "first\nsecond\n");
        assert_eq!(log["truncated"], false);
        assert_eq!(log["omittedLines"], 0);
        assert_eq!(log["omittedBytes"], 0);
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
}
