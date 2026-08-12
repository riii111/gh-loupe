use std::thread;

use serde_json::{Map, Value};

use crate::error::{Exit, Result};
use crate::github::rest;
use crate::model::Target;

pub fn execute(target: &Target) -> Result<Value> {
    let (issue_result, comments_result) = thread::scope(|scope| {
        let issue = scope.spawn(|| rest::issue(target));
        let comments = scope.spawn(|| rest::issue_comments(target));

        (
            issue.join().expect("issue worker must not panic"),
            comments
                .join()
                .expect("issue comments worker must not panic"),
        )
    });

    let issue = project_issue(&issue_result?)?;
    let comments = project_comments(comments_result?)?;

    let mut result = Map::new();
    result.insert(
        "repository".to_owned(),
        Value::String(target.repository.clone()),
    );
    result.insert("issue".to_owned(), issue);
    result.insert("comments".to_owned(), Value::Array(comments));
    Ok(Value::Object(result))
}

fn project_issue(issue: &Value) -> Result<Value> {
    if issue.get("pull_request").is_some() {
        return Err(Exit::invalid_response(
            "GitHub target is a pull request; use the pr commands",
        ));
    }

    let mut result = Map::new();
    result.insert("number".to_owned(), required_u64(issue, "number")?);
    result.insert(
        "title".to_owned(),
        Value::String(required_string_field(issue, "title")?.to_owned()),
    );
    result.insert(
        "url".to_owned(),
        Value::String(required_string_field(issue, "html_url")?.to_owned()),
    );
    result.insert(
        "state".to_owned(),
        Value::String(normalize_enum(required_string_field(issue, "state")?)),
    );
    result.insert(
        "stateReason".to_owned(),
        nullable_enum_field(issue, "state_reason")?,
    );
    result.insert("body".to_owned(), nullable_string_field(issue, "body")?);
    result.insert("author".to_owned(), project_author(issue, "user")?);
    result.insert("labels".to_owned(), project_labels(issue)?);
    result.insert("assignees".to_owned(), project_logins(issue, "assignees")?);
    result.insert("milestone".to_owned(), project_milestone(issue)?);
    result.insert(
        "createdAt".to_owned(),
        Value::String(required_string_field(issue, "created_at")?.to_owned()),
    );
    result.insert(
        "updatedAt".to_owned(),
        Value::String(required_string_field(issue, "updated_at")?.to_owned()),
    );
    result.insert(
        "closedAt".to_owned(),
        nullable_string_field(issue, "closed_at")?,
    );
    result.insert("subIssues".to_owned(), project_sub_issues(issue)?);
    result.insert("dependencies".to_owned(), project_dependencies(issue)?);
    Ok(Value::Object(result))
}

fn project_comments(comments: Vec<Value>) -> Result<Vec<Value>> {
    let mut comments = comments
        .into_iter()
        .map(project_comment)
        .collect::<Result<Vec<_>>>()?;
    comments.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok(comments.into_iter().map(|comment| comment.value).collect())
}

struct Comment {
    id: String,
    created_at: String,
    value: Value,
}

fn project_comment(comment: Value) -> Result<Comment> {
    let id = required_string_field(&comment, "node_id")?.to_owned();
    let created_at = required_string_field(&comment, "created_at")?.to_owned();
    let mut result = Map::new();
    result.insert("id".to_owned(), Value::String(id.clone()));
    result.insert(
        "url".to_owned(),
        Value::String(required_string_field(&comment, "html_url")?.to_owned()),
    );
    result.insert("author".to_owned(), project_author(&comment, "user")?);
    result.insert(
        "body".to_owned(),
        Value::String(required_string_field(&comment, "body")?.to_owned()),
    );
    result.insert("createdAt".to_owned(), Value::String(created_at.clone()));
    result.insert(
        "updatedAt".to_owned(),
        Value::String(required_string_field(&comment, "updated_at")?.to_owned()),
    );
    Ok(Comment {
        id,
        created_at,
        value: Value::Object(result),
    })
}

fn project_labels(issue: &Value) -> Result<Value> {
    let labels = required_array(issue, "labels")?;
    let mut names = labels
        .iter()
        .map(|label| {
            let name = label
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| Exit::invalid_response("GitHub label name must be a string"))?;
            Ok(name.to_owned())
        })
        .collect::<Result<Vec<_>>>()?;
    names.sort_unstable();
    Ok(Value::Array(names.into_iter().map(Value::String).collect()))
}

