use serde_json::{Map, Value};

use crate::error::{Exit, Result};
use crate::github::graphql;
use crate::markdown;
use crate::model::Target;

use super::{bool_field, nullable_location, required_field, string_field, string_value};

pub fn execute(
    target: &Target,
    review_thread_id: &str,
    include_diff_hunk: bool,
    include_details: bool,
) -> Result<Value> {
    project_review_thread(
        graphql::review_thread::execute(target, review_thread_id)?,
        include_diff_hunk,
        include_details,
    )
}

fn project_review_thread(
    review_thread: Value,
    include_diff_hunk: bool,
    include_details: bool,
) -> Result<Value> {
    let mut result = Map::new();
    let id = string_field(&review_thread, "id")?;
    result.insert("id".to_owned(), Value::String(id.to_owned()));
    for field in ["isResolved", "isOutdated"] {
        result.insert(
            field.to_owned(),
            Value::Bool(bool_field(&review_thread, field)?),
        );
    }
    for field in ["path", "line", "originalLine", "startLine", "diffSide"] {
        result.insert(field.to_owned(), nullable_location(&review_thread, field)?);
    }

    let comments = required_field(&review_thread, "comments")?
        .as_array()
        .ok_or_else(|| Exit::invalid_response("GitHub comments must be an array"))?;
    let projected_comments = comments
        .iter()
        .map(|comment| project_comment(comment, include_diff_hunk, include_details))
        .collect::<Result<Vec<_>>>()?;
    let details_omitted = projected_comments.iter().any(|(_, omitted)| *omitted);
    let mut comments = projected_comments
        .into_iter()
        .map(|(comment, _)| comment)
        .collect::<Vec<_>>();
    comments.sort_by(|left, right| {
        string_value(left, "createdAt")
            .cmp(string_value(right, "createdAt"))
            .then_with(|| string_value(left, "id").cmp(string_value(right, "id")))
    });
    result.insert("comments".to_owned(), Value::Array(comments));
    result.insert("detailsOmitted".to_owned(), Value::Bool(details_omitted));
    Ok(Value::Object(result))
}

fn project_comment(
    comment: &Value,
    include_diff_hunk: bool,
    include_details: bool,
) -> Result<(Value, bool)> {
    let mut result = Map::new();
    let mut details_omitted = false;
    for field in ["id", "url", "body", "createdAt", "updatedAt"] {
        let value = string_field(comment, field)?;
        if field == "body" && !include_details {
            let (body, omitted) = markdown::omit_details(value);
            result.insert(field.to_owned(), Value::String(body));
            details_omitted = omitted;
        } else {
            result.insert(field.to_owned(), Value::String(value.to_owned()));
        }
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
    Ok((Value::Object(result), details_omitted))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn malformed_required_field_is_rejected_without_projecting_it() {
        let result = project_review_thread(json!({"id": 42}), false, false);
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
