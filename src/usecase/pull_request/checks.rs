use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
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
const MAX_DIAGNOSTIC_WORKERS: usize = 4;

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
    fn from_object(object: &Map<String, Value>) -> Result<Self> {
        Ok(Self {
            path: required_string(object, "path", "annotation path")?.to_owned(),
            start_line: required_u64(object, "start_line", "annotation start line")?,
            end_line: required_u64(object, "end_line", "annotation end line")?,
            annotation_level: required_string(object, "annotation_level", "annotation level")?
                .to_owned(),
            title: nullable_string(object, "title", "annotation title")?.map(str::to_owned),
            message: required_string(object, "message", "annotation message")?.to_owned(),
        })
    }

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
        checks
    } else {
        let response = github::pull_request::checks(target, required)?;
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

    let mut result = Map::new();
    result.insert(
        "checks".to_owned(),
        Value::Array(checks.into_iter().map(Check::into_value).collect()),
    );
    Ok(Value::Object(result))
}

fn collect_diagnostics(
    target: &Target,
    checks: &mut [Check],
    options: CheckDiagnosticsOptions,
    head_oid: &str,
    deadline: Instant,
    timeout_message: &str,
) -> Result<()> {
    let failed = checks
        .iter()
        .enumerate()
        .filter_map(|(index, check)| {
            matches!(check.bucket, CheckBucket::Fail | CheckBucket::Cancel).then_some(index)
        })
        .collect::<Vec<_>>();
    if failed.is_empty() {
        return Ok(());
    }

    let progress = Progress::start(failed.len(), options.quiet);
    ensure_before_deadline(deadline, timeout_message)?;
    let context = DiagnosticContext {
        target,
        include_failed_logs: options.include_failed_logs,
        head_oid,
        deadline,
        timeout_message,
    };
    let diagnostic_jobs = failed
        .iter()
        .enumerate()
        .filter_map(|(position, &index)| {
            checks[index]
                .check_run_id
                .map(|check_run_id| DiagnosticJob {
                    index,
                    position,
                    check_run_id,
                    link: checks[index].link.clone(),
                })
        })
        .collect::<Vec<_>>();
    if diagnostic_worker_count(diagnostic_jobs.len()) == 0 {
        for index in failed {
            ensure_before_deadline(deadline, timeout_message)?;
            let check = &mut checks[index];
            let result =
                collect_one_diagnostic(&context, check.check_run_id, check.link.as_deref())?;
            apply_diagnostic_result(check, result, options.include_failed_logs);
            progress.complete_one();
        }
    } else {
        for &index in &failed {
            if checks[index].check_run_id.is_none() {
                let result = collect_one_diagnostic(&context, None, None)?;
                apply_diagnostic_result(&mut checks[index], result, options.include_failed_logs);
            }
        }
        let results =
            collect_diagnostics_parallel(&context, &diagnostic_jobs, failed.len(), &progress);
        for (index, result) in results {
            let result = result?;
            apply_diagnostic_result(&mut checks[index], result, options.include_failed_logs);
        }
    }
    ensure_before_deadline(deadline, timeout_message)?;
    Ok(())
}

struct DiagnosticJob {
    index: usize,
    position: usize,
    check_run_id: u64,
    link: Option<String>,
}

struct DiagnosticContext<'a> {
    target: &'a Target,
    include_failed_logs: bool,
    head_oid: &'a str,
    deadline: Instant,
    timeout_message: &'a str,
}

fn diagnostic_worker_count(job_count: usize) -> usize {
    if job_count < 2 {
        0
    } else {
        job_count.min(MAX_DIAGNOSTIC_WORKERS)
    }
}

struct DiagnosticResult {
    annotations: Vec<Annotation>,
    log: Option<Value>,
}

fn collect_one_diagnostic(
    context: &DiagnosticContext<'_>,
    check_run_id: Option<u64>,
    link: Option<&str>,
) -> Result<DiagnosticResult> {
    let annotations = match check_run_id {
        Some(id) => validate_annotations(&github::pull_request::annotations(
            context.target,
            id,
            context.deadline,
            context.timeout_message,
        )?)?,
        None => Vec::new(),
    };
    let log = if context.include_failed_logs {
        Some(if check_run_id.is_some() {
            collect_actions_log(
                context.target,
                link,
                check_run_id,
                context.head_oid,
                LOG_BYTE_LIMIT,
                LOG_LINE_LIMIT,
                context.deadline,
                context.timeout_message,
            )?
        } else {
            Value::Null
        })
    } else {
        None
    };
    Ok(DiagnosticResult { annotations, log })
}

fn apply_diagnostic_result(check: &mut Check, result: DiagnosticResult, include_failed_logs: bool) {
    check.annotations = Some(result.annotations);
    if include_failed_logs {
        check.log = result.log;
    }
}

