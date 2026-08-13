use serde_json::{Map, Value, json};

use crate::error::{Exit, Result, RuntimeError};
use crate::model::Target;

use super::invalid_graphql_response;

const ISSUE_RELATIONS_QUERY: &str = r"
query($owner: String!, $name: String!, $number: Int!, $limit: Int!) {
  repository(owner: $owner, name: $name) {
    nameWithOwner
    issue(number: $number) {
      number
      parent {
        ...issueRelationSummary
      }
      subIssues(first: $limit) {
        nodes { ...issueRelationSummary }
        totalCount
      }
      blockedBy(first: $limit) {
        nodes { ...issueRelationSummary }
        totalCount
      }
      blocking(first: $limit) {
        nodes { ...issueRelationSummary }
        totalCount
      }
    }
  }
}

fragment issueRelationSummary on Issue {
  repository { nameWithOwner }
  number
  title
  url
  state
  stateReason
  assignees(first: 100) {
    nodes { login }
  }
}
";

pub fn execute(target: &Target, limit: i32) -> Result<Value> {
    let (owner, name) = target
        .repository
        .split_once('/')
        .expect("repository is validated");
    let data = super::query(
        ISSUE_RELATIONS_QUERY,
        &json!({
            "owner": owner,
            "name": name,
            "number": target.number.parse::<i32>().map_err(|_| invalid_graphql_response())?,
            "limit": limit,
        }),
    )?;
    let data = object(&data)?;
    let repository = required_value(data, "repository")?;
    if repository.is_null() {
        return Err(issue_not_found(target));
    }
    let repository = object(repository)?;
    let returned_repository = string_field(repository, "nameWithOwner")?;
    if !returned_repository.eq_ignore_ascii_case(&target.repository) {
        return Err(invalid_graphql_response());
    }
    let issue = required_value(repository, "issue")?;
    if issue.is_null() {
        return Err(issue_not_found(target));
    }
    let issue = object(issue)?;
    let returned_number = integer_field(issue, "number")?;
    let target_number = target
        .number
        .parse::<u64>()
        .map_err(|_| invalid_graphql_response())?;
    if returned_number != target_number {
        return Err(invalid_graphql_response());
    }

    let mut result = Map::new();
    result.insert(
        "repository".to_owned(),
        Value::String(target.repository.clone()),
    );
    result.insert("parent".to_owned(), project_parent(issue)?);
    result.insert(
        "subIssues".to_owned(),
        project_connection(issue, "subIssues", limit)?,
    );
    result.insert(
        "blockedBy".to_owned(),
        project_connection(issue, "blockedBy", limit)?,
    );
    result.insert(
        "blocking".to_owned(),
        project_connection(issue, "blocking", limit)?,
    );
    Ok(Value::Object(result))
}

fn project_parent(issue: &Map<String, Value>) -> Result<Value> {
    let parent = required_value(issue, "parent")?;
    if parent.is_null() {
        Ok(Value::Null)
    } else {
        project_summary(parent)
    }
}

fn project_connection(issue: &Map<String, Value>, field: &str, limit: i32) -> Result<Value> {
    let connection = object(required_value(issue, field)?)?;
    let total_count = integer_field(connection, "totalCount")?;
    let nodes = array(required_value(connection, "nodes")?)?;
    let expected_nodes =
        usize::try_from(total_count.min(limit as u64)).map_err(|_| invalid_graphql_response())?;
    if nodes.len() < expected_nodes || total_count < nodes.len() as u64 {
        return Err(invalid_graphql_response());
    }

    let items = nodes
        .iter()
        .map(project_summary)
        .collect::<Result<Vec<_>>>()?;
    let items = items.into_iter().take(expected_nodes).collect();
    let mut result = Map::new();
    result.insert("items".to_owned(), Value::Array(items));
    result.insert("totalCount".to_owned(), Value::from(total_count));
    result.insert(
        "truncated".to_owned(),
        Value::Bool(total_count > limit as u64),
    );
    Ok(Value::Object(result))
}

