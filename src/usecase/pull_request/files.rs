use serde_json::{Map, Value};

use crate::error::{Exit, Result};
use crate::github::graphql;
use crate::model::Target;

pub fn execute(target: &Target, limit: usize) -> Result<Value> {
    let response = graphql::pull_request_files(target, limit)?;
    project(response, limit)
}

fn project(response: Value, limit: usize) -> Result<Value> {
    let response = response.as_object().ok_or_else(|| {
        Exit::invalid_response("GitHub pull request files response must be an object")
    })?;
    let changed_files = required_count(response, "changedFiles", "pull request")?;
    let additions = required_count(response, "additions", "pull request")?;
    let deletions = required_count(response, "deletions", "pull request")?;
    let connection = response
        .get("files")
        .ok_or_else(|| Exit::invalid_response("GitHub pull request response omitted files"))?
        .as_object()
        .ok_or_else(|| Exit::invalid_response("GitHub pull request files must be an object"))?;
    let total_count = required_count(connection, "totalCount", "files connection")?;
    let nodes = connection
        .get("nodes")
        .ok_or_else(|| Exit::invalid_response("GitHub files connection omitted nodes"))?
        .as_array()
        .ok_or_else(|| Exit::invalid_response("GitHub files connection nodes must be an array"))?;
    if nodes.len() as u64 > total_count || nodes.len() as u64 > changed_files || nodes.len() > limit
    {
        return Err(Exit::invalid_response(
            "GitHub files connection returned an invalid number of nodes",
        ));
    }
    let files = nodes.iter().map(project_file).collect::<Result<Vec<_>>>()?;
    let summary = Value::Object(Map::from_iter([
        ("total".to_owned(), Value::from(changed_files)),
        ("additions".to_owned(), Value::from(additions)),
        ("deletions".to_owned(), Value::from(deletions)),
    ]));

    Ok(Value::Object(Map::from_iter([
        ("files".to_owned(), Value::Array(files)),
        ("summary".to_owned(), summary),
        ("totalCount".to_owned(), Value::from(changed_files)),
        (
            "truncated".to_owned(),
            Value::Bool(changed_files > nodes.len() as u64),
        ),
    ])))
}

fn project_file(file: &Value) -> Result<Value> {
    let file = file
        .as_object()
        .ok_or_else(|| Exit::invalid_response("GitHub pull request file must be an object"))?;
    let path = required_string(file, "path", "pull request file")?;
    let status = required_string(file, "changeType", "pull request file")?;
    let additions = required_count(file, "additions", "pull request file")?;
    let deletions = required_count(file, "deletions", "pull request file")?;
    Ok(Value::Object(Map::from_iter([
        ("path".to_owned(), Value::String(path.to_owned())),
        (
            "status".to_owned(),
            Value::String(status.to_ascii_uppercase()),
        ),
        ("additions".to_owned(), Value::from(additions)),
        ("deletions".to_owned(), Value::from(deletions)),
    ])))
}

fn required_count(value: &serde_json::Map<String, Value>, field: &str, label: &str) -> Result<u64> {
    value
        .get(field)
        .ok_or_else(|| Exit::invalid_response(format!("GitHub {label} omitted {field}")))?
        .as_u64()
        .ok_or_else(|| {
            Exit::invalid_response(format!("GitHub {label} field {field} must be an integer"))
        })
}

fn required_string<'a>(
    value: &'a serde_json::Map<String, Value>,
    field: &str,
    label: &str,
) -> Result<&'a str> {
    value
        .get(field)
        .ok_or_else(|| Exit::invalid_response(format!("GitHub {label} omitted {field}")))?
        .as_str()
        .ok_or_else(|| {
            Exit::invalid_response(format!("GitHub {label} field {field} must be a string"))
        })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn response() -> Value {
        json!({
            "changedFiles": 3,
            "additions": 12,
            "deletions": 4,
            "files": {
                "nodes": [
                    {"path":"src/new.rs","changeType":"ADDED","additions":10,"deletions":0},
                    {"path":"README.md","changeType":"modified","additions":2,"deletions":4}
                ],
                "totalCount": 3,
                "pageInfo": {"hasNextPage": true, "endCursor": "cursor"}
            }
        })
    }

    #[test]
    fn projects_files_and_pr_wide_summary_without_patch_fields() {
        let result = project(response(), 2).unwrap_or_else(|_| panic!("project files"));
        assert_eq!(
            result,
            json!({
                "files": [
                    {"path":"src/new.rs","status":"ADDED","additions":10,"deletions":0},
                    {"path":"README.md","status":"MODIFIED","additions":2,"deletions":4}
                ],
                "summary": {"total":3,"additions":12,"deletions":4},
                "totalCount":3,
                "truncated":true
            })
        );
        assert!(!serde_json::to_string(&result).unwrap().contains("patch"));
    }

    #[test]
    fn rejects_more_nodes_than_requested_limit() {
        let error = project(response(), 1).expect_err("limit must be enforced");
        assert!(error.stderr_line().contains("invalid number of nodes"));
    }
}