fn project_logins(value: &Value, field: &str) -> Result<Value> {
    let users = required_array(value, field)?;
    let mut logins = users
        .iter()
        .map(|user| {
            user.get("login")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| Exit::invalid_response("GitHub user login must be a string"))
        })
        .collect::<Result<Vec<_>>>()?;
    logins.sort_unstable();
    Ok(Value::Array(
        logins.into_iter().map(Value::String).collect(),
    ))
}

fn project_author(value: &Value, field: &str) -> Result<Value> {
    match required_field(value, field)? {
        Value::Null => Ok(Value::Null),
        Value::Object(user) => user
            .get("login")
            .and_then(Value::as_str)
            .map(|login| Value::String(login.to_owned()))
            .ok_or_else(|| Exit::invalid_response("GitHub user login must be a string")),
        _ => Err(Exit::invalid_response(
            "GitHub user must be an object or null",
        )),
    }
}

fn project_milestone(issue: &Value) -> Result<Value> {
    match required_field(issue, "milestone")? {
        Value::Null => Ok(Value::Null),
        Value::Object(milestone) => {
            let milestone = Value::Object(milestone.clone());
            let mut result = Map::new();
            result.insert(
                "title".to_owned(),
                Value::String(required_string_field(&milestone, "title")?.to_owned()),
            );
            result.insert(
                "state".to_owned(),
                Value::String(normalize_enum(required_string_field(&milestone, "state")?)),
            );
            result.insert(
                "dueOn".to_owned(),
                nullable_string_field(&milestone, "due_on")?,
            );
            Ok(Value::Object(result))
        }
        _ => Err(Exit::invalid_response(
            "GitHub milestone must be an object or null",
        )),
    }
}

fn project_sub_issues(issue: &Value) -> Result<Value> {
    let Some(summary) = issue.get("sub_issues_summary") else {
        return Ok(Value::Null);
    };
    if summary.is_null() {
        return Ok(Value::Null);
    }
    let mut result = Map::new();
    result.insert("total".to_owned(), required_u64(summary, "total")?);
    result.insert("completed".to_owned(), required_u64(summary, "completed")?);
    Ok(Value::Object(result))
}

fn project_dependencies(issue: &Value) -> Result<Value> {
    let Some(summary) = issue.get("issue_dependencies_summary") else {
        return Ok(Value::Null);
    };
    if summary.is_null() {
        return Ok(Value::Null);
    }
    let mut result = Map::new();
    result.insert("blockedBy".to_owned(), required_u64(summary, "blocked_by")?);
    result.insert("blocking".to_owned(), required_u64(summary, "blocking")?);
    Ok(Value::Object(result))
}

fn required_field<'a>(value: &'a Value, field: &str) -> Result<&'a Value> {
    value
        .get(field)
        .ok_or_else(|| Exit::invalid_response(format!("GitHub response omitted {field}")))
}

fn required_array<'a>(value: &'a Value, field: &str) -> Result<&'a [Value]> {
    required_field(value, field)?
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| Exit::invalid_response(format!("GitHub field {field} must be an array")))
}

fn required_string_field<'a>(value: &'a Value, field: &str) -> Result<&'a str> {
    required_field(value, field)?
        .as_str()
        .ok_or_else(|| Exit::invalid_response(format!("GitHub field {field} must be a string")))
}

fn nullable_string_field(value: &Value, field: &str) -> Result<Value> {
    match required_field(value, field)? {
        Value::Null => Ok(Value::Null),
        Value::String(value) => Ok(Value::String(value.clone())),
        _ => Err(Exit::invalid_response(format!(
            "GitHub field {field} must be a string or null"
        ))),
    }
}

fn nullable_enum_field(value: &Value, field: &str) -> Result<Value> {
    match required_field(value, field)? {
        Value::Null => Ok(Value::Null),
        Value::String(value) => Ok(Value::String(normalize_enum(value))),
        _ => Err(Exit::invalid_response(format!(
            "GitHub field {field} must be a string or null"
        ))),
    }
}

fn required_u64(value: &Value, field: &str) -> Result<Value> {
    required_field(value, field)?
        .as_u64()
        .map(Value::from)
        .ok_or_else(|| Exit::invalid_response(format!("GitHub field {field} must be an integer")))
}

