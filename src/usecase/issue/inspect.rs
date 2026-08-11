use std::thread;

use serde_json::{Map, Value};

use crate::error::Result;
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
    let issue = issue_result?;
    let comments = comments_result?;

    let mut result = Map::new();
    result.insert("issue".to_owned(), issue);
    result.insert("comments".to_owned(), Value::Array(comments));
    Ok(Value::Object(result))
}
