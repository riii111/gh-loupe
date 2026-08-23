use serde_json::{Map, Value};

use crate::error::{Exit, Result};
use crate::github::rest;
use crate::markdown;
use crate::model::Target;

use super::{required_field, required_string_field, string_value};

pub struct CommentList {
    pub comments: Vec<Value>,
    pub total_count: usize,
    pub truncated: bool,
}

pub fn execute(
    target: &Target,
    include_details: bool,
    limit: Option<usize>,
    since: Option<&str>,
) -> Result<CommentList> {
    let mut comments = rest::pull_request_comments(target, since)?
        .iter()
        .map(|comment| project(comment, include_details))
        .collect::<Result<Vec<_>>>()?;
    if let Some(since) = since {
        comments.retain(|comment| string_value(comment, "updatedAt") > since);
    }
    comments.sort_by(|left, right| {
        string_value(left, "createdAt")
            .cmp(string_value(right, "createdAt"))
            .then_with(|| string_value(left, "id").cmp(string_value(right, "id")))
    });
    let total_count = comments.len();
    if let Some(limit) = limit {
        let start = total_count.saturating_sub(limit);
        comments.drain(..start);
    }
    Ok(CommentList {
        truncated: comments.len() < total_count,
        comments,
        total_count,
    })
}

fn project(comment: &Value, include_details: bool) -> Result<Value> {
    let mut result = Map::new();
    let mut details_omitted = false;
    for (source, output) in [
        ("node_id", "id"),
        ("html_url", "url"),
        ("body", "body"),
        ("created_at", "createdAt"),
        ("updated_at", "updatedAt"),
    ] {
        let value = required_string_field(comment, source)?;
        if source == "body" && !include_details {
            let (body, omitted) = markdown::omit_details(value);
            result.insert(output.to_owned(), Value::String(body));
            details_omitted = omitted;
        } else {
            result.insert(output.to_owned(), Value::String(value.to_owned()));
        }
    }
    result.insert("detailsOmitted".to_owned(), Value::Bool(details_omitted));
    let user = required_field(comment, "user")?;
    let author = match user {
        Value::Null => Value::Null,
        Value::Object(user) => user
            .get("login")
            .filter(|login| login.is_string())
            .cloned()
            .ok_or_else(|| Exit::invalid_response("GitHub user login must be a string"))?,
        _ => {
            return Err(Exit::invalid_response(
                "GitHub user must be an object or null",
            ));
        }
    };
    result.insert("author".to_owned(), author);
    Ok(Value::Object(result))
}
