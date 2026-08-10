use serde_json::{Value, json};

use crate::error::{ErrorKind, Exit, Result, RuntimeError};
use crate::model::Target;

use super::super::cli;

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
    let mut cursor = Value::Null;
    let mut threads = Vec::new();

    loop {
        let variables = variables(owner, name, &target.number, &cursor)?;
        let data = query(THREADS_QUERY, &variables)?;
        let pull_request = value_at(&data, &["repository", "pullRequest"])?;
        if pull_request.is_null() {
            return Err(runtime_error(
                ErrorKind::NotFound,
                format!(
                    "pull request not found: {}#{}",
                    target.repository, target.number
                ),
                false,
            ));
        }
        let connection = value_at(pull_request, &["reviewThreads"])?;
        threads.extend(nodes(connection)?.iter().cloned());
        let Some(next) = next_cursor(connection, &cursor)? else {
            break;
        };
        cursor = Value::String(next);
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
        .ok_or_else(|| invalid_response("GitHub field id must be a string"))?
        .to_owned();
    let mut comments = thread
        .as_object_mut()
        .and_then(|thread| thread.shift_remove("comments"))
        .ok_or_else(|| invalid_response("GitHub review thread omitted comments"))?;
    let mut cursor = Value::Null;
    while let Some(next) = next_cursor(&comments, &cursor)? {
        let variables =
            serde_json::to_string(&json!({"id": id, "cursor": next})).map_err(|error| {
                invalid_response(format!("failed to encode GitHub request: {error}"))
            })?;
        let data = query(COMMENTS_QUERY, &variables)?;
        let page = value_at(&data, &["node", "comments"])?;
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
        cursor = Value::String(next);
    }
    let nodes = comments
        .as_object_mut()
        .and_then(|comments| comments.shift_remove("nodes"))
        .ok_or_else(|| invalid_response("GitHub comments nodes must be an array"))?;
    thread
        .as_object_mut()
        .expect("thread was validated as an object")
        .insert("comments".to_owned(), nodes);
    Ok(())
}

fn variables(owner: &str, name: &str, number: &str, cursor: &Value) -> Result<String> {
    let owner = serde_json::to_string(owner).expect("serializing a string cannot fail");
    let name = serde_json::to_string(name).expect("serializing a string cannot fail");
    let cursor = serde_json::to_string(cursor)
        .map_err(|error| invalid_response(format!("failed to encode GitHub request: {error}")))?;
    Ok(format!(
        r#"{{"owner":{owner},"name":{name},"number":{number},"cursor":{cursor}}}"#
    ))
}

fn query(document: &str, variables: &str) -> Result<Value> {
    let document = serde_json::to_string(document)
        .map_err(|error| invalid_response(format!("failed to encode GitHub request: {error}")))?;
    let payload = format!(r#"{{"query":{document},"variables":{variables}}}"#);
    let response = cli::runtime_json(["api", "graphql", "--input", "-"], Some(&payload))?;
    if let Some(errors) = response.get("errors") {
        let message = format!("GitHub GraphQL error: {errors}");
        return Err(Exit::runtime(&cli::classify_runtime_failure(message), 1));
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

fn next_cursor(connection: &Value, previous: &Value) -> Result<Option<String>> {
    let page_info = value_at(connection, &["pageInfo"])?;
    let has_next = value_at(page_info, &["hasNextPage"])?
        .as_bool()
        .ok_or_else(|| invalid_response("GitHub pageInfo.hasNextPage must be a boolean"))?;
    if !has_next {
        return Ok(None);
    }
    let cursor = value_at(page_info, &["endCursor"])?
        .as_str()
        .filter(|cursor| !cursor.is_empty())
        .ok_or_else(|| invalid_response("GitHub pageInfo.endCursor must contain a cursor"))?;
    if previous.as_str() == Some(cursor) {
        return Err(invalid_response("GitHub pagination cursor did not advance"));
    }
    Ok(Some(cursor.to_owned()))
}

fn value_at<'a>(value: &'a Value, path: &[&str]) -> Result<&'a Value> {
    path.iter().try_fold(value, |value, key| {
        value
            .get(*key)
            .ok_or_else(|| invalid_response(format!("GitHub response omitted {key}")))
    })
}

fn invalid_response(message: impl Into<String>) -> Exit {
    cli::runtime_invalid_response(message)
}

fn runtime_error(kind: ErrorKind, message: String, retryable: bool) -> Exit {
    Exit::runtime(
        &RuntimeError {
            kind,
            message,
            retryable,
            retry_after_seconds: None,
        },
        1,
    )
}
