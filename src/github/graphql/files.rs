use serde_json::{Value, json};

use crate::error::{Exit, Result, RuntimeError};
use crate::model::Target;

use super::{invalid_graphql_response, pagination, query};

const FILES_QUERY: &str = r"
query PullRequestFiles($owner: String!, $name: String!, $number: Int!, $limit: Int!) {
  repository(owner: $owner, name: $name) {
    pullRequest(number: $number) {
      changedFiles
      additions
      deletions
      files(first: $limit) {
        nodes {
          path
          changeType
          additions
          deletions
        }
        totalCount
        pageInfo { hasNextPage endCursor }
      }
    }
  }
}
";

pub fn pull_request_files(target: &Target, limit: usize) -> Result<Value> {
    let (owner, name) = target
        .repository
        .split_once('/')
        .expect("repository is validated");
    let number = target
        .number
        .parse::<i32>()
        .map_err(|_| invalid_graphql_response())?;
    let data = query(
        FILES_QUERY,
        &json!({
            "owner": owner,
            "name": name,
            "number": number,
            "limit": limit,
        }),
    )?;
    let pull_request = pagination::take_value_at(data, &["repository", "pullRequest"])?;
    if pull_request.is_null() {
        return Err(Exit::runtime(&RuntimeError::not_found(format!(
            "pull request not found: {}#{}",
            target.repository, target.number
        ))));
    }
    validate_response(&pull_request)?;
    Ok(pull_request)
}

fn validate_response(pull_request: &Value) -> Result<()> {
    let pull_request = pull_request
        .as_object()
        .ok_or_else(invalid_graphql_response)?;
    for field in ["changedFiles", "additions", "deletions"] {
        if !pull_request.get(field).is_some_and(Value::is_u64) {
            return Err(invalid_graphql_response());
        }
    }
    let files = pull_request
        .get("files")
        .and_then(Value::as_object)
        .ok_or_else(invalid_graphql_response)?;
    if !files.get("totalCount").is_some_and(Value::is_u64) {
        return Err(invalid_graphql_response());
    }
    let nodes = files
        .get("nodes")
        .and_then(Value::as_array)
        .ok_or_else(invalid_graphql_response)?;
    for node in nodes {
        let node = node.as_object().ok_or_else(invalid_graphql_response)?;
        if !node.get("path").is_some_and(Value::is_string)
            || !node.get("changeType").is_some_and(Value::is_string)
            || !node.get("additions").is_some_and(Value::is_u64)
            || !node.get("deletions").is_some_and(Value::is_u64)
        {
            return Err(invalid_graphql_response());
        }
    }
    let page_info = files
        .get("pageInfo")
        .and_then(Value::as_object)
        .ok_or_else(invalid_graphql_response)?;
    let has_next_page = page_info
        .get("hasNextPage")
        .and_then(Value::as_bool)
        .ok_or_else(invalid_graphql_response)?;
    if !has_next_page {
        return Ok(());
    }
    match page_info.get("endCursor") {
        Some(Value::String(cursor)) if !cursor.is_empty() => Ok(()),
        _ => Err(invalid_graphql_response()),
    }
}
