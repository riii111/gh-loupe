use serde_json::{Value, json};

use crate::error::{Exit, Result, RuntimeError};
use crate::model::Target;

use super::pagination;

const REVIEW_THREAD_SUMMARIES_QUERY: &str = r"
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

const REVIEW_THREAD_SUMMARY_COMMENTS_QUERY: &str = r"
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
    let mut summaries = Vec::new();
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
        let data = super::query(REVIEW_THREAD_SUMMARIES_QUERY, &variables)?;
        let pull_request = pagination::take_value_at(data, &["repository", "pullRequest"])?;
        if pull_request.is_null() {
            let message = format!(
                "pull request not found: {}#{}",
                target.repository, target.number
            );
            return Err(Exit::runtime(&RuntimeError::not_found(message)));
        }
        let mut connection = pagination::take_value_at(pull_request, &["reviewThreads"])?;
        summaries.extend(pagination::take_nodes(&mut connection)?);
        let Some(_) = cursor_tracker.next(&connection)? else {
            break;
        };
    }

    if !include_resolved {
        summaries.retain(|summary| summary.get("isResolved") != Some(&Value::Bool(true)));
    }
    for summary in &mut summaries {
        append_summary_comment_pages(summary)?;
    }
    Ok(summaries)
}

fn append_summary_comment_pages(summary: &mut Value) -> Result<()> {
    let id = summary
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| Exit::invalid_response("GitHub field id must be a string"))?
        .to_owned();
    let mut comments = summary
        .as_object_mut()
        .and_then(|summary| summary.shift_remove("comments"))
        .ok_or_else(|| Exit::invalid_response("GitHub review thread omitted comments"))?;
    pagination::append_connection_pages(&mut comments, |cursor| {
        let variables = json!({"id": id, "cursor": cursor});
        let data = super::query(REVIEW_THREAD_SUMMARY_COMMENTS_QUERY, &variables)?;
        pagination::take_value_at(data, &["node", "comments"])
    })?;
    let nodes = comments
        .as_object_mut()
        .and_then(|comments| comments.shift_remove("nodes"))
        .ok_or_else(|| Exit::invalid_response("GitHub comments nodes must be an array"))?;
    summary
        .as_object_mut()
        .expect("review thread was validated as an object")
        .insert("comments".to_owned(), nodes);
    Ok(())
}
