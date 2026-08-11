use std::time::Instant;

use serde_json::{Value, json};

use crate::error::{Exit, Result, RuntimeError};
use crate::model::Target;

use super::cli;

mod pagination;
pub mod review_thread;
pub mod review_threads;

const OVERVIEW_QUERY: &str = r"
query($owner: String!, $name: String!, $number: Int!, $cursor: String) {
  repository(owner: $owner, name: $name) {
    pullRequest(number: $number) {
      number
      title
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
    let mut cursor_tracker = pagination::CursorTracker::default();
    let mut contexts = Vec::new();
    let mut head_oid = None;
    let number = target
        .number
        .parse::<i32>()
        .map_err(|_| invalid_graphql_response())?;

    loop {
        let variables = json!({
            "owner": owner,
            "name": name,
            "number": number,
            "cursor": cursor_tracker.cursor(),
        });
        let data = query_runtime_with_deadline(
            CHECK_CONTEXTS_QUERY,
            &variables,
            deadline,
            timeout_message,
        )?;
        let pull_request = pagination::value_at(&data, &["repository", "pullRequest"])?;
        if pull_request.is_null() {
            return Err(Exit::runtime(
                &RuntimeError::not_found(format!(
                    "pull request not found: {}#{}",
                    target.repository, target.number
                )),
                1,
            ));
        }
        let commit = pagination::nodes(pagination::value_at(pull_request, &["commits"])?)?
            .first()
            .and_then(|node| node.get("commit"))
            .ok_or_else(invalid_graphql_response)?;
        let current_oid = pagination::value_at(commit, &["oid"])?
            .as_str()
            .ok_or_else(invalid_graphql_response)?;
        if head_oid
            .as_deref()
            .is_some_and(|expected| expected != current_oid)
        {
            return Err(invalid_graphql_response());
        }
        head_oid = Some(current_oid.to_owned());
        let rollup = pagination::value_at(commit, &["statusCheckRollup"])?;
        if rollup.is_null() {
            return Ok((current_oid.to_owned(), contexts));
        }
        let connection = pagination::value_at(rollup, &["contexts"])?;
        contexts.extend(pagination::nodes(connection)?.iter().cloned());
        if cursor_tracker.next(connection)?.is_none() {
            return Ok((current_oid.to_owned(), contexts));
        }
    }
}

pub fn pull_request_overview(target: &Target) -> Result<(Value, usize)> {
    let (owner, name) = target
        .repository
        .split_once('/')
        .expect("repository is validated");
    let mut cursor_tracker = pagination::CursorTracker::default();
    let mut unresolved = 0;

    let pull_request = loop {
        let owner = serde_json::to_string(owner).expect("serializing a string cannot fail");
        let name = serde_json::to_string(name).expect("serializing a string cannot fail");
        let cursor_json = serde_json::to_string(&cursor_tracker.cursor())
            .expect("GraphQL cursor variables are always serializable");
        let variables = format!(
            r#"{{"owner":{owner},"name":{name},"number":{},"cursor":{cursor_json}}}"#,
            target.number
        );
        let data = query_runtime(OVERVIEW_QUERY, &variables)?;
        let current = pagination::value_at(&data, &["repository", "pullRequest"])?;
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
        let nodes = pagination::nodes(&connection)?;
        for node in nodes {
            let is_resolved = node
                .get("isResolved")
                .and_then(Value::as_bool)
                .ok_or_else(invalid_graphql_response)?;
            unresolved += usize::from(!is_resolved);
        }
        if cursor_tracker.next(&connection)?.is_none() {
            validate_overview_fields(&current)?;
            break current;
        }
    };

    Ok((pull_request, unresolved))
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
    let fields: [(&str, FieldValidator); 9] = [
        ("number", Value::is_u64),
        ("title", Value::is_string),
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

fn invalid_graphql_response() -> Exit {
    Exit::runtime(
        &RuntimeError::invalid_response("GitHub returned an invalid GraphQL response"),
        1,
    )
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
