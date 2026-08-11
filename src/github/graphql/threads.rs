use serde_json::{Value, json};

use crate::error::{Exit, Result};
use crate::model::Target;

use super::super::cli;
use super::pagination;

const THREADS_QUERY: &str = r"
query ThreadSummaries($owner: String!, $name: String!, $number: Int!, $cursor: String) {
  repository(owner: $owner, name: $name) {
    pullRequest(number: $number) {
      reviewThreads(first: 100, after: $cursor) {
        nodes {
          id
          isResolved
          isOutdated
          path
          line
          originalLine
          startLine
          diffSide
          comments(first: 100) {
            nodes { id createdAt updatedAt }
            pageInfo { hasNextPage endCursor }
          }
        }
        pageInfo { hasNextPage endCursor }
      }
    }
  }
}
";

const COMMENTS_QUERY: &str = r"
query ThreadSummaryComments($id: ID!, $cursor: String) {
  node(id: $id) {
    ... on PullRequestReviewThread {
      comments(first: 100, after: $cursor) {
        nodes { id createdAt updatedAt }
        pageInfo { hasNextPage endCursor }
      }
    }
  }
}
";

pub fn execute(target: &Target, include_resolved: bool) -> Result<Vec<Value>> {
    let (owner, name) = target
        .repository
        .split_once('/')
        .expect("repository is validated");
    let mut cursor_tracker = pagination::CursorTracker::default();
    let mut threads = Vec::new();

    loop {
        let variables = variables(owner, name, &target.number, cursor_tracker.cursor())?;
        let data = query(THREADS_QUERY, &variables)?;
        let pull_request = pagination::value_at(&data, &["repository", "pullRequest"])?;
        if pull_request.is_null() {
            let message = format!(
                "pull request not found: {}#{}",
                target.repository, target.number
            );
            return Err(cli::runtime_cli_failure(1, message.as_bytes()));
        }
        let connection = pagination::value_at(pull_request, &["reviewThreads"])?;
        threads.extend(pagination::nodes(connection)?.iter().cloned());
        let Some(_) = cursor_tracker.next(connection)? else {
            break;
        };
    }

    if !include_resolved {
        threads.retain(|thread| thread.get("isResolved") != Some(&Value::Bool(true)));
    }
    for thread in &mut threads {
        append_comment_pages(thread)?;
    }
    Ok(threads)
}

fn append_comment_pages(thread: &mut Value) -> Result<()> {
    let id = thread
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| Exit::invalid_response("GitHub field id must be a string"))?
        .to_owned();
    let mut comments = thread
        .as_object_mut()
        .and_then(|thread| thread.shift_remove("comments"))
        .ok_or_else(|| Exit::invalid_response("GitHub review thread omitted comments"))?;
    let mut cursor_tracker = pagination::CursorTracker::default();
    while let Some(cursor) = cursor_tracker.next(&comments)? {
        let variables =
            serde_json::to_string(&json!({"id": id, "cursor": cursor})).map_err(|error| {
                Exit::invalid_response(format!("failed to encode GitHub request: {error}"))
            })?;
        let data = query(COMMENTS_QUERY, &variables)?;
        let page = pagination::value_at(&data, &["node", "comments"])?;
        let new_nodes = pagination::nodes(page)?.to_vec();
        comments
            .get_mut("nodes")
            .and_then(Value::as_array_mut)
            .ok_or_else(|| Exit::invalid_response("GitHub comments nodes must be an array"))?
            .extend(new_nodes);
        comments
            .as_object_mut()
            .ok_or_else(|| Exit::invalid_response("GitHub comments must be an object"))?
            .insert(
                "pageInfo".to_owned(),
                pagination::value_at(page, &["pageInfo"])?.clone(),
            );
    }
    let nodes = comments
        .as_object_mut()
        .and_then(|comments| comments.shift_remove("nodes"))
        .ok_or_else(|| Exit::invalid_response("GitHub comments nodes must be an array"))?;
    thread
        .as_object_mut()
        .expect("thread was validated as an object")
        .insert("comments".to_owned(), nodes);
    Ok(())
}

fn variables(owner: &str, name: &str, number: &str, cursor: Option<&str>) -> Result<String> {
    let owner = serde_json::to_string(owner).expect("serializing a string cannot fail");
    let name = serde_json::to_string(name).expect("serializing a string cannot fail");
    let cursor = serde_json::to_string(&cursor).map_err(|error| {
        Exit::invalid_response(format!("failed to encode GitHub request: {error}"))
    })?;
    Ok(format!(
        r#"{{"owner":{owner},"name":{name},"number":{number},"cursor":{cursor}}}"#
    ))
}

fn query(document: &str, variables: &str) -> Result<Value> {
    let document = serde_json::to_string(document).map_err(|error| {
        Exit::invalid_response(format!("failed to encode GitHub request: {error}"))
    })?;
    let payload = format!(r#"{{"query":{document},"variables":{variables}}}"#);
    let response = cli::json_runtime(["api", "graphql", "--input", "-"], Some(&payload), false)?;
    if let Some(errors) = response.get("errors") {
        let message = format!("GitHub GraphQL error: {errors}");
        return Err(cli::runtime_cli_failure(1, message.as_bytes()));
    }
    response
        .get("data")
        .cloned()
        .ok_or_else(|| Exit::invalid_response("GitHub returned a GraphQL response without data"))
}
