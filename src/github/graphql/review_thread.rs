use serde_json::{Value, json};

use crate::error::{Exit, Result, RuntimeError};
use crate::model::Target;

use super::pagination;

const REVIEW_THREAD_DETAIL_QUERY: &str = r"
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

pub fn execute(target: &Target, review_thread_id: &str) -> Result<Value> {
    let variables = json!({"id": review_thread_id});
    let data = query(REVIEW_THREAD_DETAIL_QUERY, &variables)?;
    review_thread_node(&data, review_thread_id)?;
    let mut detail = pagination::take_value_at(data, &["node"])?;
    verify_pull_request(&detail, target)?;
    detail
        .as_object_mut()
        .expect("review thread was validated as an object")
        .shift_remove("pullRequest");
    append_detail_comment_pages(&mut detail, review_thread_id)?;
    Ok(detail)
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

fn append_detail_comment_pages(detail: &mut Value, review_thread_id: &str) -> Result<()> {
    let mut comments = detail
        .as_object_mut()
        .and_then(|detail| detail.shift_remove("comments"))
        .ok_or_else(|| Exit::invalid_response("GitHub review thread omitted comments"))?;
    pagination::append_connection_pages(&mut comments, |cursor| {
        let variables = json!({"id": review_thread_id, "cursor": cursor});
        let data = query(REVIEW_THREAD_DETAIL_COMMENTS_QUERY, &variables)?;
        review_thread_node(&data, review_thread_id)?;
        pagination::take_value_at(data, &["node", "comments"])
    })?;
    let nodes = comments
        .as_object_mut()
        .and_then(|comments| comments.shift_remove("nodes"))
        .ok_or_else(|| Exit::invalid_response("GitHub comments nodes must be an array"))?;
    detail
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
            let missing_type = errors
                .as_array()
                .and_then(|errors| errors.first())
                .is_some_and(|error| error.get("type").is_none());
            if missing_type
                && message
                    .to_ascii_lowercase()
                    .contains("could not resolve to a node")
            {
                return Err(not_found(message));
            }
            Err(super::graphql_error(&errors))
        }
    }
}
