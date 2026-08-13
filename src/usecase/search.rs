use serde_json::{Map, Value};

use crate::error::{Exit, Result};
use crate::github::rest;

pub fn issues(repository: &str, query: &str, limit: usize) -> Result<Value> {
    let response = rest::search_issues(repository, query, limit)?;
    project_search_response(response, repository, limit, ItemKind::Issue)
}

pub fn pull_requests(repository: &str, query: &str, limit: usize) -> Result<Value> {
    let response = rest::search_pull_requests(repository, query, limit)?;
    project_search_response(response, repository, limit, ItemKind::PullRequest)
}

pub fn for_commit(repository: &str, sha: &str, limit: usize) -> Result<Value> {
    let response = rest::pull_requests_for_commit(repository, sha, limit)?;
    let items = response
        .as_array()
        .ok_or_else(|| Exit::invalid_response("GitHub returned an invalid commit PR response"))?;
    let pull_requests = items
        .iter()
        .map(|item| project_item(item, ItemKind::PullRequest))
        .collect::<Result<Vec<_>>>()?;
    let truncated = pull_requests.len() > limit || (limit == 100 && pull_requests.len() == limit);
    let pull_requests = pull_requests.into_iter().take(limit).collect();

    Ok(Value::Object(Map::from_iter([
        (
            "repository".to_owned(),
            Value::String(repository.to_owned()),
        ),
        ("pullRequests".to_owned(), Value::Array(pull_requests)),
        ("truncated".to_owned(), Value::Bool(truncated)),
    ])))
}

#[derive(Clone, Copy)]
enum ItemKind {
    Issue,
    PullRequest,
}

fn project_search_response(
    response: Value,
    repository: &str,
    limit: usize,
    kind: ItemKind,
) -> Result<Value> {
    let response = response
        .as_object()
        .ok_or_else(|| Exit::invalid_response("GitHub returned an invalid search response"))?;
    let total_count = required_u64(response, "total_count", "search response")?;
    let incomplete_results = required_bool(response, "incomplete_results", "search response")?;
    let items = response
        .get("items")
        .ok_or_else(|| Exit::invalid_response("GitHub response omitted items"))?
        .as_array()
        .ok_or_else(|| Exit::invalid_response("GitHub search items must be an array"))?;
    if (items.len() as u64) > total_count {
        return Err(Exit::invalid_response(
            "GitHub search total_count is smaller than items",
        ));
    }
    let items = items
        .iter()
        .map(|item| project_item(item, kind))
        .collect::<Result<Vec<_>>>()?;
    let truncated = total_count > limit as u64 || items.len() > limit;
    let items = items.into_iter().take(limit).collect::<Vec<_>>();
    let item_field = match kind {
        ItemKind::Issue => "issues",
        ItemKind::PullRequest => "pullRequests",
    };

    Ok(Value::Object(Map::from_iter([
        (
            "repository".to_owned(),
            Value::String(repository.to_owned()),
        ),
        (item_field.to_owned(), Value::Array(items)),
        ("totalCount".to_owned(), Value::from(total_count)),
        ("truncated".to_owned(), Value::Bool(truncated)),
        (
            "incompleteResults".to_owned(),
            Value::Bool(incomplete_results),
        ),
    ])))
}

fn project_item(item: &Value, kind: ItemKind) -> Result<Value> {
    let item = item
        .as_object()
        .ok_or_else(|| Exit::invalid_response("GitHub search item must be an object"))?;
    match kind {
        ItemKind::Issue if item.contains_key("pull_request") => Err(Exit::invalid_response(
            "GitHub search returned a pull request for issue search",
        )),
        ItemKind::PullRequest if !item.contains_key("pull_request") => Err(Exit::invalid_response(
            "GitHub search returned an issue for pull request search",
        )),
        _ => {
            let mut result = Map::new();
            result.insert(
                "number".to_owned(),
                Value::from(required_u64(item, "number", "search item")?),
            );
            result.insert(
                "title".to_owned(),
                Value::String(required_string(item, "title", "search item")?.to_owned()),
            );
            result.insert(
                "url".to_owned(),
                Value::String(required_string(item, "html_url", "search item")?.to_owned()),
            );
            result.insert(
                "state".to_owned(),
                Value::String(normalize_enum(required_string(
                    item,
                    "state",
                    "search item",
                )?)),
            );
            result.insert(
                "updatedAt".to_owned(),
                Value::String(required_string(item, "updated_at", "search item")?.to_owned()),
            );
            match kind {
                ItemKind::Issue => {
                    result.insert(
                        "stateReason".to_owned(),
                        nullable_string(item, "state_reason", "search item")?,
                    );
                }
                ItemKind::PullRequest => {
                    result.insert(
                        "isDraft".to_owned(),
                        Value::Bool(required_bool(item, "draft", "search item")?),
                    );
                }
            }
            Ok(Value::Object(result))
        }
    }
}

