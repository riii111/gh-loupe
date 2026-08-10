pub mod graphql;
mod process;
pub mod rest;

use serde_json::Value;

use crate::error::{Exit, Result};

pub fn current_repository() -> Result<String> {
    let response = process::json(["repo", "view", "--json", "nameWithOwner"], None, false)?;
    response
        .get("nameWithOwner")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| Exit::message("GitHub returned an invalid repository response"))
}