fn collect_diagnostics_parallel(
    context: &DiagnosticContext<'_>,
    jobs: &[DiagnosticJob],
    total_progress: usize,
    progress: &Progress,
) -> Vec<(usize, Result<DiagnosticResult>)> {
    let next_job = Arc::new(Mutex::new(0usize));
    let stop = Arc::new(AtomicBool::new(false));
    let results = Arc::new(Mutex::new(Vec::with_capacity(jobs.len())));
    let mut is_job = vec![false; total_progress];
    for job in jobs {
        is_job[job.position] = true;
    }
    let completed = Arc::new(
        is_job
            .into_iter()
            .map(|is_job| AtomicBool::new(!is_job))
            .collect::<Vec<_>>(),
    );
    let next_completed = Arc::new(AtomicUsize::new(0));
    let worker_count = diagnostic_worker_count(jobs.len());
    mark_ordered_completion(&completed, &next_completed, total_progress, progress);

    thread::scope(|scope| {
        for _ in 0..worker_count {
            let next_job = Arc::clone(&next_job);
            let stop = Arc::clone(&stop);
            let results = Arc::clone(&results);
            let completed = Arc::clone(&completed);
            let next_completed = Arc::clone(&next_completed);
            scope.spawn(move || {
                while let Some(job_position) =
                    claim_next_diagnostic_job(&next_job, &stop, jobs.len())
                {
                    let job = &jobs[job_position];
                    let result = ensure_before_deadline(context.deadline, context.timeout_message)
                        .and_then(|()| {
                            collect_one_diagnostic(
                                context,
                                Some(job.check_run_id),
                                job.link.as_deref(),
                            )
                        });
                    let failed = result.is_err();
                    if failed {
                        stop.store(true, Ordering::Release);
                    }
                    results
                        .lock()
                        .expect("lock diagnostic results")
                        .push((job.index, result));
                    if !failed {
                        completed[job.position].store(true, Ordering::Release);
                        mark_ordered_completion(
                            &completed,
                            &next_completed,
                            total_progress,
                            progress,
                        );
                    }
                }
            });
        }
    });
    let mut results = {
        let mut shared_results = results.lock().expect("lock diagnostic results");
        std::mem::take(&mut *shared_results)
    };
    results.sort_unstable_by_key(|(index, _)| *index);
    results
}

fn claim_next_diagnostic_job(
    next_job: &Mutex<usize>,
    stop: &AtomicBool,
    job_count: usize,
) -> Option<usize> {
    let mut next_job = next_job.lock().expect("lock next diagnostic job");
    if stop.load(Ordering::Acquire) || *next_job >= job_count {
        return None;
    }
    let job_position = *next_job;
    *next_job += 1;
    Some(job_position)
}

fn mark_ordered_completion(
    completed: &[AtomicBool],
    next_completed: &AtomicUsize,
    total: usize,
    progress: &Progress,
) {
    loop {
        let next = next_completed.load(Ordering::Acquire);
        if next >= total || !completed[next].load(Ordering::Acquire) {
            break;
        }
        if next_completed
            .compare_exchange(next, next + 1, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            progress.complete_one();
        }
    }
}

fn validate_check(value: &Value) -> Result<Check> {
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

fn validate_check_contexts(values: &[Value], required: bool) -> Result<Vec<Check>> {
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

fn validate_annotations(response: &Value) -> Result<Vec<Annotation>> {
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
            annotations.push(Annotation::from_object(object)?);
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

fn required_string<'a>(
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

fn required_u64(object: &Map<String, Value>, field: &str, label: &str) -> Result<u64> {
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
        .ok_or_else(|| Exit::invalid_response("GitHub returned an invalid Actions job response"))?;
    let returned_id = required_u64(job, "id", "Actions job identifier")?;
    if returned_id != job_id {
        return Err(Exit::invalid_response(
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
        return Err(Exit::invalid_response(
            "GitHub returned a non-UTF-8 job log",
        ));
    }
    let byte_start = bytes.len().saturating_sub(LOG_BYTE_LIMIT);
    let mut start = byte_start;
    let mut text = std::str::from_utf8(&bytes[start..]);
    while let Err(error) = text {
        if error.valid_up_to() != 0
            || start >= byte_start.saturating_add(UTF8_BOUNDARY_BYTES)
            || start >= bytes.len()
        {
            return Err(Exit::invalid_response(
                "GitHub returned a non-UTF-8 job log",
            ));
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

fn newline_count(bytes: &[u8]) -> u64 {
    bytes
        .iter()
        .fold(0_u64, |count, byte| count + u64::from(*byte == b'\n'))
}

fn compare_checks(left: &Check, right: &Check) -> std::cmp::Ordering {
    left.name
        .cmp(&right.name)
        .then_with(|| left.link.cmp(&right.link))
        .then_with(|| right.started_at.cmp(&left.started_at))
        .then_with(|| left.check_run_id.cmp(&right.check_run_id))
}

fn ensure_before_deadline(deadline: Instant, timeout_message: &str) -> Result<()> {
    if Instant::now() < deadline {
        return Ok(());
    }
    Err(Exit::runtime(&RuntimeError {
        kind: ErrorKind::Timeout,
        message: timeout_message.to_owned(),
        retryable: true,
        retry_after_seconds: None,
    }))
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
    fn diagnostics_with_zero_or_one_job_do_not_create_workers() {
        assert_eq!(diagnostic_worker_count(0), 0);
        assert_eq!(diagnostic_worker_count(1), 0);
        assert_eq!(diagnostic_worker_count(2), 2);
        assert_eq!(diagnostic_worker_count(10), MAX_DIAGNOSTIC_WORKERS);
    }

    #[test]
    fn diagnostic_job_claim_stops_atomically_after_failure() {
        let next_job = Mutex::new(0);
        let stop = AtomicBool::new(false);

        assert_eq!(claim_next_diagnostic_job(&next_job, &stop, 2), Some(0));
        stop.store(true, Ordering::Release);
        assert_eq!(claim_next_diagnostic_job(&next_job, &stop, 2), None);
    }

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

    fn bucket(state: &str) -> CheckBucket {
        CheckBucket::from_state(state).unwrap_or_else(|_| panic!("known check state: {state}"))
    }

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
        checks.sort_by(compare_checks);

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
}