fn normalize_enum(value: &str) -> String {
    value.to_ascii_uppercase()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn projects_issue_schema_without_raw_metadata() {
        let issue = json!({
            "number": 39,
            "title": "Issue title",
            "html_url": "https://github.com/owner/repository/issues/39",
            "url": "https://api.github.com/repos/owner/repository/issues/39",
            "state": "open",
            "state_reason": null,
            "body": null,
            "user": null,
            "labels": [{"name": "zeta"}, {"name": "alpha"}],
            "assignees": [{"login": "zoe"}, {"login": "alice"}],
            "milestone": {
                "title": "v1",
                "state": "open",
                "due_on": null,
                "url": "https://api.github.com/milestones/1"
            },
            "created_at": "2026-08-12T00:00:00Z",
            "updated_at": "2026-08-12T01:00:00Z",
            "closed_at": null,
            "sub_issues_summary": {"total": 2, "completed": 1, "percent_completed": 50},
            "issue_dependencies_summary": {
                "blocked_by": 2,
                "total_blocked_by": 9,
                "blocking": 1,
                "total_blocking": 8
            },
            "reactions": {"total_count": 99}
        });

        assert_eq!(
            project_issue(&issue).unwrap_or_else(|_| panic!("valid issue")),
            json!({
                "number": 39,
                "title": "Issue title",
                "url": "https://github.com/owner/repository/issues/39",
                "state": "OPEN",
                "stateReason": null,
                "body": null,
                "author": null,
                "labels": ["alpha", "zeta"],
                "assignees": ["alice", "zoe"],
                "milestone": {"title": "v1", "state": "OPEN", "dueOn": null},
                "createdAt": "2026-08-12T00:00:00Z",
                "updatedAt": "2026-08-12T01:00:00Z",
                "closedAt": null,
                "subIssues": {"total": 2, "completed": 1},
                "dependencies": {"blockedBy": 2, "blocking": 1}
            })
        );
    }

    #[test]
    fn absent_summaries_are_null() {
        let issue = json!({
            "number": 39,
            "title": "Issue title",
            "html_url": "https://github.com/owner/repository/issues/39",
            "state": "closed",
            "state_reason": "not_planned",
            "body": "body",
            "user": {"login": "author"},
            "labels": [],
            "assignees": [],
            "milestone": null,
            "created_at": "2026-08-12T00:00:00Z",
            "updated_at": "2026-08-12T01:00:00Z",
            "closed_at": "2026-08-12T02:00:00Z"
        });

        let projected = project_issue(&issue).unwrap_or_else(|_| panic!("valid issue"));
        assert_eq!(projected["state"], "CLOSED");
        assert_eq!(projected["stateReason"], "NOT_PLANNED");
        assert_eq!(projected["subIssues"], Value::Null);
        assert_eq!(projected["dependencies"], Value::Null);
    }

    #[test]
    fn comments_use_global_ids_and_are_sorted() {
        let comments = project_comments(vec![
            json!({
                "node_id": "IC_z",
                "html_url": "https://github.com/owner/repository/issues/39#issuecomment-2",
                "user": {"login": "later"},
                "body": "later",
                "created_at": "2026-01-02T00:00:00Z",
                "updated_at": "2026-01-02T00:00:00Z",
                "id": 2
            }),
            json!({
                "node_id": "IC_a",
                "html_url": "https://github.com/owner/repository/issues/39#issuecomment-1",
                "user": null,
                "body": "first",
                "created_at": "2026-01-01T00:00:00Z",
                "updated_at": "2026-01-01T00:00:00Z",
                "id": 1
            }),
        ])
        .unwrap_or_else(|_| panic!("valid comments"));

        assert_eq!(comments[0]["id"], "IC_a");
        assert_eq!(comments[0]["author"], Value::Null);
        assert_eq!(comments[1]["id"], "IC_z");
        assert_eq!(comments[0].as_object().expect("comment object").len(), 6);
        assert_eq!(
            comments[0]["url"],
            "https://github.com/owner/repository/issues/39#issuecomment-1"
        );
    }

    #[test]
    fn pull_request_marker_is_rejected() {
        let issue =
            json!({"pull_request": {"html_url": "https://github.com/owner/repository/pull/39"}});

        let error = project_issue(&issue).expect_err("pull request must be rejected");
        assert!(error.stderr_line().contains("use the pr commands"));
    }

    #[test]
    fn null_pull_request_marker_is_rejected() {
        let issue = json!({"pull_request": null});

        let error = project_issue(&issue).expect_err("null pull request marker must be rejected");
        assert!(error.stderr_line().contains("use the pr commands"));
    }
}
