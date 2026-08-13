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

pub fn search_issues(repository: &str, query: &str, limit: usize) -> Result<Value> {
    search(repository, query, "issue", limit)
}

pub fn search_pull_requests(repository: &str, query: &str, limit: usize) -> Result<Value> {
    search(repository, query, "pr", limit)
}

pub fn pull_requests_for_commit(repository: &str, sha: &str, limit: usize) -> Result<Value> {
    let per_page = limit.saturating_add(1).min(100);
    let endpoint = format!("repos/{repository}/commits/{sha}/pulls?per_page={per_page}");
    cli::json_runtime(["api", "--method", "GET", &endpoint], None)
}

fn search(repository: &str, query: &str, kind: &str, limit: usize) -> Result<Value> {
    let per_page = limit.saturating_add(1).min(100);
    let query = format!("{query} repo:{repository} is:{kind}");
    let endpoint = format!(
        "search/issues?q={}&per_page={per_page}",
        percent_encode(&query)
    );
    cli::json_runtime(["api", "--method", "GET", &endpoint], None)
}

fn percent_encode(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push('%');
            encoded.push(hex_digit(byte >> 4));
            encoded.push(hex_digit(byte & 0x0f));
        }
    }
    encoded
}

fn hex_digit(value: u8) -> char {
    match value {
        0..=9 => (b'0' + value) as char,
        10..=15 => (b'A' + value - 10) as char,
        _ => unreachable!("hex digit is four bits"),
    }
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
