use std::cmp::Ordering;

use serde_json::{Map, Value};

use crate::error::{Exit, Result};
use crate::github::rest;
use crate::model::Target;

use super::string_field;

pub fn execute(target: &Target) -> Result<Vec<Value>> {
    let mut reviews = rest::pull_request_reviews(target)?
        .into_iter()
        .map(project)
        .collect::<Result<Vec<_>>>()?;
    reviews.sort_by(|left, right| {
        compare_submitted_at(left.submitted_at.as_deref(), right.submitted_at.as_deref())
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok(reviews.into_iter().map(|review| review.value).collect())
}

struct Review {
    id: String,
    submitted_at: Option<String>,
    value: Value,
}

fn project(review: Value) -> Result<Review> {
    let id = string_field(&review, "node_id")?.to_owned();
    let submitted_at = nullable_string_field(&review, "submitted_at")?;
    let mut value = Map::new();
    value.insert("id".to_owned(), Value::String(id.clone()));
    value.insert("author".to_owned(), author(&review)?);
    value.insert(
        "state".to_owned(),
        Value::String(string_field(&review, "state")?.to_owned()),
    );
    value.insert(
        "body".to_owned(),
        Value::String(string_field(&review, "body")?.to_owned()),
    );
    value.insert(
        "submittedAt".to_owned(),
        submitted_at.clone().map_or(Value::Null, Value::String),
    );
    value.insert(
        "commitOid".to_owned(),
        nullable_string_field(&review, "commit_id")?.map_or(Value::Null, Value::String),
    );
    Ok(Review {
        id,
        submitted_at,
        value: Value::Object(value),
    })
}

fn compare_submitted_at(left: Option<&str>, right: Option<&str>) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => left.cmp(right),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn nullable_string_field(value: &Value, field: &str) -> Result<Option<String>> {
    let value = value
        .get(field)
        .ok_or_else(|| Exit::invalid_response(format!("GitHub response omitted {field}")))?;
    match value {
        Value::Null => Ok(None),
        Value::String(value) => Ok(Some(value.clone())),
        _ => Err(Exit::invalid_response(format!(
            "GitHub field {field} must be a string or null"
        ))),
    }
}

fn author(review: &Value) -> Result<Value> {
    match review
        .get("user")
        .ok_or_else(|| Exit::invalid_response("GitHub response omitted user"))?
    {
        Value::Null => Ok(Value::Null),
        Value::Object(user) => user
            .get("login")
            .and_then(Value::as_str)
            .map(|login| Value::String(login.to_owned()))
            .ok_or_else(|| Exit::invalid_response("GitHub field user.login must be a string")),
        _ => Err(Exit::invalid_response(
            "GitHub field user must be an object or null",
        )),
    }
}
