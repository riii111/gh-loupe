use std::time::Instant;

use serde_json::Value;

use crate::error::{Exit, Result, RuntimeError};
use crate::model::Target;

use super::cli;

pub use cli::BoundedBytes;

pub fn required_check_buckets(target: &Target) -> Result<Value> {
    check_buckets(target, true)
}

pub fn all_check_buckets(target: &Target) -> Result<Value> {
    check_buckets(target, false)
}

fn check_buckets(target: &Target, required: bool) -> Result<Value> {
    let checks = cli::json_runtime_or_empty(
        check_args(target, required, "bucket"),
        None,
        empty_error_prefix(required),
    )?;
    if !checks.is_array() {
        return Err(Exit::runtime(&RuntimeError::invalid_response(
            if required {
                "GitHub returned an invalid required checks response"
            } else {
                "GitHub returned an invalid all checks response"
            },
        )));
    }
    Ok(checks)
}

pub fn checks(target: &Target, required: bool) -> Result<Value> {
    cli::json_runtime_or_empty(
        check_args(
            target,
            required,
            "name,state,bucket,link,workflow,startedAt,completedAt",
        ),
        None,
        empty_error_prefix(required),
    )
}

fn check_args<'a>(target: &'a Target, required: bool, fields: &'a str) -> Vec<&'a str> {
    let mut args = vec!["pr", "checks", &target.number, "--repo", &target.repository];
    if required {
        args.push("--required");
    }
    args.extend(["--json", fields]);
    args
}

fn empty_error_prefix(required: bool) -> &'static str {
    if required {
        "no required checks reported on "
    } else {
        "no checks reported on "
    }
}

pub fn annotations(
    target: &Target,
    check_run_id: u64,
    deadline: Instant,
    timeout_message: &str,
) -> Result<Value> {
    cli::json_runtime_with_deadline(
        [
            "api",
            "--method",
            "GET",
            "--paginate",
            "--slurp",
            &format!(
                "repos/{}/check-runs/{check_run_id}/annotations?per_page=100",
                target.repository
            ),
        ],
        None,
        deadline,
        timeout_message,
    )
}

pub fn job(
    target: &Target,
    job_id: u64,
    deadline: Instant,
    timeout_message: &str,
) -> Result<Value> {
    cli::json_runtime_with_deadline(
        [
            "api",
            "--method",
            "GET",
            &format!("repos/{}/actions/jobs/{job_id}", target.repository),
        ],
        None,
        deadline,
        timeout_message,
    )
}

pub fn job_log(
    target: &Target,
    job_id: u64,
    max_bytes: usize,
    max_lines: usize,
    deadline: Instant,
    timeout_message: &str,
) -> Result<BoundedBytes> {
    cli::bytes_runtime_with_deadline(
        [
            "api",
            "--method",
            "GET",
            &format!("repos/{}/actions/jobs/{job_id}/logs", target.repository),
        ],
        max_bytes,
        max_lines,
        deadline,
        timeout_message,
    )
}
