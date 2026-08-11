use serde_json::{Value, json};

use crate::error::{Exit, Result, RuntimeError};
use crate::model::Target;

use super::pagination;

const REVIEW_THREAD_QUERY: &str = r"
query ReviewThreadDetail($id: ID!) {
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

const REVIEW_THREAD_COMMENTS_QUERY: &str = r"
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

pub fn execute(target: &Target, review_thread_id: &str) -> Result<Value> {
    let variables = json!({"id": review_thread_id});
    let data = query(REVIEW_THREAD_QUERY, &variables)?;
    let mut review_thread = review_thread_node(&data, review_thread_id)?.clone();
    verify_pull_request(&review_thread, target)?;
    review_thread
        .as_object_mut()
        .expect("review thread was validated as an object")
        .shift_remove("pullRequest");
    append_review_thread_comment_pages(&mut review_thread, review_thread_id)?;
    Ok(review_thread)
}

fn verify_pull_request(review_thread: &Value, target: &Target) -> Result<()> {
    let pull_request = pagination::value_at(review_thread, &["pullRequest"])?;
    let number = pagination::value_at(pull_request, &["number"])?
        .as_u64()
        .ok_or_else(|| Exit::invalid_response("GitHub pull request number must be an integer"))?;
    let repository = pagination::value_at(pull_request, &["repository", "nameWithOwner"])?
        .as_str()
        .ok_or_else(|| {
            Exit::invalid_response("GitHub repository nameWithOwner must be a string")
        })?;
    if number.to_string() != target.number || !repository.eq_ignore_ascii_case(&target.repository) {
        return Err(not_found(format!(
            "review thread not found in {}#{}",
            target.repository, target.number
        )));
    }
    Ok(())
}

fn append_review_thread_comment_pages(
    review_thread: &mut Value,
    review_thread_id: &str,
) -> Result<()> {
    let mut comments = review_thread
        .as_object_mut()
        .and_then(|review_thread| review_thread.shift_remove("comments"))
        .ok_or_else(|| Exit::invalid_response("GitHub review thread omitted comments"))?;
    let mut cursor_tracker = pagination::CursorTracker::default();
    while let Some(cursor) = cursor_tracker.next(&comments)? {
        let variables = json!({"id": review_thread_id, "cursor": cursor});
        let data = query(REVIEW_THREAD_COMMENTS_QUERY, &variables)?;
        let node = review_thread_node(&data, review_thread_id)?;
        let page = pagination::value_at(node, &["comments"])?;
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
    review_thread
        .as_object_mut()
        .expect("review thread was validated as an object")
        .insert("comments".to_owned(), nodes);
    Ok(())
}

fn review_thread_node<'a>(data: &'a Value, review_thread_id: &str) -> Result<&'a Value> {
    let node = pagination::value_at(data, &["node"])?;
    if node.is_null()
        || node.get("__typename").and_then(Value::as_str) != Some("PullRequestReviewThread")
    {
        return Err(not_found(format!(
            "review thread not found: {review_thread_id}"
        )));
    }
    let id = node
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| Exit::invalid_response("GitHub review thread id must be a string"))?;
    if id != review_thread_id {
        return Err(Exit::invalid_response(
            "GitHub returned a different review thread",
        ));
    }
    Ok(node)
}

fn not_found(message: impl Into<String>) -> Exit {
    Exit::runtime(&RuntimeError::not_found(message))
}

fn query(document: &str, variables: &Value) -> Result<Value> {
    match super::query_with_errors(document, variables)? {
        super::QueryResponse::Data(data) => Ok(data),
        super::QueryResponse::Errors(errors) => {
            let message = super::graphql_error_message(&errors);
            if message
                .to_ascii_lowercase()
                .contains("could not resolve to a node")
            {
                return Err(not_found(message));
            }
            Err(super::graphql_error(&errors))
        }
    }
}
