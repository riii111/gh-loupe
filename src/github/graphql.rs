use std::time::Instant;

use serde_json::{Value, json};

use crate::error::{Exit, Result, RuntimeError};
use crate::model::Target;

use super::cli;

pub mod threads;

const THREADS_QUERY: &str = r"
query($owner: String!, $name: String!, $number: Int!, $cursor: String) {
  repository(owner: $owner, name: $name) {
    pullRequest(number: $number) {
      number
      url
      title
      state
      isDraft
      headRefOid
      baseRefOid
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
          resolvedBy { login }
          comments(first: 100) {
            nodes {
              id
              databaseId
              url
              body
              author { login }
              createdAt
              updatedAt
              path
              line
              originalLine
              diffHunk
              replyTo { id }
            }
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
query($id: ID!, $cursor: String) {
  node(id: $id) {
    ... on PullRequestReviewThread {
      comments(first: 100, after: $cursor) {
        nodes {
          id
          databaseId
          url
          body
          author { login }
          createdAt
          updatedAt
          path
          line
          originalLine
          diffHunk
          replyTo { id }
        }
        pageInfo { hasNextPage endCursor }
      }
    }
  }
}
";

const OVERVIEW_QUERY: &str = r"
query($owner: String!, $name: String!, $number: Int!, $cursor: String) {
  repository(owner: $owner, name: $name) {
    pullRequest(number: $number) {
      number
      url
      state
      isDraft
      headRefOid
      baseRefOid
      reviewDecision
      mergeStateStatus
      reviewThreads(first: 100, after: $cursor) {
        nodes { isResolved }
        pageInfo { hasNextPage endCursor }
      }
    }
  }
}
";

const CHECK_CONTEXTS_QUERY: &str = r"
query($owner: String!, $name: String!, $number: Int!, $cursor: String) {
  repository(owner: $owner, name: $name) {
    pullRequest(number: $number) {
      commits(last: 1) {
        nodes {
          commit {
            oid
            statusCheckRollup {
              contexts(first: 100, after: $cursor) {
                nodes {
                  __typename
                  ... on CheckRun {
                    databaseId
                    name
                    isRequired(pullRequestNumber: $number)
                    status
                    conclusion
                    startedAt
                    completedAt
                    detailsUrl
                    checkSuite {
                      workflowRun {
                        event
                        workflow { name }
                      }
                    }
                  }
                  ... on StatusContext {
                    context
                    isRequired(pullRequestNumber: $number)
                    state
                    targetUrl
                    description
                  }
                }
                pageInfo { hasNextPage endCursor }
              }
            }
          }
        }
      }
    }
  }
}
";

type FieldValidator = fn(&Value) -> bool;

pub fn pull_request_check_contexts(
    target: &Target,
    deadline: Instant,
    timeout_message: &str,
) -> Result<(String, Vec<Value>)> {
    let (owner, name) = target
        .repository
        .split_once('/')
        .expect("repository is validated");
    let mut cursor = Value::Null;
    let mut contexts = Vec::new();
    let mut head_oid = None;
    let number = target
        .number
        .parse::<u64>()
        .expect("pull request number is validated");

    loop {
        let variables = json!({
            "owner": owner,
            "name": name,
            "number": number,
            "cursor": cursor,
        });
        let data = query_runtime_with_deadline(
            CHECK_CONTEXTS_QUERY,
            &variables,
            deadline,
            timeout_message,
        )?;
        let pull_request = value_at_runtime(&data, &["repository", "pullRequest"])?;
        if pull_request.is_null() {
            return Err(Exit::runtime(
                &RuntimeError::not_found(format!(
                    "pull request not found: {}#{}",
                    target.repository, target.number
                )),
                1,
            ));
        }
        let nodes = value_at_runtime(pull_request, &["commits", "nodes"])?
            .as_array()
            .ok_or_else(invalid_graphql_response)?;
        let commit = nodes
            .first()
            .and_then(|node| node.get("commit"))
            .ok_or_else(invalid_graphql_response)?;
        let current_oid = value_at_runtime(commit, &["oid"])?
            .as_str()
            .ok_or_else(invalid_graphql_response)?;
        if head_oid
            .as_deref()
            .is_some_and(|expected| expected != current_oid)
        {
            return Err(invalid_graphql_response());
        }
        head_oid = Some(current_oid.to_owned());
        let rollup = value_at_runtime(commit, &["statusCheckRollup"])?;
        if rollup.is_null() {
            return Ok((current_oid.to_owned(), contexts));
        }
        let connection = value_at_runtime(rollup, &["contexts"])?;
        contexts.extend(
            value_at_runtime(connection, &["nodes"])?
                .as_array()
                .ok_or_else(invalid_graphql_response)?
                .iter()
                .cloned(),
        );
        let page_info = value_at_runtime(connection, &["pageInfo"])?;
        let has_next_page = value_at_runtime(page_info, &["hasNextPage"])?
            .as_bool()
            .ok_or_else(invalid_graphql_response)?;
        if !has_next_page {
            return Ok((current_oid.to_owned(), contexts));
        }
        cursor = value_at_runtime(page_info, &["endCursor"])?.clone();
        if !cursor.is_string() {
            return Err(invalid_graphql_response());
        }
    }
}

pub fn pull_request_threads(
    target: &Target,
    include_resolved: bool,
) -> Result<(Value, Vec<Value>)> {
    let (owner, name) = target
        .repository
        .split_once('/')
        .expect("repository is validated");
    let mut cursor = Value::Null;
    let mut threads = Vec::new();

    let pull_request = loop {
        let owner = serde_json::to_string(owner).expect("serializing a string cannot fail");
        let name = serde_json::to_string(name).expect("serializing a string cannot fail");
        let cursor_json = serde_json::to_string(&cursor)
            .map_err(|error| Exit::message(format!("failed to encode GitHub request: {error}")))?;
        let variables = format!(
            r#"{{"owner":{owner},"name":{name},"number":{},"cursor":{cursor_json}}}"#,
            target.number
        );
        let data = query(THREADS_QUERY, &variables)?;
        let current = value_at(&data, &["repository", "pullRequest"])?;
        if current.is_null() {
            return Err(Exit::message(format!(
                "pull request not found: {}#{}",
                target.repository, target.number
            )));
        }
        let mut current = current.clone();
        let connection = current
            .as_object_mut()
            .and_then(|current| current.shift_remove("reviewThreads"))
            .ok_or_else(|| Exit::message("GitHub returned an invalid GraphQL response"))?;
        threads.extend(
            value_at(&connection, &["nodes"])?
                .as_array()
                .ok_or_else(|| Exit::message("GitHub returned an invalid GraphQL response"))?
                .iter()
                .cloned(),
        );
        if !value_at(&connection, &["pageInfo", "hasNextPage"])?
            .as_bool()
            .ok_or_else(|| Exit::message("GitHub returned an invalid GraphQL response"))?
        {
            break current;
        }
        cursor = value_at(&connection, &["pageInfo", "endCursor"])?.clone();
    };

    if !include_resolved {
        threads.retain(|thread| thread.get("isResolved") != Some(&Value::Bool(true)));
    }

    for thread in &mut threads {
        append_comment_pages(thread)?;
    }

    Ok((pull_request, threads))
}

pub fn pull_request_overview(target: &Target) -> Result<(Value, usize)> {
    let (owner, name) = target
        .repository
        .split_once('/')
        .expect("repository is validated");
    let mut cursor = Value::Null;
    let mut unresolved = 0;

    let pull_request = loop {
        let owner = serde_json::to_string(owner).expect("serializing a string cannot fail");
        let name = serde_json::to_string(name).expect("serializing a string cannot fail");
        let cursor_json = serde_json::to_string(&cursor).map_err(|error| {
            Exit::runtime(
                &RuntimeError::invalid_response(format!(
                    "failed to encode GitHub request: {error}"
                )),
                1,
            )
        })?;
        let variables = format!(
            r#"{{"owner":{owner},"name":{name},"number":{},"cursor":{cursor_json}}}"#,
            target.number
        );
        let data = query_runtime(OVERVIEW_QUERY, &variables)?;
        let current = value_at_runtime(&data, &["repository", "pullRequest"])?;
        if current.is_null() {
            return Err(Exit::runtime(
                &RuntimeError::not_found(format!(
                    "pull request not found: {}#{}",
                    target.repository, target.number
                )),
                1,
            ));
        }
        let mut current = current.clone();
        let connection = current
            .as_object_mut()
            .and_then(|current| current.shift_remove("reviewThreads"))
            .ok_or_else(invalid_graphql_response)?;
        let nodes = value_at_runtime(&connection, &["nodes"])?
            .as_array()
            .ok_or_else(invalid_graphql_response)?;
        for node in nodes {
            let is_resolved = node
                .get("isResolved")
                .and_then(Value::as_bool)
                .ok_or_else(invalid_graphql_response)?;
            unresolved += usize::from(!is_resolved);
        }
        let page_info = value_at_runtime(&connection, &["pageInfo"])?;
        let has_next_page = value_at_runtime(page_info, &["hasNextPage"])?
            .as_bool()
            .ok_or_else(invalid_graphql_response)?;
        if !has_next_page {
            validate_overview_fields(&current)?;
            break current;
        }
        cursor = value_at_runtime(page_info, &["endCursor"])?.clone();
        if !cursor.is_string() {
            return Err(invalid_graphql_response());
        }
    };

    Ok((pull_request, unresolved))
}

fn append_comment_pages(thread: &mut Value) -> Result<()> {
    let id = thread
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| Exit::message("GitHub returned an invalid GraphQL response"))?
        .to_owned();
    let comments = thread
        .as_object_mut()
        .and_then(|thread| thread.get_mut("comments"))
        .ok_or_else(|| Exit::message("GitHub returned an invalid GraphQL response"))?;
    let mut cursor = value_at(comments, &["pageInfo", "endCursor"])?.clone();
    while value_at(comments, &["pageInfo", "hasNextPage"])?
        .as_bool()
        .ok_or_else(|| Exit::message("GitHub returned an invalid GraphQL response"))?
    {
        let variables = serde_json::to_string(&json!({"id": id, "cursor": cursor}))
            .map_err(|error| Exit::message(format!("failed to encode GitHub request: {error}")))?;
        let data = query(COMMENTS_QUERY, &variables)?;
        let page = value_at(&data, &["node", "comments"])?;
        let nodes = value_at(page, &["nodes"])?
            .as_array()
            .ok_or_else(|| Exit::message("GitHub returned an invalid GraphQL response"))?
            .clone();
        comments
            .get_mut("nodes")
            .and_then(Value::as_array_mut)
            .ok_or_else(|| Exit::message("GitHub returned an invalid GraphQL response"))?
            .extend(nodes);
        comments
            .as_object_mut()
            .expect("comments is an object")
            .insert(
                "pageInfo".to_owned(),
                value_at(page, &["pageInfo"])?.clone(),
            );
        cursor = value_at(page, &["pageInfo", "endCursor"])?.clone();
    }
    let nodes = comments
        .as_object_mut()
        .and_then(|comments| comments.shift_remove("nodes"))
        .ok_or_else(|| Exit::message("GitHub returned an invalid GraphQL response"))?;
    thread
        .as_object_mut()
        .expect("thread is an object")
        .insert("comments".to_owned(), nodes);
    Ok(())
}

fn query(query: &str, variables: &str) -> Result<Value> {
    let query = serde_json::to_string(query)
        .map_err(|error| Exit::message(format!("failed to encode GitHub request: {error}")))?;
    let payload = format!(r#"{{"query":{query},"variables":{variables}}}"#);
    let response = cli::json(["api", "graphql", "--input", "-"], Some(&payload), false)?;
    if let Some(errors) = response.get("errors") {
        return Err(Exit::child(1, format_graphql_errors(errors).as_bytes()));
    }
    response
        .get("data")
        .cloned()
        .ok_or_else(|| Exit::message("GitHub returned a GraphQL response without data"))
}

fn query_runtime(query: &str, variables: &str) -> Result<Value> {
    let query = serde_json::to_string(query).expect("GraphQL documents are always serializable");
    let payload = format!(r#"{{"query":{query},"variables":{variables}}}"#);
    let response = cli::json_runtime(["api", "graphql", "--input", "-"], Some(&payload), false)?;
    if let Some(errors) = response.get("errors") {
        let message = format_graphql_errors(errors);
        return Err(Exit::runtime(
            &RuntimeError::from_cli_failure(message.as_bytes()),
            1,
        ));
    }
    response.get("data").cloned().ok_or_else(|| {
        Exit::runtime(
            &RuntimeError::invalid_response("GitHub returned a GraphQL response without data"),
            1,
        )
    })
}

fn query_runtime_with_deadline(
    query: &str,
    variables: &Value,
    deadline: Instant,
    timeout_message: &str,
) -> Result<Value> {
    let payload = serde_json::to_string(&json!({
        "query": query,
        "variables": variables,
    }))
    .expect("fixed GraphQL requests are always serializable");
    let response = cli::json_runtime_with_deadline(
        ["api", "graphql", "--input", "-"],
        Some(&payload),
        false,
        deadline,
        timeout_message,
    )?;
    if let Some(errors) = response.get("errors") {
        let message = format_graphql_errors(errors);
        return Err(Exit::runtime(
            &RuntimeError::from_cli_failure(message.as_bytes()),
            1,
        ));
    }
    response.get("data").cloned().ok_or_else(|| {
        Exit::runtime(
            &RuntimeError::invalid_response("GitHub returned a GraphQL response without data"),
            1,
        )
    })
}

fn validate_overview_fields(pull_request: &Value) -> Result<()> {
    let fields: [(&str, FieldValidator); 8] = [
        ("number", Value::is_u64),
        ("url", Value::is_string),
        ("state", string_or_null),
        ("isDraft", bool_or_null),
        ("headRefOid", string_or_null),
        ("baseRefOid", string_or_null),
        ("reviewDecision", string_or_null),
        ("mergeStateStatus", string_or_null),
    ];
    for (field, valid) in fields {
        let value = pull_request
            .get(field)
            .ok_or_else(invalid_graphql_response)?;
        if !valid(value) {
            return Err(invalid_graphql_response());
        }
    }
    Ok(())
}

fn string_or_null(value: &Value) -> bool {
    value.is_string() || value.is_null()
}

fn bool_or_null(value: &Value) -> bool {
    value.is_boolean() || value.is_null()
}

fn value_at_runtime<'a>(value: &'a Value, path: &[&str]) -> Result<&'a Value> {
    path.iter().try_fold(value, |value, key| {
        value.get(*key).ok_or_else(invalid_graphql_response)
    })
}

fn invalid_graphql_response() -> Exit {
    Exit::runtime(
        &RuntimeError::invalid_response("GitHub returned an invalid GraphQL response"),
        1,
    )
}

fn value_at<'a>(value: &'a Value, path: &[&str]) -> Result<&'a Value> {
    path.iter().try_fold(value, |value, key| {
        value
            .get(*key)
            .ok_or_else(|| Exit::message("GitHub returned an invalid GraphQL response"))
    })
}

fn format_graphql_errors(value: &Value) -> String {
    match value {
        Value::Null => "null".to_owned(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => {
            serde_json::to_string(value).expect("serializing a string cannot fail")
        }
        Value::Array(values) => {
            let values = values
                .iter()
                .map(format_graphql_errors)
                .collect::<Vec<_>>()
                .join(", ");
            format!("[{values}]")
        }
        Value::Object(values) => {
            let values = values
                .iter()
                .map(|(key, value)| {
                    format!(
                        "{}: {}",
                        serde_json::to_string(key).expect("serializing a string cannot fail"),
                        format_graphql_errors(value)
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("{{{values}}}")
        }
    }
}
