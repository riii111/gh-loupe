use serde_json::{Value, json};

use crate::error::{Exit, Result, RuntimeError};
use crate::model::Target;

use super::pagination;

const REVIEW_THREADS_QUERY: &str = r"
query ReviewThreadSummaries($owner: String!, $name: String!, $number: Int!, $cursor: String) {
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

const REVIEW_THREAD_COMMENTS_QUERY: &str = r"
query ReviewThreadSummaryComments($id: ID!, $cursor: String) {
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
    let mut review_threads = Vec::new();
    let number = target
        .number
        .parse::<i32>()
        .map_err(|_| super::invalid_graphql_response())?;

    loop {
        let variables = json!({
            "owner": owner,
            "name": name,
            "number": number,
            "cursor": cursor_tracker.cursor(),
        });
        let data = super::query(REVIEW_THREADS_QUERY, &variables)?;
        let pull_request = pagination::value_at(&data, &["repository", "pullRequest"])?;
        if pull_request.is_null() {
            let message = format!(
                "pull request not found: {}#{}",
                target.repository, target.number
            );
            return Err(Exit::runtime(&RuntimeError::not_found(message)));
        }
        let connection = pagination::value_at(pull_request, &["reviewThreads"])?;
        review_threads.extend(pagination::nodes(connection)?.iter().cloned());
        let Some(_) = cursor_tracker.next(connection)? else {
            break;
        };
    }

    if !include_resolved {
        review_threads
            .retain(|review_thread| review_thread.get("isResolved") != Some(&Value::Bool(true)));
    }
    for review_thread in &mut review_threads {
        append_review_thread_comment_pages(review_thread)?;
    }
    Ok(review_threads)
}

fn append_review_thread_comment_pages(review_thread: &mut Value) -> Result<()> {
    let id = review_thread
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| Exit::invalid_response("GitHub field id must be a string"))?
        .to_owned();
    let mut comments = review_thread
        .as_object_mut()
        .and_then(|review_thread| review_thread.shift_remove("comments"))
        .ok_or_else(|| Exit::invalid_response("GitHub review thread omitted comments"))?;
    let mut cursor_tracker = pagination::CursorTracker::default();
    while let Some(cursor) = cursor_tracker.next(&comments)? {
        let variables = json!({"id": id, "cursor": cursor});
        let data = super::query(REVIEW_THREAD_COMMENTS_QUERY, &variables)?;
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
    review_thread
        .as_object_mut()
        .expect("review thread was validated as an object")
        .insert("comments".to_owned(), nodes);
    Ok(())
}
