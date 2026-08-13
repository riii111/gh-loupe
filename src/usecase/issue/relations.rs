use serde_json::Value;

use crate::error::Result;
use crate::github::graphql::issue_relations;
use crate::model::Target;

pub fn execute(target: &Target, limit: i32) -> Result<Value> {
    issue_relations::execute(target, limit)
}
