pub mod checks;
mod cli;
pub mod graphql;
pub mod rest;

use serde_json::Value;

use crate::error::{Exit, Result, RuntimeError};

pub fn current_repository_runtime() -> Result<String> {
    let response = cli::json_runtime(["repo", "view", "--json", "nameWithOwner"], None)?;
    response
        .get("nameWithOwner")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| {
            Exit::runtime(&RuntimeError::invalid_response(
                "GitHub returned an invalid repository response",
            ))
        })
}