fn required_u64(value: &serde_json::Map<String, Value>, field: &str, label: &str) -> Result<u64> {
    value
        .get(field)
        .ok_or_else(|| Exit::invalid_response(format!("GitHub {label} omitted {field}")))?
        .as_u64()
        .ok_or_else(|| {
            Exit::invalid_response(format!("GitHub {label} field {field} must be an integer"))
        })
}

fn required_bool(value: &serde_json::Map<String, Value>, field: &str, label: &str) -> Result<bool> {
    value
        .get(field)
        .ok_or_else(|| Exit::invalid_response(format!("GitHub {label} omitted {field}")))?
        .as_bool()
        .ok_or_else(|| {
            Exit::invalid_response(format!("GitHub {label} field {field} must be a boolean"))
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

fn nullable_string(
    value: &serde_json::Map<String, Value>,
    field: &str,
    label: &str,
) -> Result<Value> {
    match value
        .get(field)
        .ok_or_else(|| Exit::invalid_response(format!("GitHub {label} omitted {field}")))?
    {
        Value::Null => Ok(Value::Null),
        Value::String(value) => Ok(Value::String(value.clone())),
        _ => Err(Exit::invalid_response(format!(
            "GitHub {label} field {field} must be a string or null"
        ))),
    }
}

fn normalize_enum(value: &str) -> String {
    value.to_ascii_uppercase()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn issue(number: u64) -> Value {
        json!({
            "number": number,
            "title": "Issue",
            "html_url": "https://github.com/owner/repo/issues/1",
            "state": "open",
            "state_reason": null,
            "updated_at": "2026-08-13T00:00:00Z"
        })
    }

    fn pull_request(number: u64) -> Value {
        json!({
            "number": number,
            "title": "PR",
            "html_url": "https://github.com/owner/repo/pull/1",
            "state": "closed",
            "updated_at": "2026-08-13T00:00:00Z",
            "draft": false,
            "pull_request": {}
        })
    }

    #[test]
    fn projects_fixed_issue_summary_and_metadata() {
        let response = json!({
            "total_count": 1,
            "incomplete_results": false,
            "items": [issue(1)]
        });
        assert_eq!(
            project_search_response(response, "owner/repo", 20, ItemKind::Issue)
                .unwrap_or_else(|_| panic!("valid response")),
            json!({
                "repository": "owner/repo",
                "issues": [{
                    "number": 1,
                    "title": "Issue",
                    "url": "https://github.com/owner/repo/issues/1",
                    "state": "OPEN",
                    "updatedAt": "2026-08-13T00:00:00Z",
                    "stateReason": null
                }],
                "totalCount": 1,
                "truncated": false,
                "incompleteResults": false
            })
        );
    }

    #[test]
    fn projects_pull_request_summary_without_raw_fields() {
        let response = json!({
            "total_count": 1,
            "incomplete_results": true,
            "items": [pull_request(1)]
        });
        let result = project_search_response(response, "owner/repo", 20, ItemKind::PullRequest)
            .unwrap_or_else(|_| panic!("valid response"));
        assert_eq!(result["pullRequests"][0]["isDraft"], false);
        assert_eq!(result["incompleteResults"], true);
        assert_eq!(result["pullRequests"][0].as_object().map(Map::len), Some(6));
    }

    #[test]
    fn rejects_wrong_marker_and_malformed_wrapper() {
        let wrong_kind = json!({
            "total_count": 1,
            "incomplete_results": false,
            "items": [pull_request(1)]
        });
        assert!(project_search_response(wrong_kind, "owner/repo", 20, ItemKind::Issue).is_err());
        assert!(project_search_response(json!([]), "owner/repo", 20, ItemKind::Issue).is_err());
    }

    #[test]
    fn truncates_items_but_preserves_total_and_incomplete_results() {
        let response = json!({
            "total_count": 3,
            "incomplete_results": false,
            "items": [issue(1), issue(2), issue(3)]
        });
        let result = project_search_response(response, "owner/repo", 2, ItemKind::Issue)
            .unwrap_or_else(|_| panic!("valid response"));
        assert_eq!(result["issues"].as_array().map(Vec::len), Some(2));
        assert_eq!(result["totalCount"], 3);
        assert_eq!(result["truncated"], true);
        assert_eq!(result["incompleteResults"], false);
    }
}
