use serde_json::{Map, Value};

use crate::error::{Exit, Result};
use crate::github::graphql;
use crate::model::Target;

pub fn execute(target: &Target, review_thread_id: &str, include_diff_hunk: bool) -> Result<Value> {
    project_review_thread(
        graphql::review_thread::execute(target, review_thread_id)?,
        include_diff_hunk,
    )
}

fn project_review_thread(review_thread: Value, include_diff_hunk: bool) -> Result<Value> {
    let mut result = Map::new();
    let id = required_field(&review_thread, "id")?;
    if !id.is_string() {
        return Err(Exit::invalid_response("GitHub field id must be a string"));
    }
    result.insert("id".to_owned(), id.clone());
    for field in ["isResolved", "isOutdated"] {
        let value = required_field(&review_thread, field)?;
        if !value.is_boolean() {
            return Err(Exit::invalid_response(format!(
                "GitHub field {field} must be a boolean"
            )));
        }
        result.insert(field.to_owned(), value.clone());
    }
    for field in ["path", "line", "originalLine", "startLine", "diffSide"] {
        result.insert(field.to_owned(), nullable_location(&review_thread, field)?);
    }

    let comments = required_field(&review_thread, "comments")?
        .as_array()
        .ok_or_else(|| Exit::invalid_response("GitHub comments must be an array"))?;
    let mut comments = comments
        .iter()
        .map(|comment| project_comment(comment, include_diff_hunk))
        .collect::<Result<Vec<_>>>()?;
    comments.sort_by(|left, right| {
        string_value(left, "createdAt")
            .cmp(string_value(right, "createdAt"))
            .then_with(|| string_value(left, "id").cmp(string_value(right, "id")))
    });
    result.insert("comments".to_owned(), Value::Array(comments));
    Ok(Value::Object(result))
}

fn project_comment(comment: &Value, include_diff_hunk: bool) -> Result<Value> {
    let mut result = Map::new();
    for field in ["id", "url", "body", "createdAt", "updatedAt"] {
        let value = required_field(comment, field)?;
        if !value.is_string() {
            return Err(Exit::invalid_response(format!(
                "GitHub field {field} must be a string"
            )));
        }
        result.insert(field.to_owned(), value.clone());
    }
    let author = required_field(comment, "author")?;
    let author = match author {
        Value::Null => Value::Null,
        Value::Object(author) => author
            .get("login")
            .filter(|login| login.is_string())
            .cloned()
            .ok_or_else(|| Exit::invalid_response("GitHub author login must be a string"))?,
        _ => {
            return Err(Exit::invalid_response(
                "GitHub author must be an object or null",
            ));
        }
    };
    result.insert("author".to_owned(), author);

    let reply_to = required_field(comment, "replyTo")?;
    let reply_to_id = match reply_to {
        Value::Null => Value::Null,
        Value::Object(reply_to) => reply_to
            .get("id")
            .filter(|id| id.is_string())
            .cloned()
            .ok_or_else(|| Exit::invalid_response("GitHub replyTo id must be a string"))?,
        _ => {
            return Err(Exit::invalid_response(
                "GitHub replyTo must be an object or null",
            ));
        }
    };
    result.insert("replyToId".to_owned(), reply_to_id);

    if include_diff_hunk {
        let diff_hunk = required_field(comment, "diffHunk")?;
        if !diff_hunk.is_string() {
            return Err(Exit::invalid_response("GitHub diffHunk must be a string"));
        }
        result.insert("diffHunk".to_owned(), diff_hunk.clone());
    }
    Ok(Value::Object(result))
}

fn required_field<'a>(value: &'a Value, field: &str) -> Result<&'a Value> {
    value
        .get(field)
        .ok_or_else(|| Exit::invalid_response(format!("GitHub response omitted {field}")))
}

fn nullable_location(value: &Value, field: &str) -> Result<Value> {
    let value = required_field(value, field)?;
    let valid = match field {
        "path" | "diffSide" => value.is_null() || value.is_string(),
        "line" | "originalLine" | "startLine" => value.is_null() || value.as_i64().is_some(),
        _ => false,
    };
    if !valid {
        return Err(Exit::invalid_response(format!(
            "GitHub field {field} has an invalid value"
        )));
    }
    Ok(value.clone())
}

fn string_value<'a>(value: &'a Value, field: &str) -> &'a str {
    value[field]
        .as_str()
        .expect("projected comment string was validated")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn malformed_required_field_is_rejected_without_projecting_it() {
        let result = project_review_thread(json!({"id": 42}), false);
        let Err(error) = result else {
            panic!("expected an invalid response")
        };

        assert!(
            error
                .stderr_line()
                .contains("GitHub field id must be a string")
        );
    }
}
