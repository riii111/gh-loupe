use serde_json::Value;

use crate::error::{Exit, Result};
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
