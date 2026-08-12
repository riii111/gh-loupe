use std::time::Instant;

use serde_json::{Value, json};

use crate::error::{ErrorKind, Exit, Result, RuntimeError};
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

pub(super) enum QueryResponse {
    Data(Value),
    Errors(Value),
}

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
        let data =
            query_with_deadline(CHECK_CONTEXTS_QUERY, &variables, deadline, timeout_message)?;
        let pull_request = pagination::take_value_at(data, &["repository", "pullRequest"])?;
        if pull_request.is_null() {
            return Err(Exit::runtime(&RuntimeError::not_found(format!(
                "pull request not found: {}#{}",
                target.repository, target.number
            ))));
        }
        let mut commits = pagination::take_value_at(pull_request, &["commits"])?;
        let commit = pagination::take_nodes(&mut commits)?
            .into_iter()
            .next()
            .and_then(|mut node| node.as_object_mut()?.shift_remove("commit"))
            .ok_or_else(invalid_graphql_response)?;
        let current_oid = pagination::value_at(&commit, &["oid"])?
            .as_str()
            .ok_or_else(invalid_graphql_response)?
            .to_owned();
        if head_oid
            .as_deref()
            .is_some_and(|expected| expected != current_oid.as_str())
        {
            return Err(invalid_graphql_response());
        }
        head_oid = Some(current_oid.clone());
        let rollup = pagination::take_value_at(commit, &["statusCheckRollup"])?;
        if rollup.is_null() {
            return Ok((current_oid, contexts));
        }
        let mut connection = pagination::take_value_at(rollup, &["contexts"])?;
        contexts.extend(pagination::take_nodes(&mut connection)?);
        if cursor_tracker.next(&connection)?.is_none() {
            return Ok((current_oid, contexts));
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
    let number = target
        .number
        .parse::<i32>()
        .map_err(|_| invalid_graphql_response())?;

    let pull_request = loop {
        let variables = json!({
            "owner": owner,
            "name": name,
            "number": number,
            "cursor": cursor_tracker.cursor(),
        });
        let data = query(OVERVIEW_QUERY, &variables)?;
        let mut current = pagination::take_value_at(data, &["repository", "pullRequest"])?;
        if current.is_null() {
            return Err(Exit::runtime(&RuntimeError::not_found(format!(
                "pull request not found: {}#{}",
                target.repository, target.number
            ))));
        }
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

pub(super) fn query(document: &str, variables: &Value) -> Result<Value> {
    match execute_query(document, variables, None)? {
        QueryResponse::Data(data) => Ok(data),
        QueryResponse::Errors(errors) => Err(graphql_error(&errors)),
    }
}

pub(super) fn query_with_errors(document: &str, variables: &Value) -> Result<QueryResponse> {
    execute_query(document, variables, None)
}

pub(super) fn query_with_deadline(
    document: &str,
    variables: &Value,
    deadline: Instant,
    timeout_message: &str,
) -> Result<Value> {
    match execute_query(document, variables, Some((deadline, timeout_message)))? {
        QueryResponse::Data(data) => Ok(data),
        QueryResponse::Errors(errors) => Err(graphql_error(&errors)),
    }
}

fn execute_query(
    document: &str,
    variables: &Value,
    deadline: Option<(Instant, &str)>,
) -> Result<QueryResponse> {
    let payload = serde_json::to_string(&json!({
        "query": document,
        "variables": variables,
    }))
    .map_err(|error| Exit::invalid_response(format!("failed to encode GitHub request: {error}")))?;
    let response = match deadline {
        Some((deadline, timeout_message)) => cli::json_runtime_with_deadline(
            ["api", "graphql", "--input", "-"],
            Some(&payload),
            deadline,
            timeout_message,
        )?,
        None => cli::json_runtime(["api", "graphql", "--input", "-"], Some(&payload))?,
    };
    graphql_response(response)
}

fn graphql_response(response: Value) -> Result<QueryResponse> {
    let Value::Object(mut response) = response else {
        return Err(Exit::runtime(&RuntimeError::invalid_response(
            "GitHub returned a GraphQL response without data",
        )));
    };
    if let Some(errors) = response.shift_remove("errors") {
        return Ok(QueryResponse::Errors(errors));
    }
    response
        .shift_remove("data")
        .map(QueryResponse::Data)
        .ok_or_else(|| {
            Exit::runtime(&RuntimeError::invalid_response(
                "GitHub returned a GraphQL response without data",
            ))
        })
}

pub(super) fn graphql_error(errors: &Value) -> Exit {
    Exit::runtime(&RuntimeError {
        kind: ErrorKind::GitHubCli,
        message: graphql_error_message(errors),
        retryable: false,
        retry_after_seconds: None,
    })
}

pub(super) fn graphql_error_message(errors: &Value) -> String {
    format!(
        "GitHub GraphQL error: {}",
        serde_json::to_string(errors).expect("GraphQL error values are always serializable")
    )
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

pub(super) fn invalid_graphql_response() -> Exit {
    Exit::runtime(&RuntimeError::invalid_response(
        "GitHub returned an invalid GraphQL response",
    ))
}
