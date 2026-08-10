use serde_json::{Map, Value};

use crate::error::Result;
use crate::github::{graphql, rest};
use crate::model::Target;

pub fn execute(target: &Target, include_resolved: bool, compact: bool) -> Result<Value> {
    let (pull_request, mut threads) = graphql::pull_request_threads(target, include_resolved)?;
    if compact {
        remove_diff_hunks(&mut threads);
    }

    let mut result = Map::new();
    result.insert("pullRequest".to_owned(), pull_request);
    result.insert("checks".to_owned(), rest::pull_request_checks(target)?);
    result.insert(
        "conversationComments".to_owned(),
        Value::Array(rest::pages(&format!(
            "repos/{}/issues/{}/comments?per_page=100",
            target.repository, target.number
        ))?),
    );
    result.insert(
        "reviews".to_owned(),
        Value::Array(rest::pages(&format!(
            "repos/{}/pulls/{}/reviews?per_page=100",
            target.repository, target.number
        ))?),
    );
    result.insert("reviewThreads".to_owned(), Value::Array(threads));
    result.insert(
        "includesResolvedThreads".to_owned(),
        Value::Bool(include_resolved),
    );
    Ok(Value::Object(result))
}

fn remove_diff_hunks(threads: &mut [Value]) {
    for thread in threads {
        if let Some(comments) = thread.get_mut("comments").and_then(Value::as_array_mut) {
            for comment in comments {
                if let Some(comment) = comment.as_object_mut() {
                    comment.shift_remove("diffHunk");
                }
            }
        }
    }
}
