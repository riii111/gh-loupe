use serde_json::Value;

use crate::error::{Exit, Result, RuntimeError};
use crate::model::Target;

use super::cli;

pub fn pull_request_checks(target: &Target) -> Result<Value> {
    let checks = cli::json(
        [
            "pr",
            "checks",
            &target.number,
            "--repo",
            &target.repository,
            "--json",
            "name,state,bucket,link,workflow,startedAt,completedAt",
        ],
        None,
        true,
    )?;
    if !checks.is_array() {
        return Err(Exit::message("GitHub returned an invalid checks response"));
    }
    Ok(checks)
}

pub fn required_check_buckets(target: &Target) -> Result<Value> {
    check_buckets(target, true)
}

pub fn all_check_buckets(target: &Target) -> Result<Value> {
    check_buckets(target, false)
}

fn check_buckets(target: &Target, required: bool) -> Result<Value> {
    let mut args = vec!["pr", "checks", &target.number, "--repo", &target.repository];
    if required {
        args.push("--required");
    }
    args.extend(["--json", "bucket"]);
    let checks = if required {
        cli::json_runtime_or_empty(args, None, true, "no required checks reported on ")?
    } else {
        cli::json_runtime(args, None, true)?
    };
    if !checks.is_array() {
        return Err(Exit::runtime(
            &RuntimeError::invalid_response(if required {
                "GitHub returned an invalid required checks response"
            } else {
                "GitHub returned an invalid all checks response"
            }),
            1,
        ));
    }
    Ok(checks)
}

pub fn issue(target: &Target) -> Result<Value> {
    let issue = cli::json(
        [
            "api",
            &format!("repos/{}/issues/{}", target.repository, target.number),
        ],
        None,
        false,
    )?;
    if !issue.is_object() {
        return Err(Exit::message("GitHub returned an invalid issue response"));
    }
    Ok(issue)
}

pub fn pages(endpoint: &str) -> Result<Vec<Value>> {
    let pages = cli::json(
        ["api", "--method", "GET", "--paginate", "--slurp", endpoint],
        None,
        false,
    )?;
    let pages = pages
        .as_array()
        .ok_or_else(|| Exit::message("GitHub returned an invalid paginated response"))?;
    if pages.iter().any(|page| !page.is_array()) {
        return Err(Exit::message(
            "GitHub returned an invalid paginated response",
        ));
    }
    let items = pages
        .iter()
        .flat_map(|page| page.as_array().expect("validated above").iter().cloned())
        .collect::<Vec<_>>();
    if items.iter().any(|item| !item.is_object()) {
        return Err(Exit::message("GitHub returned invalid paginated items"));
    }
    Ok(items)
}
