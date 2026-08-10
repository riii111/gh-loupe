mod cli;
pub mod graphql;
pub mod rest;

use serde_json::Value;

use crate::error::{Exit, Result};

pub fn current_repository() -> Result<String> {
    let response = cli::json(["repo", "view", "--json", "nameWithOwner"], None, false)?;
    response
        .get("nameWithOwner")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| Exit::message("GitHub returned an invalid repository response"))
}

pub fn current_repository_runtime() -> Result<String> {
    let response = cli::runtime_json(["repo", "view", "--json", "nameWithOwner"], None)?;
    response
        .get("nameWithOwner")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| {
            cli::runtime_invalid_response("GitHub returned an invalid repository response")
        })
}
