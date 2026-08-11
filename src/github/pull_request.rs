use std::time::Instant;

use serde_json::Value;

use crate::error::Result;
use crate::model::Target;

use super::cli;

pub use cli::BoundedBytes;

pub fn checks(target: &Target, required: bool) -> Result<Value> {
    let mut args = vec![
        "pr",
        "checks",
        &target.number,
        "--repo",
        &target.repository,
        "--json",
        "name,state,bucket,link,workflow,startedAt,completedAt",
    ];
    if required {
        args.push("--required");
        return cli::json_runtime_or_empty(args, None, true, "no required checks reported on ");
    }
    cli::json_runtime(args, None, true)
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
        false,
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
        false,
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
) -> Result<cli::BoundedBytes> {
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
