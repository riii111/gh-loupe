use serde_json::{Value, json};

use crate::error::{Exit, Result};
use crate::model::Target;

use super::cli;

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
