use std::ffi::OsStr;
use std::io::{self, Read, Write};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::error::{ErrorKind, Exit, Result, RuntimeError};

pub(super) fn json<I, S>(args: I, payload: Option<&str>, allow_nonzero_json: bool) -> Result<Value>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    json_with_deadline(args, payload, allow_nonzero_json, None)
}

pub(super) fn json_runtime<I, S>(
    args: I,
    payload: Option<&str>,
    allow_nonzero_json: bool,
) -> Result<Value>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    json_runtime_with_empty(args, payload, allow_nonzero_json, None)
}

pub(super) fn json_runtime_or_empty<I, S>(
    args: I,
    payload: Option<&str>,
    allow_nonzero_json: bool,
    empty_error_prefix: &str,
) -> Result<Value>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    json_runtime_with_empty(args, payload, allow_nonzero_json, Some(empty_error_prefix))
}

fn json_runtime_with_empty<I, S>(
    args: I,
    payload: Option<&str>,
    allow_nonzero_json: bool,
    empty_error_prefix: Option<&str>,
) -> Result<Value>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut command = Command::new("gh");
    command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if payload.is_some() {
        command.stdin(Stdio::piped());
    }
    let output = execute(command, payload, None).map_err(|error| {
        runtime_exit(
            ErrorKind::GitHubCli,
            format!("failed to execute GitHub CLI: {error}"),
            false,
            None,
        )
    })?;
    let code = output.status.code().unwrap_or(1);
    if !output.status.success() && (!allow_nonzero_json || !matches!(code, 1 | 8)) {
        return Err(classify_failure(code, &output.stderr));
    }
    match serde_json::from_slice(&output.stdout) {
        Ok(response) => Ok(response),
        Err(_error)
            if code == 1
                && output.stdout.is_empty()
                && empty_error_prefix.is_some_and(|prefix| {
                    String::from_utf8_lossy(&output.stderr)
                        .trim()
                        .starts_with(prefix)
                }) =>
        {
            Ok(Value::Array(Vec::new()))
        }
        Err(_error) if !output.status.success() => Err(classify_failure(code, &output.stderr)),
        Err(error) => Err(runtime_exit(
            ErrorKind::InvalidResponse,
            format!("GitHub returned invalid JSON: {error}"),
            false,
            None,
        )),
    }
}

fn classify_failure(code: i32, stderr: &[u8]) -> Exit {
    let message = String::from_utf8_lossy(stderr).trim().to_owned();
    let message = if message.is_empty() {
        format!("GitHub CLI exited with status {code}")
    } else {
        message
    };
    let normalized = message.to_ascii_lowercase();
    let (kind, retryable) = if code == 4
        || normalized.contains("gh auth login")
        || normalized.contains("http 401")
        || normalized.contains("authentication")
    {
        (ErrorKind::Authentication, false)
    } else if normalized.contains("rate limit") || normalized.contains("secondary rate") {
        (ErrorKind::RateLimited, true)
    } else if normalized.contains("http 403") || normalized.contains("forbidden") {
        (ErrorKind::Authorization, false)
    } else if normalized.contains("http 404")
        || normalized.contains("not found")
        || normalized.contains("could not resolve to a pullrequest")
        || normalized.contains("could not resolve to a repository")
    {
        (ErrorKind::NotFound, false)
    } else if normalized.contains("could not resolve host")
        || normalized.contains("connection reset")
        || normalized.contains("connection refused")
        || normalized.contains("tls handshake")
        || normalized.contains("network")
    {
        (ErrorKind::Network, true)
    } else {
        (ErrorKind::GitHubCli, false)
    };
    let retry_after_seconds = (kind == ErrorKind::RateLimited)
        .then(|| parse_retry_after_seconds(&normalized))
        .flatten();
    runtime_exit(kind, message, retryable, retry_after_seconds)
}

fn parse_retry_after_seconds(message: &str) -> Option<u64> {
    ["retry-after:", "retry after"]
        .into_iter()
        .find_map(|marker| {
            let remainder = message.split_once(marker)?.1.trim_start();
            let digits = remainder
                .chars()
                .take_while(char::is_ascii_digit)
                .collect::<String>();
            (!digits.is_empty()).then(|| digits.parse().ok()).flatten()
        })
}

fn runtime_exit(
    kind: ErrorKind,
    message: String,
    retryable: bool,
    retry_after_seconds: Option<u64>,
) -> Exit {
    Exit::runtime(
        &RuntimeError {
            kind,
            message,
            retryable,
            retry_after_seconds,
        },
        1,
    )
}

pub(super) fn json_with_deadline<I, S>(
    args: I,
    payload: Option<&str>,
    allow_nonzero_json: bool,
    deadline: Option<Instant>,
) -> Result<Value>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut command = Command::new("gh");
    command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if payload.is_some() {
        command.stdin(Stdio::piped());
    }
    let output = match execute(command, payload, deadline) {
        Ok(output) => output,
        Err(ProcessError::TimedOut) => {
            return Err(Exit::runtime(
                &RuntimeError {
                    kind: ErrorKind::Timeout,
                    message: "GitHub CLI execution timed out".to_owned(),
                    retryable: true,
                    retry_after_seconds: None,
                },
                1,
            ));
        }
        Err(error) => return Err(Exit::message(error.to_string())),
    };
    let code = output.status.code().unwrap_or(1);
    if !output.status.success() && !allow_nonzero_json {
        return Err(Exit::child(code, &output.stderr));
    }
    match serde_json::from_slice(&output.stdout) {
        Ok(response) => Ok(response),
        Err(_error) if !output.status.success() => Err(Exit::child(code, &output.stderr)),
        Err(error) => Err(Exit::message(format!(
            "GitHub returned invalid JSON: {error}"
        ))),
    }
}

