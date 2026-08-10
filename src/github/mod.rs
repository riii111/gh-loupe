mod cli;
pub mod graphql;
pub mod pull_request;
pub mod rest;

use serde_json::Value;

use crate::error::{ErrorKind, Exit, Result, RuntimeError};

pub fn current_repository() -> Result<String> {
    let response = cli::json(["repo", "view", "--json", "nameWithOwner"], None, false)?;
    response
        .get("nameWithOwner")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| Exit::message("GitHub returned an invalid repository response"))
}

pub fn current_repository_runtime() -> Result<String> {
    let response = cli::json_runtime(["repo", "view", "--json", "nameWithOwner"], None, false)?;
    response
        .get("nameWithOwner")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| {
            Exit::runtime(
                &RuntimeError {
                    kind: ErrorKind::InvalidResponse,
                    message: "GitHub returned an invalid repository response".to_owned(),
                    retryable: false,
                    retry_after_seconds: None,
                },
                1,
            )
        })
}
