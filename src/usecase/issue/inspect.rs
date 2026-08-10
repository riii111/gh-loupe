use serde_json::{Map, Value};

use crate::error::Result;
use crate::github::rest;
use crate::model::Target;

pub fn execute(target: &Target) -> Result<Value> {
    let mut result = Map::new();
    result.insert("issue".to_owned(), rest::issue(target)?);
    result.insert(
        "comments".to_owned(),
        Value::Array(rest::pages(&format!(
            "repos/{}/issues/{}/comments?per_page=100",
            target.repository, target.number
        ))?),
    );
    Ok(Value::Object(result))
}
