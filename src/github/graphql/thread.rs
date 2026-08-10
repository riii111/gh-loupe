use std::collections::HashSet;

use serde_json::{Value, json};

use crate::error::{Exit, Result, RuntimeError};
use crate::model::Target;

use super::super::cli;

const THREAD_QUERY: &str = r"
query ThreadDetail($id: ID!) {
  node(id: $id) {
    __typename
    ... on PullRequestReviewThread {
      id
      isResolved
      isOutdated
      path
      line
      originalLine
      startLine
      diffSide
      pullRequest {
        number
        repository { nameWithOwner }
      }
      comments(first: 100) {
        nodes {
          id
          url
          body
          author { login }
          createdAt
          updatedAt
          diffHunk
          replyTo { id }
        }
        pageInfo { hasNextPage endCursor }
      }
    }
  }
}
";

const COMMENTS_QUERY: &str = r"
query ThreadDetailComments($id: ID!, $cursor: String!) {
  node(id: $id) {
    __typename
    ... on PullRequestReviewThread {
      id
      comments(first: 100, after: $cursor) {
        nodes {
          id
          url
          body
          author { login }
          createdAt
          updatedAt
          diffHunk
          replyTo { id }
        }
        pageInfo { hasNextPage endCursor }
      }
    }
  }
}
";

pub fn execute(target: &Target, thread_id: &str) -> Result<Value> {
    let variables = serde_json::to_string(&json!({"id": thread_id}))
        .map_err(|error| invalid_response(format!("failed to encode GitHub request: {error}")))?;
    let data = query(THREAD_QUERY, &variables)?;
    let mut thread = review_thread_node(&data, thread_id)?.clone();
    verify_pull_request(&thread, target)?;
    thread
        .as_object_mut()
        .expect("review thread is an object")
        .shift_remove("pullRequest");
    append_comment_pages(&mut thread, thread_id)?;
    Ok(thread)
}

fn verify_pull_request(thread: &Value, target: &Target) -> Result<()> {
    let pull_request = value_at(thread, &["pullRequest"])?;
    let number = value_at(pull_request, &["number"])?
        .as_u64()
        .ok_or_else(|| invalid_response("GitHub pull request number must be an integer"))?;
    let repository = value_at(pull_request, &["repository", "nameWithOwner"])?
        .as_str()
        .ok_or_else(|| invalid_response("GitHub repository nameWithOwner must be a string"))?;
    if number.to_string() != target.number || !repository.eq_ignore_ascii_case(&target.repository) {
        return Err(not_found(format!(
            "review thread not found in {}#{}",
            target.repository, target.number
        )));
    }
    Ok(())
}

fn append_comment_pages(thread: &mut Value, thread_id: &str) -> Result<()> {
    let mut comments = thread
        .as_object_mut()
        .and_then(|thread| thread.shift_remove("comments"))
        .ok_or_else(|| invalid_response("GitHub review thread omitted comments"))?;
    let mut seen = HashSet::new();
    while let Some(cursor) = next_cursor(&comments)? {
        if !seen.insert(cursor.clone()) {
            return Err(invalid_response("GitHub pagination cursor repeated"));
        }
        let variables = serde_json::to_string(&json!({"id": thread_id, "cursor": cursor}))
            .map_err(|error| {
                invalid_response(format!("failed to encode GitHub request: {error}"))
            })?;
        let data = query(COMMENTS_QUERY, &variables)?;
        let node = review_thread_node(&data, thread_id)?;
        let page = value_at(node, &["comments"])?;
        let new_nodes = nodes(page)?.to_vec();
        comments
            .get_mut("nodes")
            .and_then(Value::as_array_mut)
            .ok_or_else(|| invalid_response("GitHub comments nodes must be an array"))?
            .extend(new_nodes);
        comments
            .as_object_mut()
            .ok_or_else(|| invalid_response("GitHub comments must be an object"))?
            .insert(
                "pageInfo".to_owned(),
                value_at(page, &["pageInfo"])?.clone(),
            );
    }
    let nodes = comments
        .as_object_mut()
        .and_then(|comments| comments.shift_remove("nodes"))
        .ok_or_else(|| invalid_response("GitHub comments nodes must be an array"))?;
    thread
        .as_object_mut()
        .expect("review thread is an object")
        .insert("comments".to_owned(), nodes);
    Ok(())
}

fn review_thread_node<'a>(data: &'a Value, thread_id: &str) -> Result<&'a Value> {
    let node = value_at(data, &["node"])?;
    if node.is_null()
        || node.get("__typename").and_then(Value::as_str) != Some("PullRequestReviewThread")
    {
        return Err(not_found(format!("review thread not found: {thread_id}")));
    }
    let id = node
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_response("GitHub review thread id must be a string"))?;
    if id != thread_id {
        return Err(invalid_response(
            "GitHub returned a different review thread",
        ));
    }
    Ok(node)
}

fn query(document: &str, variables: &str) -> Result<Value> {
    let document = serde_json::to_string(document)
        .map_err(|error| invalid_response(format!("failed to encode GitHub request: {error}")))?;
    let payload = format!(r#"{{"query":{document},"variables":{variables}}}"#);
    let response = cli::json_runtime(["api", "graphql", "--input", "-"], Some(&payload), false)?;
    if let Some(errors) = response.get("errors") {
        let message = format!("GitHub GraphQL error: {errors}");
        if message
            .to_ascii_lowercase()
            .contains("could not resolve to a node")
        {
            return Err(not_found(message));
        }
        return Err(cli::classify_failure(1, message.as_bytes()));
    }
    response
        .get("data")
        .cloned()
        .ok_or_else(|| invalid_response("GitHub returned a GraphQL response without data"))
}

fn nodes(connection: &Value) -> Result<&[Value]> {
    value_at(connection, &["nodes"])?
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| invalid_response("GitHub connection nodes must be an array"))
}

fn next_cursor(connection: &Value) -> Result<Option<String>> {
    let page_info = value_at(connection, &["pageInfo"])?;
    let has_next = value_at(page_info, &["hasNextPage"])?
        .as_bool()
        .ok_or_else(|| invalid_response("GitHub pageInfo.hasNextPage must be a boolean"))?;
    if !has_next {
        return Ok(None);
    }
    value_at(page_info, &["endCursor"])?
        .as_str()
        .filter(|cursor| !cursor.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| invalid_response("GitHub pageInfo.endCursor must contain a cursor"))
        .map(Some)
}

fn value_at<'a>(value: &'a Value, path: &[&str]) -> Result<&'a Value> {
    path.iter().try_fold(value, |value, key| {
        value
            .get(*key)
            .ok_or_else(|| invalid_response(format!("GitHub response omitted {key}")))
    })
}

fn not_found(message: impl Into<String>) -> Exit {
    Exit::runtime(&RuntimeError::not_found(message), 1)
}

fn invalid_response(message: impl Into<String>) -> Exit {
    Exit::invalid_response(message)
}
