use serde_json::Value;

use crate::error::{Exit, Result};
use crate::model::Target;

use super::cli;

pub fn issue(target: &Target) -> Result<Value> {
    let issue = cli::json_runtime(
        [
            "api",
            &format!("repos/{}/issues/{}", target.repository, target.number),
        ],
        None,
    )?;
    if !issue.is_object() {
        return Err(Exit::invalid_response(
            "GitHub returned an invalid issue response",
        ));
    }
    Ok(issue)
}

pub fn issue_comments(target: &Target) -> Result<Vec<Value>> {
    let endpoint = format!(
        "repos/{}/issues/{}/comments?per_page=100",
        target.repository, target.number
    );
    let pages = cli::json_runtime(
        ["api", "--method", "GET", "--paginate", "--slurp", &endpoint],
        None,
    )?;
    flatten_pages(
        pages,
        "GitHub returned an invalid paginated comments response",
        "GitHub returned an invalid comments page",
        "GitHub returned invalid paginated items",
    )
}

pub fn pull_request_reviews(target: &Target) -> Result<Vec<Value>> {
    let endpoint = format!(
        "repos/{}/pulls/{}/reviews?per_page=100",
        target.repository, target.number
    );
    let pages = cli::json_runtime(
        ["api", "--method", "GET", "--paginate", "--slurp", &endpoint],
        None,
    )?;
    flatten_pages(
        pages,
        "GitHub returned an invalid reviews response",
        "GitHub returned an invalid reviews page",
        "GitHub returned an invalid review",
    )
}

pub fn pull_request_comments(target: &Target) -> Result<Vec<Value>> {
    let endpoint = format!(
        "repos/{}/issues/{}/comments?per_page=100",
        target.repository, target.number
    );
    let pages = cli::json_runtime(
        ["api", "--method", "GET", "--paginate", "--slurp", &endpoint],
        None,
    )?;
    flatten_pages(
        pages,
        "GitHub returned an invalid paginated comments response",
        "GitHub returned an invalid comments page",
        "GitHub returned an invalid conversation comment",
    )
}

fn flatten_pages(
    pages: Value,
    response_message: &str,
    page_message: &str,
    item_message: &str,
) -> Result<Vec<Value>> {
    let Value::Array(pages) = pages else {
        return Err(Exit::invalid_response(response_message));
    };
    let mut items = Vec::new();
    for page in pages {
        let Value::Array(page) = page else {
            return Err(Exit::invalid_response(page_message));
        };
        for item in page {
            if !item.is_object() {
                return Err(Exit::invalid_response(item_message));
            }
            items.push(item);
        }
    }
    Ok(items)
}