fn project_summary(value: &Value) -> Result<Value> {
    let value = object(value)?;
    let repository = string_field(
        object(required_value(value, "repository")?)?,
        "nameWithOwner",
    )?;
    if !valid_repository(repository) {
        return Err(invalid_graphql_response());
    }
    let number = integer_field(value, "number")?;
    if number == 0 {
        return Err(invalid_graphql_response());
    }
    let title = string_field(value, "title")?;
    let url = string_field(value, "url")?;
    let state = string_field(value, "state")?;
    let state_reason = nullable_string_field(value, "stateReason")?;
    let assignees = project_assignees(value)?;

    let mut result = Map::new();
    result.insert(
        "repository".to_owned(),
        Value::String(repository.to_owned()),
    );
    result.insert("number".to_owned(), Value::from(number));
    result.insert("title".to_owned(), Value::String(title.to_owned()));
    result.insert("url".to_owned(), Value::String(url.to_owned()));
    result.insert(
        "state".to_owned(),
        Value::String(state.to_ascii_uppercase()),
    );
    result.insert("stateReason".to_owned(), state_reason);
    result.insert("assignees".to_owned(), assignees);
    Ok(Value::Object(result))
}

fn project_assignees(value: &Map<String, Value>) -> Result<Value> {
    let connection = object(required_value(value, "assignees")?)?;
    let nodes = array(required_value(connection, "nodes")?)?;
    let mut logins = nodes
        .iter()
        .map(|node| string_field(object(node)?, "login").map(str::to_owned))
        .collect::<Result<Vec<_>>>()?;
    logins.sort_unstable();
    Ok(Value::Array(
        logins.into_iter().map(Value::String).collect(),
    ))
}

fn required_value<'a>(value: &'a Map<String, Value>, field: &str) -> Result<&'a Value> {
    value.get(field).ok_or_else(invalid_graphql_response)
}

fn object(value: &Value) -> Result<&Map<String, Value>> {
    value.as_object().ok_or_else(invalid_graphql_response)
}

fn array(value: &Value) -> Result<&Vec<Value>> {
    value.as_array().ok_or_else(invalid_graphql_response)
}

fn string_field<'a>(value: &'a Map<String, Value>, field: &str) -> Result<&'a str> {
    required_value(value, field)?
        .as_str()
        .ok_or_else(invalid_graphql_response)
}

fn nullable_string_field(value: &Map<String, Value>, field: &str) -> Result<Value> {
    match required_value(value, field)? {
        Value::Null => Ok(Value::Null),
        Value::String(value) => Ok(Value::String(value.to_ascii_uppercase())),
        _ => Err(invalid_graphql_response()),
    }
}

fn integer_field(value: &Map<String, Value>, field: &str) -> Result<u64> {
    required_value(value, field)?
        .as_u64()
        .ok_or_else(invalid_graphql_response)
}

fn valid_repository(value: &str) -> bool {
    let mut parts = value.split('/');
    matches!((parts.next(), parts.next(), parts.next()), (Some(owner), Some(name), None) if !owner.is_empty() && !name.is_empty())
}

fn issue_not_found(target: &Target) -> Exit {
    Exit::runtime(&RuntimeError::not_found(format!(
        "issue not found: {}#{}",
        target.repository, target.number
    )))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn summary(repository: &str, number: u64) -> Value {
        json!({
            "repository": {"nameWithOwner": repository},
            "number": number,
            "title": "Issue",
            "url": "https://github.com/owner/repository/issues/1",
            "state": "OPEN",
            "stateReason": null,
            "assignees": {"nodes": [{"login": "zoe"}, {"login": "alice"}]}
        })
    }

    #[test]
    fn projects_summary_without_raw_fields() {
        let projected =
            project_summary(&summary("owner/repository", 1)).unwrap_or_else(|_| panic!("summary"));
        assert_eq!(
            projected,
            json!({
                "repository": "owner/repository",
                "number": 1,
                "title": "Issue",
                "url": "https://github.com/owner/repository/issues/1",
                "state": "OPEN",
                "stateReason": null,
                "assignees": ["alice", "zoe"]
            })
        );
    }

    #[test]
    fn connection_preserves_node_order_and_reports_truncation() {
        let issue = json!({
            "subIssues": {"nodes": [summary("owner/repository", 1), summary("owner/repository", 2)], "totalCount": 3}
        });
        let result = project_connection(
            issue.as_object().unwrap_or_else(|| panic!("object")),
            "subIssues",
            2,
        )
        .unwrap_or_else(|_| panic!("connection"));
        assert_eq!(result["totalCount"], 3);
        assert_eq!(result["truncated"], true);
        assert_eq!(result["items"][0]["number"], 1);
        assert_eq!(result["items"][1]["number"], 2);
    }
}