struct ProcessOutput {
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

#[derive(Debug)]
enum ProcessError {
    Io(io::Error),
    TimedOut,
    ReaderPanicked,
}

impl std::fmt::Display for ProcessError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => error.fmt(formatter),
            Self::TimedOut => formatter.write_str("GitHub CLI execution timed out"),
            Self::ReaderPanicked => formatter.write_str("failed to collect GitHub CLI output"),
        }
    }
}

fn execute(
    mut command: Command,
    payload: Option<&str>,
    deadline: Option<Instant>,
) -> std::result::Result<ProcessOutput, ProcessError> {
    let child = command.spawn().map_err(ProcessError::Io)?;
    collect_output(child, payload, deadline)
}

fn collect_output(
    mut child: Child,
    payload: Option<&str>,
    deadline: Option<Instant>,
) -> std::result::Result<ProcessOutput, ProcessError> {
    let stdout = collect_pipe(child.stdout.take().expect("stdout is piped"));
    let stderr = collect_pipe(child.stderr.take().expect("stderr is piped"));
    if let Some(payload) = payload
        && let Err(error) = write_child_stdin(&mut child, payload)
    {
        terminate_and_reap(&mut child)?;
        return Err(ProcessError::Io(error));
    }
    let status = wait_until(&mut child, deadline);
    let stdout = stdout
        .join()
        .map_err(|_| ProcessError::ReaderPanicked)?
        .map_err(ProcessError::Io)?;
    let stderr = stderr
        .join()
        .map_err(|_| ProcessError::ReaderPanicked)?
        .map_err(ProcessError::Io)?;
    let status = status?;
    Ok(ProcessOutput {
        status,
        stdout,
        stderr,
    })
}

fn collect_pipe<R>(mut pipe: R) -> thread::JoinHandle<io::Result<Vec<u8>>>
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut bytes = Vec::new();
        pipe.read_to_end(&mut bytes)?;
        Ok(bytes)
    })
}

fn wait_until(
    child: &mut Child,
    deadline: Option<Instant>,
) -> std::result::Result<ExitStatus, ProcessError> {
    let Some(deadline) = deadline else {
        return child.wait().map_err(ProcessError::Io);
    };
    loop {
        if let Some(status) = child.try_wait().map_err(ProcessError::Io)? {
            return Ok(status);
        }
        let now = Instant::now();
        if now >= deadline {
            terminate_and_reap(child)?;
            return Err(ProcessError::TimedOut);
        }
        thread::sleep(
            deadline
                .saturating_duration_since(now)
                .min(Duration::from_millis(10)),
        );
    }
}

fn terminate_and_reap(child: &mut Child) -> std::result::Result<(), ProcessError> {
    match child.kill() {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::InvalidInput => {}
        Err(error) => return Err(ProcessError::Io(error)),
    }
    child.wait().map_err(ProcessError::Io)?;
    Ok(())
}

fn write_child_stdin(child: &mut std::process::Child, payload: &str) -> io::Result<()> {
    let result = child
        .stdin
        .take()
        .expect("stdin is piped when a payload is present")
        .write_all(payload.as_bytes());
    match result {
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        result => result,
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader};

    use super::*;

    #[cfg(unix)]
    #[test]
    fn broken_stdin_still_allows_child_status_and_stderr_to_be_collected() {
        let mut child = Command::new("sh")
            .args([
                "-c",
                "exec 0<&-; printf 'ready\\n'; printf 'simulated stdin failure\\n' >&2; exit 29",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn child");

        let mut ready = String::new();
        BufReader::new(child.stdout.take().expect("stdout is piped"))
            .read_line(&mut ready)
            .expect("read synchronization marker");
        assert_eq!(ready, "ready\n");

        write_child_stdin(&mut child, "payload").expect("BrokenPipe is ignored");
        let output = child.wait_with_output().expect("collect child output");
        assert_eq!(output.status.code(), Some(29));
        assert_eq!(output.stderr, b"simulated stdin failure\n");
    }

    #[cfg(unix)]
    #[test]
    fn deadline_terminates_and_reaps_the_child() {
        let child = Command::new("sh")
            .args(["-c", "exec sleep 60"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn child");
        let pid = child.id().to_string();
        let result = collect_output(
            child,
            None,
            Some(Instant::now() + Duration::from_millis(50)),
        );

        assert!(matches!(result, Err(ProcessError::TimedOut)));
        assert!(
            !Command::new("kill")
                .args(["-0", &pid])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .expect("check child status")
                .success(),
            "timed-out child still exists"
        );
    }

    #[test]
    fn runtime_failures_are_classified_without_losing_retry_after() {
        let rate_limit = classify_failure(1, b"secondary rate limit; retry-after: 45\n");
        assert_eq!(
            rate_limit.stderr_line(),
            Some(
                r#"{"schemaVersion":1,"error":{"kind":"rateLimited","message":"secondary rate limit; retry-after: 45","retryable":true,"retryAfterSeconds":45}}"#
            )
        );

        let network = classify_failure(1, b"could not resolve host: api.github.com\n");
        assert!(
            network
                .stderr_line()
                .expect("structured network error")
                .contains(r#""kind":"network"#)
        );
    }
}
