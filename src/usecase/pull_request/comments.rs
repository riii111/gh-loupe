use serde_json::{Map, Value};

use crate::error::{Exit, Result};
use crate::github::rest;
use crate::model::Target;

use super::{required_field, string_field, string_value};

pub fn execute(target: &Target) -> Result<Vec<Value>> {
    let mut comments = rest::pull_request_comments(target)?
        .iter()
        .map(project)
        .collect::<Result<Vec<_>>>()?;
    comments.sort_by(|left, right| {
        string_value(left, "createdAt")
            .cmp(string_value(right, "createdAt"))
            .then_with(|| string_value(left, "id").cmp(string_value(right, "id")))
    });
    Ok(comments)
}

fn project(comment: &Value) -> Result<Value> {
    let mut result = Map::new();
    for (source, output) in [
        ("node_id", "id"),
        ("html_url", "url"),
        ("body", "body"),
        ("created_at", "createdAt"),
        ("updated_at", "updatedAt"),
    ] {
        let value = string_field(comment, source)?;
        result.insert(output.to_owned(), Value::String(value.to_owned()));
    }
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
