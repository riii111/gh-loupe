use std::collections::HashMap;

use serde_json::{Value, json};

use crate::error::{Exit, Result, RuntimeError};
use crate::model::Target;

use super::pagination;

const REVIEW_THREAD_DETAILS_QUERY: &str = r"
query ReviewThreadDetails($ids: [ID!]!) {
  nodes(ids: $ids) {
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

const REVIEW_THREAD_DETAIL_COMMENTS_QUERY: &str = r"
query ReviewThreadDetailComments($id: ID!, $cursor: String!) {
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

pub fn execute(target: &Target, review_thread_ids: &[String]) -> Result<Vec<Value>> {
    let context = format!("review thread IDs {}", review_thread_ids.join(", "));
    let variables = json!({"ids": review_thread_ids});
    let data = query(REVIEW_THREAD_DETAILS_QUERY, &variables, &context)?;
    let nodes = pagination::value_at(&data, &["nodes"])
        .map_err(|error| with_context(error, &context))?
        .as_array()
        .ok_or_else(|| invalid_response_with_context("GitHub nodes must be an array", &context))?;

    let mut details = HashMap::with_capacity(nodes.len());
    for (index, node) in nodes.iter().enumerate() {
        let id = review_thread_node_from_nodes(
            node,
            review_thread_ids.get(index).map(String::as_str),
            review_thread_ids,
        )?;
        if details.insert(id.clone(), node.clone()).is_some() {
            return Err(invalid_response_with_id(
                "GitHub returned a duplicate review thread node",
                &id,
            ));
        }
    }

    let mut ordered_details = Vec::with_capacity(review_thread_ids.len());
    for review_thread_id in review_thread_ids {
        let mut detail = details
            .remove(review_thread_id)
            .ok_or_else(|| not_found(format!("review thread not found: {review_thread_id}")))?;
        verify_pull_request(&detail, target, review_thread_id)?;
        detail
            .as_object_mut()
            .expect("review thread was validated as an object")
            .shift_remove("pullRequest");
        append_detail_comment_pages(&mut detail, review_thread_id)?;
        ordered_details.push(detail);
    }
    Ok(ordered_details)
}

fn review_thread_node_from_nodes(
    node: &Value,
    null_node_id: Option<&str>,
    requested_ids: &[String],
) -> Result<String> {
    let expected_context = null_node_id
        .map(|id| format!(" for review thread {id}"))
        .unwrap_or_default();
    let node = node.as_object().ok_or_else(|| match node {
        Value::Null => not_found(format!(
            "review thread not found: {}",
            null_node_id.unwrap_or("unknown")
        )),
        _ => invalid_response_with_context(
            &format!("GitHub review thread node must be an object{expected_context}"),
            &requested_ids.join(", "),
        ),
    })?;
    if node.get("__typename").and_then(Value::as_str) != Some("PullRequestReviewThread") {
        return Err(not_found(format!(
            "review thread not found: {}",
            null_node_id.unwrap_or(&requested_ids.join(", "))
        )));
    }
    let id = node.get("id").and_then(Value::as_str).ok_or_else(|| {
        invalid_response_with_context(
            &format!("GitHub review thread id must be a string{expected_context}"),
            &requested_ids.join(", "),
        )
    })?;
    if !requested_ids.iter().any(|requested_id| requested_id == id) {
        return Err(invalid_response_with_context(
            &format!("GitHub returned an unexpected review thread id: {id}"),
            &requested_ids.join(", "),
        ));
    }
    Ok(id.to_owned())
}

fn verify_pull_request(
    review_thread: &Value,
    target: &Target,
    review_thread_id: &str,
) -> Result<()> {
    let pull_request = pagination::value_at(review_thread, &["pullRequest"])
        .map_err(|error| with_context(error, &format!("review thread {review_thread_id}")))?;
    let number = pagination::value_at(pull_request, &["number"])
        .map_err(|error| with_context(error, &format!("review thread {review_thread_id}")))?
        .as_u64()
        .ok_or_else(|| {
            invalid_response_with_id(
                "GitHub pull request number must be an integer",
                review_thread_id,
            )
        })?;
    let repository = pagination::value_at(pull_request, &["repository", "nameWithOwner"])
        .map_err(|error| with_context(error, &format!("review thread {review_thread_id}")))?
        .as_str()
        .ok_or_else(|| {
            invalid_response_with_id(
                "GitHub repository nameWithOwner must be a string",
                review_thread_id,
            )
        })?;
    if number.to_string() != target.number || !repository.eq_ignore_ascii_case(&target.repository) {
        return Err(not_found(format!(
            "review thread {review_thread_id} not found in {}#{}",
            target.repository, target.number
        )));
    }
    Ok(())
}

fn append_detail_comment_pages(detail: &mut Value, review_thread_id: &str) -> Result<()> {
    let mut comments = detail
        .as_object_mut()
        .and_then(|detail| detail.shift_remove("comments"))
        .ok_or_else(|| {
            invalid_response_with_id("GitHub review thread omitted comments", review_thread_id)
        })?;
    pagination::append_connection_pages(&mut comments, |cursor| {
        let variables = json!({"id": review_thread_id, "cursor": cursor});
        let data = query(
            REVIEW_THREAD_DETAIL_COMMENTS_QUERY,
            &variables,
            &format!("review thread {review_thread_id}"),
        )?;
        review_thread_node(&data, review_thread_id)?;
        pagination::take_value_at(data, &["node", "comments"])
            .map_err(|error| with_context(error, &format!("review thread {review_thread_id}")))
    })?;
    let nodes = comments
        .as_object_mut()
        .and_then(|comments| comments.shift_remove("nodes"))
        .ok_or_else(|| {
            invalid_response_with_id("GitHub comments nodes must be an array", review_thread_id)
        })?;
    detail
        .as_object_mut()
        .expect("review thread was validated as an object")
        .insert("comments".to_owned(), nodes);
    Ok(())
}

fn review_thread_node<'a>(data: &'a Value, review_thread_id: &str) -> Result<&'a Value> {
    let node = pagination::value_at(data, &["node"])
        .map_err(|error| with_context(error, &format!("review thread {review_thread_id}")))?;
    if node.is_null()
        || node.get("__typename").and_then(Value::as_str) != Some("PullRequestReviewThread")
    {
        return Err(not_found(format!(
            "review thread not found: {review_thread_id}"
        )));
    }
    let id = node.get("id").and_then(Value::as_str).ok_or_else(|| {
        invalid_response_with_id("GitHub review thread id must be a string", review_thread_id)
    })?;
    if id != review_thread_id {
        return Err(invalid_response_with_id(
            &format!("GitHub returned a different review thread: {id}"),
            review_thread_id,
        ));
    }
    Ok(node)
}

fn not_found(message: impl Into<String>) -> Exit {
    Exit::runtime(&RuntimeError::not_found(message))
}

fn invalid_response_with_id(message: &str, review_thread_id: &str) -> Exit {
    Exit::invalid_response(format!("{message} for review thread {review_thread_id}"))
}

fn invalid_response_with_context(message: &str, context: &str) -> Exit {
    Exit::invalid_response(format!("{message} ({context})"))
}

fn with_context(error: Exit, context: &str) -> Exit {
    let Exit { message, code } = error;
    let Ok(mut value) = serde_json::from_str::<Value>(&message) else {
        return Exit { message, code };
    };
    let Some(error_object) = value.get_mut("error").and_then(Value::as_object_mut) else {
        return Exit { message, code };
    };
    let Some(error_message) = error_object
        .get("message")
        .and_then(Value::as_str)
        .map(str::to_owned)
    else {
        return Exit { message, code };
    };
    error_object.insert(
        "message".to_owned(),
        Value::String(format!("{error_message} ({context})")),
    );
    let Ok(message) = serde_json::to_string(&value) else {
        return Exit { message, code };
    };
    Exit { message, code }
}

fn query(document: &str, variables: &Value, context: &str) -> Result<Value> {
    let response = super::query_with_errors(document, variables)
        .map_err(|error| with_context(error, context))?;
    match response {
        super::QueryResponse::Data(data) => Ok(data),
        super::QueryResponse::Errors(errors) => {
            let message = super::graphql_error_message(&errors);
            let missing_type = errors
                .as_array()
                .and_then(|errors| errors.first())
                .is_some_and(|error| error.get("type").is_none());
            let error = if missing_type
                && message
                    .to_ascii_lowercase()
                    .contains("could not resolve to a node")
            {
                not_found(message)
            } else {
                super::graphql_error(&errors)
            };
            Err(with_context(error, context))
        }
    }
}
