use std::time::Instant;

use serde_json::Value;

use crate::error::Result;
use crate::model::Target;

use super::cli;

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

pub fn checks_with_deadline(
    target: &Target,
    required: bool,
    deadline: Instant,
    timeout_message: &str,
) -> Result<Value> {
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
        return cli::json_runtime_or_empty_with_deadline(
            args,
            None,
            true,
            "no required checks reported on ",
            deadline,
            timeout_message,
        );
    }
    cli::json_runtime_with_deadline(args, None, true, deadline, timeout_message)
}

pub fn head_oid(target: &Target, deadline: Instant, timeout_message: &str) -> Result<Value> {
    cli::json_runtime_with_deadline(
        [
            "pr",
            "view",
            &target.number,
            "--repo",
            &target.repository,
            "--json",
            "headRefOid",
        ],
        None,
        false,
        deadline,
        timeout_message,
    )
}

pub fn check_runs(
    target: &Target,
    head_oid: &str,
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
                "repos/{}/commits/{head_oid}/check-runs?per_page=100",
                target.repository
            ),
        ],
        None,
        false,
        deadline,
        timeout_message,
    )
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

pub fn job_log(
    target: &Target,
    job_id: u64,
    deadline: Instant,
    timeout_message: &str,
) -> Result<Vec<u8>> {
    cli::bytes_runtime_with_deadline(
        [
            "api",
            "--method",
            "GET",
            &format!("repos/{}/actions/jobs/{job_id}/logs", target.repository),
        ],
        deadline,
        timeout_message,
    )
}
