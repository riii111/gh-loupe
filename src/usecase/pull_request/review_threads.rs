use serde_json::{Map, Value};

use crate::error::{Exit, Result};
use crate::github::graphql;
use crate::model::Target;

use super::{nullable_location, string_field};

pub fn execute(target: &Target, include_resolved: bool) -> Result<Vec<Value>> {
    let mut review_thread_summaries = graphql::review_threads::execute(target, include_resolved)?
        .into_iter()
        .map(project_review_thread)
        .collect::<Result<Vec<_>>>()?;
    review_thread_summaries.sort_by(|left, right| {
        left.first_created_at
            .cmp(&right.first_created_at)
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok(review_thread_summaries
        .into_iter()
        .map(|summary| summary.value)
        .collect())
}

struct ReviewThreadSummary {
    first_created_at: String,
    id: String,
    value: Value,
}

fn project_review_thread(review_thread: Value) -> Result<ReviewThreadSummary> {
    let id = string_field(&review_thread, "id")?.to_owned();
    let comments = review_thread
        .get("comments")
        .and_then(Value::as_array)
        .ok_or_else(|| Exit::invalid_response("GitHub comments must be an array"))?;
    let first_created_at = comments
        .iter()
        .map(|comment| string_field(comment, "createdAt"))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .min()
        .ok_or_else(|| Exit::invalid_response("GitHub review thread has no comments"))?
        .to_owned();
    let last_updated_at = comments
        .iter()
        .map(|comment| string_field(comment, "updatedAt"))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .max()
        .expect("a first comment was validated")
        .to_owned();

    let mut value = Map::new();
    value.insert("id".to_owned(), Value::String(id.clone()));
    value.insert(
        "isResolved".to_owned(),
        Value::Bool(bool_field(&review_thread, "isResolved")?),
    );
    value.insert(
        "isOutdated".to_owned(),
        Value::Bool(bool_field(&review_thread, "isOutdated")?),
    );
    for field in ["path", "line", "originalLine", "startLine", "diffSide"] {
        value.insert(field.to_owned(), nullable_location(&review_thread, field)?);
    }
    value.insert("commentCount".to_owned(), Value::from(comments.len()));
    value.insert("lastUpdatedAt".to_owned(), Value::String(last_updated_at));
    Ok(ReviewThreadSummary {
        first_created_at,
        id,
        value: Value::Object(value),
    })
}

fn bool_field(value: &Value, field: &str) -> Result<bool> {
    value
        .get(field)
        .and_then(Value::as_bool)
        .ok_or_else(|| Exit::invalid_response(format!("GitHub field {field} must be a boolean")))
}
